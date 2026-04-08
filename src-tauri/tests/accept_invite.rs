use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::{presets, Endpoint};
use iroh::protocol::Router;
use notes_core::{JoinSessionStore, ProjectManager};
use notes_lib::invite_accept::{
    accept_invite_impl, finalize_accepted_invite, perform_initial_invite_sync,
    persist_commit_confirmed_payload, persist_payload_staged_session,
    register_project_sync_objects, resume_join_sessions, stage_accepted_invite,
    OwnerInviteCoordinator, OwnerInvitePersistence, ProjectSyncObserver, ProjectSyncResolverImpl,
    SessionSecretCache,
};
use notes_lib::persist_manifest_update_for_sync;
use notes_sync::invite::{
    InviteAcceptanceContext, InviteCode, InviteHandler, InviteLifecycleHandler, InvitePayload,
    InvitePersistenceHandler, InviteState, PendingInvite,
};
use notes_sync::peer_manager::PeerManager;
use notes_sync::sync_engine::{SyncEngine, NOTES_SYNC_ALPN};
use notes_sync::SyncStateStore;
use tempfile::TempDir;

static ACCEPT_INVITE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct TestNode {
    _dir: Option<TempDir>,
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    endpoint: Endpoint,
    router: Router,
    invite_handler: Arc<InviteHandler>,
    join_session_store: Arc<JoinSessionStore>,
    session_secret_cache: Arc<SessionSecretCache>,
}

impl TestNode {
    async fn new(
        lookup: &MemoryLookup,
        lifecycle: Option<Arc<dyn InviteLifecycleHandler>>,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self::new_at_path(dir.path().to_path_buf(), Some(dir), lookup, lifecycle).await
    }

    async fn new_at_path(
        base_dir: std::path::PathBuf,
        dir: Option<TempDir>,
        lookup: &MemoryLookup,
        lifecycle: Option<Arc<dyn InviteLifecycleHandler>>,
    ) -> Self {
        let p2p_dir = base_dir.join(".p2p");
        std::fs::create_dir_all(&p2p_dir).unwrap();

        let project_manager = Arc::new(ProjectManager::new(base_dir.clone()));

        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).unwrap();
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(iroh::SecretKey::from_bytes(&secret))
            .relay_mode(iroh::RelayMode::Disabled)
            .address_lookup(lookup.clone())
            .bind()
            .await
            .unwrap();
        lookup.add_endpoint_info(endpoint.addr());

        let sync_state_store = Arc::new(SyncStateStore::new(p2p_dir));
        let mut sync_engine = Arc::new(SyncEngine::new());
        Arc::get_mut(&mut sync_engine)
            .unwrap()
            .set_sync_state_store(Arc::clone(&sync_state_store));
        let mut peer_manager =
            Arc::new(PeerManager::new(endpoint.clone(), Arc::clone(&sync_engine)));
        Arc::get_mut(&mut peer_manager)
            .unwrap()
            .set_project_sync_resolver(Arc::new(ProjectSyncResolverImpl::new(Arc::clone(
                &project_manager,
            ))));
        sync_engine.set_change_handler(Arc::new(ProjectSyncObserver::new(
            Arc::clone(&project_manager),
            Arc::downgrade(&sync_engine),
            Arc::downgrade(&peer_manager),
            endpoint.id(),
        )));

        let coordinator = Arc::new(OwnerInviteCoordinator::new(
            Arc::clone(&project_manager),
            Arc::clone(&sync_engine),
            Arc::clone(&peer_manager),
            endpoint.id(),
        ));
        let join_session_store = Arc::new(JoinSessionStore::new(base_dir.clone()));
        let session_secret_cache = Arc::new(SessionSecretCache::default());
        let mut handler = InviteHandler::new();
        handler.set_lifecycle_handler(lifecycle.unwrap_or(coordinator));
        handler.set_persistence_handler(Arc::new(OwnerInvitePersistence::new(
            base_dir,
            endpoint.id().to_string(),
        )));
        let invite_handler = Arc::new(handler);

        let router = Router::builder(endpoint.clone())
            .accept(NOTES_SYNC_ALPN, Arc::clone(&sync_engine))
            .accept(notes_sync::invite::INVITE_ALPN, Arc::clone(&invite_handler))
            .spawn();

        Self {
            _dir: dir,
            project_manager,
            sync_engine,
            peer_manager,
            endpoint,
            router,
            invite_handler,
            join_session_store,
            session_secret_cache,
        }
    }

    async fn shutdown(self) {
        self.router.shutdown().await.ok();
        self.endpoint.close().await;
    }

    async fn register_project_sync(&self, project: &str) {
        if let Ok(files) = self.project_manager.list_files(project).await {
            for file in files {
                let _ = self.project_manager.open_doc(project, &file.id).await;
            }
        }
        let _ = register_project_sync_objects(
            &self.project_manager,
            &self.sync_engine,
            &self.endpoint.id(),
            project,
        )
        .await
        .unwrap();
    }

    async fn add_manual_todo(
        &self,
        project: &str,
        text: &str,
        linked_doc_id: Option<&str>,
    ) -> String {
        let manifest_arc = self.project_manager.get_manifest_for_ui(project).unwrap();
        let (todo_id, data) = {
            let mut manifest = manifest_arc.write().await;
            let todo_id = manifest
                .add_todo(text, &self.endpoint.id().to_string(), linked_doc_id)
                .unwrap();
            let data = manifest.save();
            (todo_id.to_string(), data)
        };

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, uuid::Uuid)>(4);
        let manifest_doc_id = persist_manifest_update_for_sync(
            &self.project_manager,
            &self.sync_engine,
            &sync_tx,
            &self.endpoint.id().to_string(),
            project,
            &data,
        )
        .await
        .unwrap();
        let results = self
            .peer_manager
            .sync_doc_with_project_peers(project, manifest_doc_id)
            .await;
        assert!(results.iter().all(|(_, result)| result.is_ok()));

        todo_id
    }

    async fn toggle_manual_todo(&self, project: &str, todo_id: &str) {
        let manifest_arc = self.project_manager.get_manifest_for_ui(project).unwrap();
        let data = {
            let mut manifest = manifest_arc.write().await;
            manifest.toggle_todo(todo_id).unwrap();
            manifest.save()
        };

        let (sync_tx, _sync_rx) = tokio::sync::mpsc::channel::<(String, uuid::Uuid)>(4);
        let manifest_doc_id = persist_manifest_update_for_sync(
            &self.project_manager,
            &self.sync_engine,
            &sync_tx,
            &self.endpoint.id().to_string(),
            project,
            &data,
        )
        .await
        .unwrap();
        let results = self
            .peer_manager
            .sync_doc_with_project_peers(project, manifest_doc_id)
            .await;
        assert!(results.iter().all(|(_, result)| result.is_ok()));
    }

    async fn list_manual_todos(&self, project: &str) -> Vec<notes_core::TodoItem> {
        let manifest_arc = self.project_manager.get_manifest_for_ui(project).unwrap();
        let manifest = manifest_arc.read().await;
        manifest.list_todos().unwrap()
    }
}

