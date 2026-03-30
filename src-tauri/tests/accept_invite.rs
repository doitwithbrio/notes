use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::{presets, Endpoint};
use iroh::protocol::Router;
use notes_core::{JoinSessionStore, ProjectManager};
use notes_lib::invite_accept::{
    accept_invite_impl, finalize_accepted_invite, persist_commit_confirmed_payload,
    persist_payload_staged_session, resume_join_sessions, stage_accepted_invite,
    OwnerInviteCoordinator, OwnerInvitePersistence,
};
use notes_sync::invite::{
    InviteAcceptanceContext, InviteCode, InviteHandler, InviteLifecycleHandler,
    InvitePayload, InvitePersistenceHandler, InviteState, PendingInvite,
};
use notes_sync::peer_manager::PeerManager;
use notes_sync::sync_engine::{SyncEngine, NOTES_SYNC_ALPN};
use notes_sync::SyncStateStore;
use tempfile::TempDir;

struct TestNode {
    _dir: Option<TempDir>,
    project_manager: Arc<ProjectManager>,
    sync_engine: Arc<SyncEngine>,
    peer_manager: Arc<PeerManager>,
    endpoint: Endpoint,
    router: Router,
    invite_handler: Arc<InviteHandler>,
    join_session_store: Arc<JoinSessionStore>,
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

        let mut sync_engine_raw = SyncEngine::new();
        sync_engine_raw.set_sync_state_store(Arc::new(SyncStateStore::new(p2p_dir)));
        let sync_engine = Arc::new(sync_engine_raw);
        let peer_manager = Arc::new(PeerManager::new(endpoint.clone(), Arc::clone(&sync_engine)));

        let coordinator = Arc::new(OwnerInviteCoordinator::new(
            Arc::clone(&project_manager),
            Arc::clone(&sync_engine),
            Arc::clone(&peer_manager),
            endpoint.id(),
        ));
        let join_session_store = Arc::new(JoinSessionStore::new(base_dir.clone()));
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
        }
    }

    async fn shutdown(self) {
        self.router.shutdown().await.ok();
        self.endpoint.close().await;
    }
}

struct FailingLifecycle;

impl InviteLifecycleHandler for FailingLifecycle {
    fn prepare_payload<'a>(
        &'a self,
        _ctx: &'a InviteAcceptanceContext,
    ) -> Pin<Box<dyn Future<Output = Result<InvitePayload, notes_sync::invite::InviteError>> + Send + 'a>> {
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
    ) -> Pin<Box<dyn Future<Output = Result<(), notes_sync::invite::InviteError>> + Send + 'a>> {
        Box::pin(async move { Err(notes_sync::invite::InviteError::Lifecycle("forced commit failure".into())) })
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
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner.project_manager.create_project("shared").await.unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner.project_manager.create_note("shared", "hello.md").await.unwrap();

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
        invitee.endpoint.clone(),
        None,
        "alpha-beta-gamma-delta-epsilon-zeta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap();

    assert_eq!(result.project_name, "shared");
    assert_eq!(result.role, "editor");

    let files = invitee.project_manager.list_files("shared").await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "hello.md");

    let peers = owner.project_manager.get_project_peers("shared").await.unwrap();
    assert!(peers.iter().any(|peer| peer.peer_id == invitee.endpoint.id().to_string()));

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn accept_invite_commit_failure_does_not_install_project() {
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
        invitee.endpoint.clone(),
        None,
        "commit-fails-alpha-beta".into(),
        owner.endpoint.id().to_string(),
    )
    .await
    .unwrap_err();

    let err_text = format!("{err}");
    assert!(!err_text.is_empty());
    assert!(invitee.project_manager.open_project("shared").await.is_err());

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn install_accepted_invite_uses_distinct_local_name_when_project_exists() {
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let invitee = TestNode::new(&lookup, None).await;

    owner.project_manager.create_project("shared").await.unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner.project_manager.create_note("shared", "remote.md").await.unwrap();

    invitee.project_manager.create_project("shared").await.unwrap();
    invitee.project_manager.open_project("shared").await.unwrap();
    invitee.project_manager.create_note("shared", "local-only.md").await.unwrap();

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
    let files = invitee.project_manager.list_files("shared-1").await.unwrap();
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

#[tokio::test]
async fn owner_restore_reconciles_prepared_ack_to_committed_pending_ack() {
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    owner.project_manager.create_project("shared").await.unwrap();
    owner.project_manager.open_project("shared").await.unwrap();

    let coordinator = OwnerInviteCoordinator::new(
        Arc::clone(&owner.project_manager),
        Arc::clone(&owner.sync_engine),
        Arc::clone(&owner.peer_manager),
        owner.endpoint.id(),
    );
    let mut peer_secret = [0u8; 32];
    getrandom::fill(&mut peer_secret).unwrap();
    let invitee_peer_id = iroh::SecretKey::from_bytes(&peer_secret).public().to_string();
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

    let persistence = OwnerInvitePersistence::new(owner._dir.as_ref().unwrap().path().to_path_buf(), owner.endpoint.id().to_string());
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
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let dir = tempfile::tempdir().unwrap();
    let invitee = TestNode::new_at_path(dir.path().to_path_buf(), None, &lookup, None).await;

    owner.project_manager.create_project("shared").await.unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner.project_manager.create_note("shared", "resume.md").await.unwrap();

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

    resume_join_sessions(
        Arc::clone(&invitee.join_session_store),
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

    owner.shutdown().await;
    invitee.shutdown().await;
}

#[tokio::test]
async fn resume_join_sessions_recovers_payload_staged_session() {
    let lookup = MemoryLookup::new();
    let owner = TestNode::new(&lookup, None).await;
    let dir = tempfile::tempdir().unwrap();
    let invitee = TestNode::new_at_path(dir.path().to_path_buf(), None, &lookup, None).await;

    owner.project_manager.create_project("shared").await.unwrap();
    owner.project_manager.open_project("shared").await.unwrap();
    owner.project_manager.create_note("shared", "resume-stage.md").await.unwrap();

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
            state: InviteState::CommittedPendingAck(notes_sync::invite::InviteCommittedPendingAck {
                session_id: payload.session_id.clone(),
                invitee_peer_id: invitee.endpoint.id().to_string(),
                committed_at: std::time::Instant::now(),
            }),
        },
    );

    persist_payload_staged_session(
        &invitee.join_session_store,
        &payload,
        &owner.endpoint.id().to_string(),
        "shared",
        passphrase,
    )
    .unwrap();

    resume_join_sessions(
        Arc::clone(&invitee.join_session_store),
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