struct FailingLifecycle;

impl InviteLifecycleHandler for FailingLifecycle {
    fn prepare_payload<'a>(
        &'a self,
        _ctx: &'a InviteAcceptanceContext,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<InvitePayload, notes_sync::invite::InviteError>> + Send + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(InvitePayload {
                invite_id: "failing-invite-id".into(),
                session_id: "failing-session-id".into(),
                project_id: "failing-project-id".into(),
                project_name: "shared".into(),
                role: "editor".into(),
                manifest_hex: String::new(),
                owner_x25519_public_hex: String::new(),
                epoch_key_hex: String::new(),
                epoch: 0,
            })
        })
    }

    fn commit_acceptance<'a>(
        &'a self,
        _ctx: &'a InviteAcceptanceContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), notes_sync::invite::InviteError>> + Send + 'a>>
    {
        Box::pin(async move {
            Err(notes_sync::invite::InviteError::Lifecycle(
                "forced commit failure".into(),
            ))
        })
    }
}

fn add_pending_invite(
    handler: &InviteHandler,
    passphrase: &str,
    owner_peer_id: &str,
    project: &str,
    project_id: &str,
    role: &str,
) {
    handler.add_pending(
        passphrase.to_string(),
        PendingInvite {
            invite_id: uuid::Uuid::new_v4().to_string(),
            code: InviteCode {
                passphrase: passphrase.to_string(),
                peer_id: owner_peer_id.to_string(),
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
            },
            created_at: std::time::Instant::now(),
            attempts: 0,
            project_name: project.to_string(),
            project_id: project_id.to_string(),
            invite_role: role.to_string(),
            state: InviteState::Open,
        },
    );
}

#[tokio::test]
async fn accept_invite_happy_path_installs_project() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner
        .project_manager
        .create_note("shared", "hello.md")
        .await
        .unwrap();

    add_pending_invite(
        &owner.invite_handler,
        "alpha-beta-gamma-delta-epsilon-zeta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&invitee.session_secret_cache),
        invitee.endpoint.clone(),
        None,
        "alpha-beta-gamma-delta-epsilon-zeta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;

    assert_eq!(result.project_name, "shared");
    assert_eq!(result.role, "editor");
    assert!(invitee
        .project_manager
        .has_cached_project_x25519_identity("shared"));
    assert!(invitee.project_manager.get_epoch_keys("shared").is_ok());

    let files = invitee.project_manager.list_files("shared").await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "hello.md");

    let peers = owner
        .project_manager
        .get_project_peers("shared")
        .await
        .unwrap();
    assert!(peers
        .iter()
        .any(|peer| peer.peer_id == invitee.endpoint.id().to_string()));

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn accept_invite_existing_note_text_is_available_after_bootstrap() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    let doc_id = owner
        .project_manager
        .create_note("shared", "hello.md")
        .await
        .unwrap();
    owner
        .project_manager
        .doc_store()
        .replace_text(&doc_id, "hello from owner")
        .await
        .unwrap();
    owner
        .project_manager
        .save_doc("shared", &doc_id)
        .await
        .unwrap();
    owner.register_project_sync("shared").await;

    add_pending_invite(
        &owner.invite_handler,
        "text-bootstrap-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&invitee.session_secret_cache),
        invitee.endpoint.clone(),
        None,
        "text-bootstrap-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;

    let files = invitee
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap();
    let invited_doc = files.iter().find(|file| file.path == "hello.md").unwrap();
    let text = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if invitee
                .project_manager
                .open_doc(&result.project_name, &invited_doc.id)
                .await
                .is_err()
            {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            let text = invitee
                .project_manager
                .get_doc_text(&invited_doc.id)
                .await
                .unwrap();
            if text == "hello from owner" {
                break text;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(text, "hello from owner");

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn accept_invite_bootstrap_includes_existing_manual_todos() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner
        .add_manual_todo("shared", "bootstrap todo", None)
        .await;
    owner.register_project_sync("shared").await;

    add_pending_invite(
        &owner.invite_handler,
        "bootstrap-todo-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&invitee.session_secret_cache),
        invitee.endpoint.clone(),
        None,
        "bootstrap-todo-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();

    invitee
        .project_manager
        .open_project(&result.project_name)
        .await
        .unwrap();

    let todos = invitee.list_manual_todos(&result.project_name).await;
    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0].text, "bootstrap todo");
    assert_eq!(todos[0].linked_doc_id, None);

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn accept_invite_returns_before_full_bootstrap_hydration() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    let doc_id = owner
        .project_manager
        .create_note("shared", "hello.md")
        .await
        .unwrap();
    owner
        .project_manager
        .doc_store()
        .replace_text(&doc_id, "hello from owner")
        .await
        .unwrap();
    owner
        .project_manager
        .save_doc("shared", &doc_id)
        .await
        .unwrap();
    owner.register_project_sync("shared").await;

    add_pending_invite(
        &owner.invite_handler,
        "accept-returns-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = tokio::time::timeout(
        Duration::from_secs(10),
        accept_invite_impl(
            Arc::clone(&invitee.project_manager),
            Arc::clone(&invitee.sync_engine),
            Arc::clone(&invitee.peer_manager),
            Arc::clone(&invitee.join_session_store),
            Arc::clone(&invitee.session_secret_cache),
            invitee.endpoint.clone(),
            None,
            "accept-returns-alpha-beta".into(),
            owner.endpoint.id().to_string(),
        ),
    )
    .await;

    assert!(
        result.is_ok(),
        "accept_invite should return before full bootstrap hydration"
    );

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn accept_invite_bootstrap_makes_manifest_files_visible_before_doc_content_is_hydrated() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    let doc_id = owner
        .project_manager
        .create_note("shared", "hello.md")
        .await
        .unwrap();
    owner
        .project_manager
        .doc_store()
        .replace_text(&doc_id, "hello from owner")
        .await
        .unwrap();
    owner
        .project_manager
        .save_doc("shared", &doc_id)
        .await
        .unwrap();
    owner.register_project_sync("shared").await;

    add_pending_invite(
        &owner.invite_handler,
        "visible-before-hydrated-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&invitee.session_secret_cache),
        invitee.endpoint.clone(),
        None,
        "visible-before-hydrated-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();

    let files = invitee
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap();
    let invited_doc = files.iter().find(|file| file.path == "hello.md").unwrap();
    invitee
        .project_manager
        .open_doc(&result.project_name, &invited_doc.id)
        .await
        .unwrap();
    let text = invitee
        .project_manager
        .get_doc_text(&invited_doc.id)
        .await
        .unwrap();

    assert!(files.iter().any(|file| file.path == "hello.md"));
    assert_ne!(text, "hello from owner");

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn accept_invite_with_multiple_existing_notes_lists_all_paths_before_content_polling() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    let first = owner
        .project_manager
        .create_note("shared", "first.md")
        .await
        .unwrap();
    let second = owner
        .project_manager
        .create_note("shared", "second.md")
        .await
        .unwrap();
    owner
        .project_manager
        .doc_store()
        .replace_text(&first, "first text")
        .await
        .unwrap();
    owner
        .project_manager
        .doc_store()
        .replace_text(&second, "second text")
        .await
        .unwrap();
    owner
        .project_manager
        .save_doc("shared", &first)
        .await
        .unwrap();
    owner
        .project_manager
        .save_doc("shared", &second)
        .await
        .unwrap();
    owner.register_project_sync("shared").await;

    add_pending_invite(
        &owner.invite_handler,
        "multiple-existing-notes-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&invitee.session_secret_cache),
        invitee.endpoint.clone(),
        None,
        "multiple-existing-notes-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();

    let files = invitee
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap();
    let paths = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();

    assert!(paths.contains(&"first.md"));
    assert!(paths.contains(&"second.md"));

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn bootstrap_hydration_does_not_create_new_manifest_entries() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    let doc_id = owner
        .project_manager
        .create_note("shared", "hello.md")
        .await
        .unwrap();
    owner
        .project_manager
        .doc_store()
        .replace_text(&doc_id, "hello from owner")
        .await
        .unwrap();
    owner
        .project_manager
        .save_doc("shared", &doc_id)
        .await
        .unwrap();
    owner.register_project_sync("shared").await;

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let payload = coordinator
        .build_payload(&InviteAcceptanceContext {
            invite_id: "bootstrap-manifest-stability-id".into(),
            session_id: "bootstrap-manifest-stability-session".into(),
            passphrase: "bootstrap-manifest-stability-passphrase".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: invitee.endpoint.id().to_string(),
        })
        .await
        .unwrap();

    let staged = stage_accepted_invite(
        Arc::clone(&invitee.project_manager),
        payload,
        owner.endpoint.id(),
    )
    .await
    .unwrap();
    let (result, doc_ids) = finalize_accepted_invite(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.clone(),
        staged,
    )
    .await
    .unwrap();

    let before = invitee
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap()
        .into_iter()
        .map(|file| (file.id, file.path))
        .collect::<Vec<_>>();

    perform_initial_invite_sync(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.id(),
        &result.project_name,
        &doc_ids,
    )
    .await;

    let after = invitee
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap()
        .into_iter()
        .map(|file| (file.id, file.path))
        .collect::<Vec<_>>();

    assert_eq!(before, after);

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn perform_initial_invite_sync_bootstraps_existing_note_manifest_then_content() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    let doc_id = owner
        .project_manager
        .create_note("shared", "hello.md")
        .await
        .unwrap();
    owner.register_project_sync("shared").await;
    owner
        .project_manager
        .doc_store()
        .replace_text(&doc_id, "hello from owner")
        .await
        .unwrap();
    owner
        .project_manager
        .save_doc("shared", &doc_id)
        .await
        .unwrap();

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let payload = coordinator
        .build_payload(&InviteAcceptanceContext {
            invite_id: "invite-bootstrap-id".into(),
            session_id: "session-bootstrap-id".into(),
            passphrase: "bootstrap-passphrase".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: invitee.endpoint.id().to_string(),
        })
        .await
        .unwrap();
    coordinator
        .apply_acceptance_commit(&InviteAcceptanceContext {
            invite_id: "invite-bootstrap-id".into(),
            session_id: "session-bootstrap-id".into(),
            passphrase: "bootstrap-passphrase".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: invitee.endpoint.id().to_string(),
        })
        .await
        .unwrap();

    let staged = stage_accepted_invite(
        Arc::clone(&invitee.project_manager),
        payload,
        owner.endpoint.id(),
    )
    .await
    .unwrap();
    let (result, doc_ids) = finalize_accepted_invite(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.clone(),
        staged,
    )
    .await
    .unwrap();

    let files = invitee
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap();
    assert!(files.iter().any(|file| file.path == "hello.md"));

    for doc_id in &doc_ids {
        let results = invitee
            .peer_manager
            .sync_doc_with_project_peers(&result.project_name, *doc_id)
            .await;
        let _results = results;
    }
    perform_initial_invite_sync(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.id(),
        &result.project_name,
        &doc_ids,
    )
    .await;

    let invited_doc = invitee
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap()
        .into_iter()
        .find(|file| file.path == "hello.md")
        .unwrap();
    invitee
        .project_manager
        .open_doc(&result.project_name, &invited_doc.id)
        .await
        .unwrap();
    let text = invitee
        .project_manager
        .get_doc_text(&invited_doc.id)
        .await
        .unwrap();

    assert_eq!(text, "hello from owner");

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn editor_created_file_propagates_to_owner_manifest_and_content() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let editor = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner.register_project_sync("shared").await;

    add_pending_invite(
        &owner.invite_handler,
        "editor-create-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&editor.project_manager),
        Arc::clone(&editor.sync_engine),
        Arc::clone(&editor.peer_manager),
        Arc::clone(&editor.join_session_store),
        Arc::clone(&editor.session_secret_cache),
        editor.endpoint.clone(),
        None,
        "editor-create-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;

    let new_doc_id = editor
        .project_manager
        .create_note(&result.project_name, "editor-note.md")
        .await
        .unwrap();
    let manifest_doc_id = editor
        .project_manager
        .manifest_doc_id(&result.project_name)
        .await
        .unwrap();
    let _manifest_doc_id = manifest_doc_id;
    editor
        .project_manager
        .doc_store()
        .replace_text(&new_doc_id, "editor note body")
        .await
        .unwrap();
    editor
        .project_manager
        .save_doc(&result.project_name, &new_doc_id)
        .await
        .unwrap();
    editor.register_project_sync(&result.project_name).await;
    let _sync_results = editor
        .peer_manager
        .sync_doc_with_project_peers(&result.project_name, new_doc_id)
        .await;

    let owner_text = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let owner_files = owner.project_manager.list_files("shared").await.unwrap();
            if let Some(owner_doc) = owner_files
                .iter()
                .find(|file| file.path == "editor-note.md")
            {
                if owner
                    .project_manager
                    .open_doc("shared", &owner_doc.id)
                    .await
                    .is_err()
                {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                let owner_text = owner
                    .project_manager
                    .get_doc_text(&owner_doc.id)
                    .await
                    .unwrap();
                if owner_text == "editor note body" {
                    break owner_text;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(owner_text, "editor note body");

    owner.shutdown().await;
    editor.shutdown().await;
}

#[tokio::test]
async fn editor_first_file_on_empty_project_propagates_manifest_before_content() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let editor = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let payload = coordinator
        .build_payload(&InviteAcceptanceContext {
            invite_id: "invite-empty-manifest-id".into(),
            session_id: "session-empty-manifest-id".into(),
            passphrase: "unused".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: editor.endpoint.id().to_string(),
        })
        .await
        .unwrap();
    coordinator
        .apply_acceptance_commit(&InviteAcceptanceContext {
            invite_id: "invite-empty-manifest-id".into(),
            session_id: "session-empty-manifest-id".into(),
            passphrase: "unused".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: editor.endpoint.id().to_string(),
        })
        .await
        .unwrap();
    let (result, _) = finalize_accepted_invite(
        Arc::clone(&editor.project_manager),
        Arc::clone(&editor.sync_engine),
        Arc::clone(&editor.peer_manager),
        editor.endpoint.clone(),
        stage_accepted_invite(
            Arc::clone(&editor.project_manager),
            payload,
            owner.endpoint.id(),
        )
        .await
        .unwrap(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;

    let new_doc_id = editor
        .project_manager
        .create_note(&result.project_name, "editor-note.md")
        .await
        .unwrap();
    editor.register_project_sync(&result.project_name).await;

    let manifest_doc_id = editor
        .project_manager
        .manifest_doc_id(&result.project_name)
        .await
        .unwrap();
    let manifest_results = editor
        .peer_manager
        .sync_doc_with_project_peers(&result.project_name, manifest_doc_id)
        .await;
    let _editor_manifest_files = editor
        .project_manager
        .list_files(&result.project_name)
        .await
        .unwrap();
    let _manifest_results = manifest_results;

    let owner_files = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let owner_files = owner.project_manager.list_files("shared").await.unwrap();
            if owner_files.iter().any(|file| file.path == "editor-note.md") {
                break owner_files;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    let owner_doc = owner_files
        .iter()
        .find(|file| file.path == "editor-note.md")
        .unwrap();
    assert_eq!(owner_doc.id, new_doc_id);
    owner
        .project_manager
        .open_doc("shared", &owner_doc.id)
        .await
        .unwrap();
    let owner_text = owner
        .project_manager
        .get_doc_text(&owner_doc.id)
        .await
        .unwrap();
    assert_ne!(owner_text, "editor note body");

    owner.shutdown().await;
    editor.shutdown().await;
}

#[tokio::test]
async fn editor_first_file_on_empty_project_hydrates_content_after_doc_sync() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let editor = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let payload = coordinator
        .build_payload(&InviteAcceptanceContext {
            invite_id: "invite-empty-content-id".into(),
            session_id: "session-empty-content-id".into(),
            passphrase: "unused".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: editor.endpoint.id().to_string(),
        })
        .await
        .unwrap();
    coordinator
        .apply_acceptance_commit(&InviteAcceptanceContext {
            invite_id: "invite-empty-content-id".into(),
            session_id: "session-empty-content-id".into(),
            passphrase: "unused".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: editor.endpoint.id().to_string(),
        })
        .await
        .unwrap();
    let (result, _) = finalize_accepted_invite(
        Arc::clone(&editor.project_manager),
        Arc::clone(&editor.sync_engine),
        Arc::clone(&editor.peer_manager),
        editor.endpoint.clone(),
        stage_accepted_invite(
            Arc::clone(&editor.project_manager),
            payload,
            owner.endpoint.id(),
        )
        .await
        .unwrap(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;

    let new_doc_id = editor
        .project_manager
        .create_note(&result.project_name, "editor-note.md")
        .await
        .unwrap();
    editor
        .project_manager
        .doc_store()
        .replace_text(&new_doc_id, "editor note body")
        .await
        .unwrap();
    editor
        .project_manager
        .save_doc(&result.project_name, &new_doc_id)
        .await
        .unwrap();
    editor.register_project_sync(&result.project_name).await;

    let manifest_doc_id = editor
        .project_manager
        .manifest_doc_id(&result.project_name)
        .await
        .unwrap();
    let _ = editor
        .peer_manager
        .sync_doc_with_project_peers(&result.project_name, manifest_doc_id)
        .await;
    let _ = editor
        .peer_manager
        .sync_doc_with_project_peers(&result.project_name, new_doc_id)
        .await;

    let owner_text = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let owner_files = owner.project_manager.list_files("shared").await.unwrap();
            if let Some(owner_doc) = owner_files
                .iter()
                .find(|file| file.path == "editor-note.md")
            {
                if owner
                    .project_manager
                    .open_doc("shared", &owner_doc.id)
                    .await
                    .is_err()
                {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                let owner_text = owner
                    .project_manager
                    .get_doc_text(&owner_doc.id)
                    .await
                    .unwrap();
                if owner_text == "editor note body" {
                    break owner_text;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(owner_text, "editor note body");

    owner.shutdown().await;
    editor.shutdown().await;
}

#[tokio::test]
async fn manual_todo_add_and_toggle_propagate_from_editor_to_owner() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let editor = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();

    add_pending_invite(
        &owner.invite_handler,
        "manual-todo-sync-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&editor.project_manager),
        Arc::clone(&editor.sync_engine),
        Arc::clone(&editor.peer_manager),
        Arc::clone(&editor.join_session_store),
        Arc::clone(&editor.session_secret_cache),
        editor.endpoint.clone(),
        None,
        "manual-todo-sync-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;
    editor
        .project_manager
        .open_project(&result.project_name)
        .await
        .unwrap();
    editor.register_project_sync(&result.project_name).await;
    editor
        .peer_manager
        .add_peer_to_project(&result.project_name, owner.endpoint.id());

    let todo_id = editor
        .add_manual_todo(&result.project_name, "shared todo", None)
        .await;

    let propagated = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let todos = owner.list_manual_todos("shared").await;
            if todos
                .iter()
                .any(|todo| todo.id.to_string() == todo_id && todo.text == "shared todo")
            {
                break todos;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(propagated.len(), 1);
    assert!(!propagated[0].done);

    editor
        .toggle_manual_todo(&result.project_name, &todo_id)
        .await;

    let owner_todos = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let todos = owner.list_manual_todos("shared").await;
            if todos
                .iter()
                .any(|todo| todo.id.to_string() == todo_id && todo.done)
            {
                break todos;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(owner_todos.len(), 1);
    assert!(owner_todos[0].done);

    owner.shutdown().await;
    editor.shutdown().await;
}

#[tokio::test]
async fn editor_renamed_file_propagates_without_changing_content() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let editor = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();

    add_pending_invite(
        &owner.invite_handler,
        "editor-rename-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&editor.project_manager),
        Arc::clone(&editor.sync_engine),
        Arc::clone(&editor.peer_manager),
        Arc::clone(&editor.join_session_store),
        Arc::clone(&editor.session_secret_cache),
        editor.endpoint.clone(),
        None,
        "editor-rename-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;

    let new_doc_id = editor
        .project_manager
        .create_note(&result.project_name, "old-name.md")
        .await
        .unwrap();
    editor
        .project_manager
        .doc_store()
        .replace_text(&new_doc_id, "rename me")
        .await
        .unwrap();
    editor
        .project_manager
        .save_doc(&result.project_name, &new_doc_id)
        .await
        .unwrap();
    editor.register_project_sync(&result.project_name).await;
    editor
        .project_manager
        .rename_note(&result.project_name, &new_doc_id, "new-name.md")
        .await
        .unwrap();
    let _ = editor
        .peer_manager
        .sync_doc_with_project_peers(&result.project_name, new_doc_id)
        .await;

    let owner_text = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let owner_files = owner.project_manager.list_files("shared").await.unwrap();
            if let Some(owner_doc) = owner_files.iter().find(|file| file.path == "new-name.md") {
                if owner
                    .project_manager
                    .open_doc("shared", &owner_doc.id)
                    .await
                    .is_err()
                {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                let owner_text = owner
                    .project_manager
                    .get_doc_text(&owner_doc.id)
                    .await
                    .unwrap();
                if owner_text == "rename me" {
                    break owner_text;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(owner_text, "rename me");

    owner.shutdown().await;
    editor.shutdown().await;
}

#[tokio::test]
async fn editor_deleted_file_is_removed_for_owner() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let editor = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();

    add_pending_invite(
        &owner.invite_handler,
        "editor-delete-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let result = accept_invite_impl(
        Arc::clone(&editor.project_manager),
        Arc::clone(&editor.sync_engine),
        Arc::clone(&editor.peer_manager),
        Arc::clone(&editor.join_session_store),
        Arc::clone(&editor.session_secret_cache),
        editor.endpoint.clone(),
        None,
        "editor-delete-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();
    owner.register_project_sync("shared").await;

    let new_doc_id = editor
        .project_manager
        .create_note(&result.project_name, "delete-me.md")
        .await
        .unwrap();
    editor
        .project_manager
        .save_doc(&result.project_name, &new_doc_id)
        .await
        .unwrap();
    editor.register_project_sync(&result.project_name).await;
    editor
        .project_manager
        .delete_note(&result.project_name, &new_doc_id)
        .await
        .unwrap();
    let _ = editor
        .peer_manager
        .sync_doc_with_project_peers(&result.project_name, new_doc_id)
        .await;

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let owner_files = owner.project_manager.list_files("shared").await.unwrap();
            if owner_files.iter().all(|file| file.path != "delete-me.md") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .unwrap();

    owner.shutdown().await;
    editor.shutdown().await;
}

#[tokio::test]
async fn accept_invite_commit_failure_does_not_install_project() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, Some(Arc::new(FailingLifecycle))).await;
    let invitee = TestNode::new(&lookup, None).await;

    add_pending_invite(
        &owner.invite_handler,
        "commit-fails-alpha-beta",
        &owner.endpoint.id().to_string(),
        "shared",
        "shared-project-id",
        "editor",
    );

    let err = accept_invite_impl(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&invitee.session_secret_cache),
        invitee.endpoint.clone(),
        None,
        "commit-fails-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap_err();

    let err_text = format!("{err}");
    assert!(!err_text.is_empty());
    assert!(invitee
        .project_manager
        .open_project("shared")
        .await
        .is_err());

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn install_accepted_invite_uses_distinct_local_name_when_project_exists() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner.register_project_sync("shared").await;
    owner
        .project_manager
        .create_note("shared", "remote.md")
        .await
        .unwrap();

    invitee
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    invitee
        .project_manager
        .open_project("shared")
        .await
        .unwrap();
    invitee.register_project_sync("shared").await;
    invitee
        .project_manager
        .create_note("shared", "local-only.md")
        .await
        .unwrap();

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let payload = coordinator
        .build_payload(&InviteAcceptanceContext {
            invite_id: "invite-1".into(),
            session_id: "test-session".into(),
            passphrase: "phase-two-overwrite".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: invitee.endpoint.id().to_string(),
        })
        .await
        .unwrap();

    let staged = stage_accepted_invite(
        Arc::clone(&invitee.project_manager),
        payload,
        owner.endpoint.id(),
    )
    .await
    .unwrap();

    let (result, _) = finalize_accepted_invite(
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.clone(),
        staged,
    )
    .await
    .unwrap();

    assert_eq!(result.project_name, "shared-1");
    let files = invitee
        .project_manager
        .list_files("shared-1")
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "remote.md");

    let local_files = invitee.project_manager.list_files("shared").await.unwrap();
    assert_eq!(local_files.len(), 1);
    assert_eq!(local_files[0].path, "local-only.md");

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[test]
fn owner_invite_persistence_restores_runtime_invites() {
    let dir = tempfile::tempdir().unwrap();
    let persistence = OwnerInvitePersistence::new(dir.path().to_path_buf(), "owner-peer".into());
    let pending = PendingInvite {
        invite_id: uuid::Uuid::new_v4().to_string(),
        code: InviteCode {
            passphrase: "persisted-passphrase".into(),
            peer_id: "owner-peer".into(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
        },
        created_at: std::time::Instant::now(),
        attempts: 1,
        project_name: "shared".into(),
        project_id: "shared-project-id".into(),
        invite_role: "editor".into(),
        state: InviteState::CommittedPendingAck(notes_sync::invite::InviteCommittedPendingAck {
            session_id: "session-1".into(),
            invitee_peer_id: "peer-1".into(),
            committed_at: std::time::Instant::now(),
        }),
    };
    persistence
        .sync_invite(&pending.code.passphrase, &pending)
        .unwrap();

    let restored = persistence.load_runtime_invites().unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].0, "persisted-passphrase");
    assert!(matches!(
        restored[0].1.state,
        InviteState::CommittedPendingAck(_)
    ));
}

#[test]
fn owner_invite_persistence_restores_open_invites() {
    let dir = tempfile::tempdir().unwrap();
    let persistence = OwnerInvitePersistence::new(dir.path().to_path_buf(), "owner-peer".into());
    let pending = PendingInvite {
        invite_id: uuid::Uuid::new_v4().to_string(),
        code: InviteCode {
            passphrase: "fresh-passphrase".into(),
            peer_id: "owner-peer".into(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
        },
        created_at: std::time::Instant::now(),
        attempts: 0,
        project_name: "shared".into(),
        project_id: "shared-project-id".into(),
        invite_role: "editor".into(),
        state: InviteState::Open,
    };
    persistence
        .sync_invite(&pending.code.passphrase, &pending)
        .unwrap();

    let restored = persistence.load_runtime_invites().unwrap();
    assert_eq!(restored.len(), 1);
    assert!(matches!(restored[0].1.state, InviteState::Open));
}

#[test]
fn owner_invite_persistence_restores_missing_secret_on_resync() {
    let dir = tempfile::tempdir().unwrap();
    let persistence = OwnerInvitePersistence::new(dir.path().to_path_buf(), "owner-peer".into());
    let invite_id = uuid::Uuid::new_v4().to_string();
    let pending = PendingInvite {
        invite_id: invite_id.clone(),
        code: InviteCode {
            passphrase: "stable-passphrase".into(),
            peer_id: "owner-peer".into(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
        },
        created_at: std::time::Instant::now(),
        attempts: 0,
        project_name: "shared".into(),
        project_id: "shared-project-id".into(),
        invite_role: "editor".into(),
        state: InviteState::Open,
    };

    persistence
        .sync_invite(&pending.code.passphrase, &pending)
        .unwrap();

    let secret_name = format!("invite-passphrase-{invite_id}");
    let keystore = notes_crypto::KeyStore::new(dir.path().join(".p2p").join("invite-secrets"));
    keystore.delete_key(&secret_name).unwrap();

    let mut reserved = pending.clone();
    reserved.attempts = 1;
    reserved.state = InviteState::Reserved(notes_sync::invite::InviteReservation {
        session_id: "session-1".into(),
        invitee_peer_id: "peer-1".into(),
        reserved_at: std::time::Instant::now(),
        timeout_at: std::time::Instant::now() + Duration::from_secs(30),
        phase: notes_sync::invite::InviteSessionPhase::PayloadSent,
    });
    let persistence = OwnerInvitePersistence::new(dir.path().to_path_buf(), "owner-peer".into());
    persistence
        .sync_invite(&reserved.code.passphrase, &reserved)
        .unwrap();

    let restored = keystore.load_key(&secret_name).unwrap();
    assert_eq!(restored, b"stable-passphrase");
}

#[test]
fn session_secret_cache_preloads_join_session_secrets() {
    notes_crypto::debug_reset_secret_read_tracking();
    notes_crypto::debug_enable_secret_read_tracking(true);
    notes_crypto::debug_set_secret_read_phase(notes_crypto::SecretReadPhase::Startup);
    let dir = tempfile::tempdir().unwrap();
    let store = JoinSessionStore::new(dir.path().to_path_buf());
    store
        .save(&notes_core::PersistedJoinSession {
            schema_version: 1,
            session_id: "session-preload".into(),
            owner_peer_id: "owner-peer".into(),
            project_id: "project-id".into(),
            project_name: "shared".into(),
            local_project_name: "shared".into(),
            role: "editor".into(),
            payload: "{}".into(),
            stage: notes_core::PersistedJoinStage::PayloadStaged {
                staged_at: chrono::Utc::now(),
            },
            updated_at: chrono::Utc::now(),
        })
        .unwrap();
    store
        .save_secret_bundle(
            "session-preload",
            &notes_core::PersistedJoinSecret {
                passphrase: "join-secret".into(),
                epoch_key_hex: Some("abcd".into()),
            },
        )
        .unwrap();

    let cache = SessionSecretCache::default();
    assert_eq!(cache.preload_join_secrets(&store).unwrap(), 1);
    notes_crypto::debug_set_secret_read_phase(notes_crypto::SecretReadPhase::Runtime);

    store.delete("session-preload").unwrap();
    let secret = cache
        .load_join_secret(&store, "session-preload")
        .unwrap()
        .unwrap();
    let stats = notes_crypto::debug_get_secret_read_stats();
    assert_eq!(secret.passphrase, "join-secret");
    assert_eq!(secret.epoch_key_hex.as_deref(), Some("abcd"));
    assert_eq!(stats.runtime_reads, 0);
    assert!(stats.cache_hits > 0);
    assert!(stats
        .events
        .iter()
        .any(|event| event.class == notes_crypto::SecretReadClass::JoinSessionSecret));
    notes_crypto::debug_reset_secret_read_tracking();
}

#[tokio::test]
async fn owner_restore_reconciles_prepared_ack_to_committed_pending_ack() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let mut peer_secret = [0u8; 32];
    getrandom::fill(&mut peer_secret).unwrap();
    let invitee_peer_id = iroh::SecretKey::from_bytes(&peer_secret)
        .public()
        .to_string();
    coordinator
        .apply_acceptance_commit(&InviteAcceptanceContext {
            invite_id: "invite-1".into(),
            session_id: "session-1".into(),
            passphrase: "prepared-passphrase".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: invitee_peer_id.clone(),
        })
        .await
        .unwrap();

    let persistence = OwnerInvitePersistence::new(
        owner._dir.as_ref().unwrap().path().to_path_buf(),
        owner.endpoint.id().to_string(),
    );
    persistence
        .sync_invite(
            "prepared-passphrase",
            &PendingInvite {
                invite_id: "invite-1".into(),
                code: InviteCode {
                    passphrase: "prepared-passphrase".into(),
                    peer_id: owner.endpoint.id().to_string(),
                    expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
                },
                created_at: std::time::Instant::now(),
                attempts: 1,
                project_name: "shared".into(),
                project_id: "shared-project-id".into(),
                invite_role: "editor".into(),
                state: InviteState::Reserved(notes_sync::invite::InviteReservation {
                    session_id: "session-1".into(),
                    invitee_peer_id,
                    reserved_at: std::time::Instant::now(),
                    timeout_at: std::time::Instant::now() + Duration::from_secs(30),
                    phase: notes_sync::invite::InviteSessionPhase::PreparedAckReceived,
                }),
            },
        )
        .unwrap();

    let restored = persistence
        .load_runtime_invites_with_manifest_reconcile()
        .unwrap();
    assert!(matches!(
        restored[0].1.state,
        InviteState::CommittedPendingAck(_)
    ));

    owner.shutdown().await;
}

#[tokio::test]
async fn resume_join_sessions_finalizes_commit_confirmed_install() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let dir = tempfile::tempdir().unwrap();
    let invitee = TestNode::new_at_path(dir.path().to_path_buf(), None, &lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner
        .project_manager
        .create_note("shared", "resume.md")
        .await
        .unwrap();

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let payload = coordinator
        .build_payload(&InviteAcceptanceContext {
            invite_id: "invite-1".into(),
            session_id: "session-1".into(),
            passphrase: "resume-passphrase".into(),
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            role: "editor".into(),
            invitee_peer_id: invitee.endpoint.id().to_string(),
        })
        .await
        .unwrap();

    persist_commit_confirmed_payload(
        &invitee.join_session_store,
        &payload,
        &owner.endpoint.id().to_string(),
        "shared",
    )
    .unwrap();
    invitee
        .join_session_store
        .save_secret_bundle(
            &payload.session_id,
            &notes_core::PersistedJoinSecret {
                passphrase: "resume-passphrase".into(),
                epoch_key_hex: Some(payload.epoch_key_hex.clone()),
            },
        )
        .unwrap();
    let fresh_secret_cache = Arc::new(SessionSecretCache::default());

    resume_join_sessions(
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&fresh_secret_cache),
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.clone(),
        None,
    )
    .await;

    let files = invitee.project_manager.list_files("shared").await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "resume.md");
    assert!(invitee.join_session_store.load_all().unwrap().is_empty());
    assert!(invitee.project_manager.get_epoch_keys("shared").is_ok());
    assert!(!fresh_secret_cache.has_join_passphrase("session-1"));

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn resume_join_sessions_recovers_payload_staged_session() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let dir = tempfile::tempdir().unwrap();
    let invitee = TestNode::new_at_path(dir.path().to_path_buf(), None, &lookup, None).await;

    owner
        .project_manager
        .create_project("shared")
        .await
        .unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner
        .project_manager
        .create_note("shared", "resume-stage.md")
        .await
        .unwrap();

    let passphrase = "resume-stage-passphrase";
    let payload = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    )
    .build_payload(&InviteAcceptanceContext {
        invite_id: "invite-2".into(),
        session_id: "session-2".into(),
        passphrase: passphrase.into(),
        project_name: "shared".into(),
        project_id: "shared-project-id".into(),
        role: "editor".into(),
        invitee_peer_id: invitee.endpoint.id().to_string(),
    })
    .await
    .unwrap();

    OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    )
    .apply_acceptance_commit(&InviteAcceptanceContext {
        invite_id: payload.invite_id.clone(),
        session_id: payload.session_id.clone(),
        passphrase: passphrase.into(),
        project_name: "shared".into(),
        project_id: "shared-project-id".into(),
        role: "editor".into(),
        invitee_peer_id: invitee.endpoint.id().to_string(),
    })
    .await
    .unwrap();

    owner.invite_handler.add_pending(
        passphrase.into(),
        PendingInvite {
            invite_id: payload.invite_id.clone(),
            code: InviteCode {
                passphrase: passphrase.into(),
                peer_id: owner.endpoint.id().to_string(),
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(10),
            },
            created_at: std::time::Instant::now(),
            attempts: 1,
            project_name: "shared".into(),
            project_id: "shared-project-id".into(),
            invite_role: "editor".into(),
            state: InviteState::CommittedPendingAck(
                notes_sync::invite::InviteCommittedPendingAck {
                    session_id: payload.session_id.clone(),
                    invitee_peer_id: invitee.endpoint.id().to_string(),
                    committed_at: std::time::Instant::now(),
                },
            ),
        },
    );

    persist_payload_staged_session(
        &invitee.join_session_store,
        &invitee.session_secret_cache,
        &payload,
        &owner.endpoint.id().to_string(),
        "shared",
        passphrase,
    )
    .unwrap();
    assert!(invitee
        .session_secret_cache
        .has_join_passphrase("session-2"));
    let fresh_secret_cache = Arc::new(SessionSecretCache::default());

    resume_join_sessions(
        Arc::clone(&invitee.join_session_store),
        Arc::clone(&fresh_secret_cache),
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.clone(),
        None,
    )
    .await;

    let files = invitee.project_manager.list_files("shared").await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "resume-stage.md");
    assert!(invitee.join_session_store.load_all().unwrap().is_empty());
    assert!(!fresh_secret_cache.has_join_passphrase("session-2"));
    let owner_persistence = OwnerInvitePersistence::new(
        owner._dir.as_ref().unwrap().path().to_path_buf(),
        owner.endpoint.id().to_string(),
    );
    let restored = owner_persistence
        .load_runtime_invites_with_manifest_reconcile()
        .unwrap();
    assert!(
        restored.into_iter().all(|(code, invite)| {
            code != passphrase || !matches!(invite.state, InviteState::Open)
        }),
        "owner should not restore the resumed invite as open/reusable after applied ack"
    );

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn resume_join_sessions_drops_staged_session_without_secret_bundle() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let invitee = TestNode::new(&lookup, None).await;

    invitee
        .join_session_store
        .save(&notes_core::PersistedJoinSession {
            schema_version: 1,
            session_id: "missing-secret-stage".into(),
            owner_peer_id: invitee.endpoint.id().to_string(),
            project_id: "project-id".into(),
            project_name: "shared".into(),
            local_project_name: "shared".into(),
            role: "editor".into(),
            payload: serde_json::to_string(&InvitePayload {
                invite_id: "invite-id".into(),
                session_id: "missing-secret-stage".into(),
                project_id: "project-id".into(),
                project_name: "shared".into(),
                role: "editor".into(),
                manifest_hex: String::new(),
                owner_x25519_public_hex: String::new(),
                epoch_key_hex: String::new(),
                epoch: 0,
            })
            .unwrap(),
            stage: notes_core::PersistedJoinStage::PayloadStaged {
                staged_at: chrono::Utc::now(),
            },
            updated_at: chrono::Utc::now(),
        })
        .unwrap();

    resume_join_sessions(
        Arc::clone(&invitee.join_session_store),
        Arc::new(SessionSecretCache::default()),
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.clone(),
        None,
    )
    .await;

    assert!(invitee.join_session_store.load_all().unwrap().is_empty());
    invitee.shutdown().await;
}

#[tokio::test]
async fn resume_join_sessions_drops_commit_confirmed_session_without_secret_bundle() {
    let _guard = ACCEPT_INVITE_TEST_LOCK.lock().await;
    let lookup = MemoryLookup::new();
    let invitee = TestNode::new(&lookup, None).await;

    invitee
        .join_session_store
        .save(&notes_core::PersistedJoinSession {
            schema_version: 1,
            session_id: "missing-secret-commit".into(),
            owner_peer_id: invitee.endpoint.id().to_string(),
            project_id: "project-id".into(),
            project_name: "shared".into(),
            local_project_name: "shared".into(),
            role: "editor".into(),
            payload: serde_json::to_string(&InvitePayload {
                invite_id: "invite-id".into(),
                session_id: "missing-secret-commit".into(),
                project_id: "project-id".into(),
                project_name: "shared".into(),
                role: "editor".into(),
                manifest_hex: String::new(),
                owner_x25519_public_hex: String::new(),
                epoch_key_hex: String::new(),
                epoch: 0,
            })
            .unwrap(),
            stage: notes_core::PersistedJoinStage::CommitConfirmed {
                confirmed_at: chrono::Utc::now(),
            },
            updated_at: chrono::Utc::now(),
        })
        .unwrap();

    resume_join_sessions(
        Arc::clone(&invitee.join_session_store),
        Arc::new(SessionSecretCache::default()),
        Arc::clone(&invitee.project_manager),
        Arc::clone(&invitee.sync_engine),
        Arc::clone(&invitee.peer_manager),
        invitee.endpoint.clone(),
        None,
    )
    .await;

    assert!(invitee.join_session_store.load_all().unwrap().is_empty());
    invitee.shutdown().await;
}
