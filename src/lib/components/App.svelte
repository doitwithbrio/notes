<script lang="ts">
  import { onMount } from 'svelte';

  import { initializeApp, appSessionState, teardownAppSession } from '../state/app-session.svelte.js';
  import { closeEditorSession, editorSessionState } from '../session/editor-session.svelte.js';
  import { teardownAppearance } from '../state/appearance.svelte.js';
  import { tauriApi } from '../api/tauri.js';
  import { clearRevokedProjectNotice, closeQuickOpen, toggleQuickOpen, uiState } from '../state/ui.svelte.js';
  import { projectState } from '../state/projects.svelte.js';
  import { isMac } from '../utils/platform.js';
  import { shouldIgnoreGlobalShortcut } from '../utils/keyboard.js';
  import { getSelectedDoc, getSelectedProjectId, getWorkspaceRoute, isProjectRoute, navigateToDoc, navigateToProject, reconcileMissingSelectedDoc } from '../navigation/workspace-router.svelte.js';
  import Sidebar from './sidebar/Sidebar.svelte';
  import ProjectOverview from './editor/ProjectOverview.svelte';
  import RightSidebar from './rightsidebar/RightSidebar.svelte';
  import UpdateBanner from './UpdateBanner.svelte';
  import { clearInviteBanner, inviteState, resumePendingJoins } from '../state/invite.svelte.js';
  import { installE2EBridge, teardownE2EBridge } from '../e2e/bridge.js';

  const activeDoc = $derived(getSelectedDoc());
  const route = $derived(getWorkspaceRoute());
  const activeProject = $derived(
    isProjectRoute(route)
      ? (projectState.projects.find((project) => project.id === route.projectId) ?? null)
      : null,
  );
  const selectedProject = $derived(
    projectState.projects.find((project) => project.id === getSelectedProjectId()) ?? null,
  );
  const selectedDocOpenFailed = $derived.by(() => {
    if (!route || route.kind !== 'doc') return false;
    if (!activeDoc) return false;
    if (editorSessionState.loading || !editorSessionState.lastError) return false;
    return editorSessionState.projectId !== activeDoc.projectId || editorSessionState.docId !== activeDoc.id;
  });
  const selectedDocRecoverable = $derived(
    selectedDocOpenFailed && editorSessionState.lastErrorCode === 'DOC_CORRUPTED_RECOVERABLE',
  );
  const selectedDocRecoveryDetails = $derived(
    selectedDocRecoverable
      ? (editorSessionState.lastErrorDetails as { notePath?: string; suggestedPath?: string } | null)
      : null,
  );
  const selectedDocCanRecover = $derived(selectedProject?.canEdit ?? true);
  const selectedDocIdentityMismatch = $derived(selectedProject?.accessState === 'identity-mismatch');

  let editorPanePromise = $state<Promise<typeof import('./editor/EditorPane.svelte')> | null>(null);
  let settingsPanePromise = $state<Promise<typeof import('./settings/SettingsPane.svelte')> | null>(null);
  let quickOpenPromise = $state<Promise<typeof import('./sidebar/QuickOpen.svelte')> | null>(null);
  let shareDialogPromise = $state<Promise<typeof import('./dialogs/ShareDialog.svelte')> | null>(null);
  let joinDialogPromise = $state<Promise<typeof import('./dialogs/JoinDialog.svelte')> | null>(null);
  const currentJoinResume = $derived(inviteState.pendingJoinResumes[0] ?? null);
  const pendingJoinCount = $derived(inviteState.pendingJoinResumes.length);
  const inviteBanner = $derived.by(() => {
    if (inviteState.latestInviteEvent?.stage === 'completed') {
      return {
        kind: 'success' as const,
        title: `joined ${inviteState.latestInviteEvent.localProjectName ?? inviteState.latestInviteEvent.projectName}`,
        body: `you are now a ${inviteState.latestInviteEvent.role} on this project`,
        cta: 'open project',
        project: inviteState.latestInviteEvent.localProjectName ?? inviteState.latestInviteEvent.projectName,
      };
    }
    if (inviteState.latestInviteEvent?.stage === 'failed') {
      return {
        kind: 'error' as const,
        title: `couldn't finish joining ${inviteState.latestInviteEvent.projectName}`,
        body: inviteState.latestInviteEvent.error ?? 'you can retry without re-entering the code',
        cta: 'retry',
        project: null,
      };
    }
    if (currentJoinResume) {
      return {
        kind: 'info' as const,
        title:
          pendingJoinCount > 1
            ? `finishing ${pendingJoinCount} project joins`
            : `finishing join for ${currentJoinResume.localProjectName}`,
        body:
          pendingJoinCount > 1
            ? 'the app is resuming all pending joins automatically in the background'
            : 'you can keep using the app while this resumes automatically',
        cta: 'retry',
        project: null,
      };
    }
    return null;
  });
  const revokedNotice = $derived(uiState.revokedProjectNotices[0] ?? null);

  $effect(() => {
    if ((activeDoc || editorSessionState.loading) && !editorPanePromise) {
      editorPanePromise = import('./editor/EditorPane.svelte');
    }
  });

  $effect(() => {
    if (route?.kind === 'settings' && !settingsPanePromise) {
      settingsPanePromise = import('./settings/SettingsPane.svelte');
    }
  });

  $effect(() => {
    if (route?.kind === 'doc' && !activeDoc && !editorSessionState.loading) {
      reconcileMissingSelectedDoc();
    }
  });

  $effect(() => {
    if (uiState.quickOpenVisible && !quickOpenPromise) {
      quickOpenPromise = import('./sidebar/QuickOpen.svelte');
    }
  });

  $effect(() => {
    if (inviteState.shareDialogOpen && !shareDialogPromise) {
      shareDialogPromise = import('./dialogs/ShareDialog.svelte');
    }
    if (inviteState.joinDialogOpen && !joinDialogPromise) {
      joinDialogPromise = import('./dialogs/JoinDialog.svelte');
    }
  });

  onMount(() => {
    void initializeApp();
    void installE2EBridge();

    const handleBeforeUnload = () => {
      void closeEditorSession();
    };

    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload);
      teardownE2EBridge();
      teardownAppearance();
      teardownAppSession();
      void closeEditorSession();
    };
  });

  function handleKeydown(e: KeyboardEvent) {
    if (shouldIgnoreGlobalShortcut(e)) return;
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (mod && e.key === 'f') {
      e.preventDefault();
      toggleQuickOpen();
    }
    if (e.key === 'Escape') {
      closeQuickOpen();
    }
  }

  function handleInviteBannerAction() {
    if (!inviteBanner) return;
    if (inviteBanner.cta === 'open project' && inviteBanner.project) {
      void navigateToProject(inviteBanner.project);
      clearInviteBanner();
      return;
    }
    if (inviteBanner.cta === 'retry') {
      clearInviteBanner();
      void resumePendingJoins();
    }
  }

  function handleRetrySelectedDoc() {
    if (!route || route.kind !== 'doc') return;
    void navigateToDoc(route.projectId, route.docId).catch((error) => {
      console.error('Failed to retry opening note:', error);
    });
  }

  function handleRecoverSelectedDoc() {
    if (!route || route.kind !== 'doc') return;
    void tauriApi.recoverDocFromMarkdown(route.projectId, route.docId)
      .then(async () => {
        await navigateToDoc(route.projectId, route.docId);
      })
      .catch((error) => {
        console.error('Failed to recover note from markdown:', error);
      });
  }

  function handleDismissRevokedNotice() {
    const notice = revokedNotice;
    if (!notice) return;
    clearRevokedProjectNotice(notice.projectId);
    void tauriApi.dismissProjectEvictionNotice(notice.backendProjectId).catch((error) => {
      console.error('Failed to dismiss revoked project notice:', error);
    });
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="shell" data-testid="app-shell">
  <div
    class="sidebar-panel"
    style="width: {uiState.sidebarOpen ? 'var(--sidebar-width)' : 'var(--sidebar-collapsed)'};"
  >
    <Sidebar />
  </div>

  <div class="editor-panel">
    <UpdateBanner />
    {#if revokedNotice}
      <div class="invite-banner error" data-testid="revoked-project-banner">
        <div class="invite-copy">
          <p class="invite-title">access revoked</p>
          <p class="invite-body">{revokedNotice.projectName} was removed from this device. Any unsynced local changes for that project were removed and will not be sent.</p>
        </div>
        <button class="invite-action" onclick={handleDismissRevokedNotice}>dismiss</button>
      </div>
    {/if}
    {#if inviteBanner}
      <div class={`invite-banner ${inviteBanner.kind}`} data-testid="invite-banner">
        <div class="invite-copy">
          <p class="invite-title">{inviteBanner.title}</p>
          <p class="invite-body">{inviteBanner.body}</p>
        </div>
        <button class="invite-action" onclick={handleInviteBannerAction}>{inviteBanner.cta}</button>
      </div>
    {/if}
    {#if appSessionState.error}
      <div class="app-message">
        <p class="title">could not load notes</p>
        <p class="body">{appSessionState.error}</p>
      </div>
    {:else}
        {#if route?.kind === 'settings'}
          {#if settingsPanePromise}
          {#await settingsPanePromise then settingsPaneModule}
            {@const SettingsPane = settingsPaneModule.default}
            <SettingsPane />
          {:catch error}
            <div class="app-message inline-message">
              <p class="title">could not load settings</p>
              <p class="body">{error instanceof Error ? error.message : 'Settings failed to load'}</p>
            </div>
          {/await}
        {/if}
      {:else}
        <div class="main-view" class:hidden={isProjectRoute(route)}>
          {#if !appSessionState.ready}
            <div class="app-message inline-message" data-testid="workspace-loading">
              <p class="title">loading workspace...</p>
            </div>
          {:else if editorSessionState.loading}
            <div class="app-message inline-message" data-testid="editor-loading">
              <p class="title">loading editor...</p>
            </div>
          {:else if selectedDocOpenFailed && activeDoc}
            <div class="app-message inline-message" data-testid="editor-open-failed">
              <p class="title">could not open {activeDoc.title}</p>
              <p class="body">{editorSessionState.lastError}</p>
              {#if selectedDocRecoverable && selectedDocRecoveryDetails}
                {#if selectedDocCanRecover}
                  <p class="body">a markdown export was found at <code>{selectedDocRecoveryDetails.notePath}</code>. recovering will rebuild this note from markdown and keep the broken automerge files quarantined.</p>
                  <div class="invite-actions">
                    <button class="invite-action secondary" data-testid="editor-open-retry" onclick={handleRetrySelectedDoc}>retry</button>
                    <button class="invite-action" data-testid="editor-open-recover" onclick={handleRecoverSelectedDoc}>recover note from markdown</button>
                  </div>
                {:else if selectedDocIdentityMismatch}
                  <p class="body">a markdown export exists at <code>{selectedDocRecoveryDetails.notePath}</code>, but this app instance is using a different device identity than the owner/editor for this project. Open the build that shows you as owner, or switch this build to the same identity, then retry recovery.</p>
                  <button class="invite-action secondary" data-testid="editor-open-retry" onclick={handleRetrySelectedDoc}>retry</button>
                {:else}
                  <p class="body">a markdown export exists at <code>{selectedDocRecoveryDetails.notePath}</code>, but only owners and editors can rebuild notes in this project.</p>
                  <button class="invite-action secondary" data-testid="editor-open-retry" onclick={handleRetrySelectedDoc}>retry</button>
                {/if}
              {:else}
              <button class="invite-action" data-testid="editor-open-retry" onclick={handleRetrySelectedDoc}>retry</button>
              {/if}
            </div>
          {:else if activeDoc}
            {#if editorPanePromise}
              {#await editorPanePromise}
                <div class="app-message inline-message">
                  <p class="title">loading editor...</p>
                </div>
              {:then editorPaneModule}
                {@const EditorPane = editorPaneModule.default}
                <EditorPane />
              {:catch error}
                <div class="app-message inline-message">
                  <p class="title">could not load editor</p>
                  <p class="body">{error instanceof Error ? error.message : 'Editor failed to load'}</p>
                </div>
              {/await}
            {/if}
          {:else}
            <div class="empty-state" data-testid="empty-editor-state">
              <p class="title">no document selected</p>
              <p class="body">pick a note from the sidebar, or create a new one</p>
            </div>
          {/if}
        </div>
        {#if isProjectRoute(route) && activeProject}
          <ProjectOverview project={activeProject} />
        {/if}
      {/if}
    {/if}
  </div>

  <div
    class="right-sidebar-panel"
    style="width: {uiState.rightSidebarOpen ? 'var(--right-sidebar-width)' : 'var(--right-sidebar-collapsed)'};"
  >
    <RightSidebar />
  </div>
</div>

{#if uiState.quickOpenVisible}
  {#if quickOpenPromise}
    {#await quickOpenPromise then quickOpenModule}
      {@const QuickOpen = quickOpenModule.default}
      <QuickOpen />
    {/await}
  {/if}
{/if}

{#if inviteState.shareDialogOpen}
  {#if shareDialogPromise}
    {#await shareDialogPromise then shareDialogModule}
      {@const ShareDialog = shareDialogModule.default}
      <ShareDialog />
    {/await}
  {/if}
{/if}

{#if inviteState.joinDialogOpen}
  {#if joinDialogPromise}
    {#await joinDialogPromise then joinDialogModule}
      {@const JoinDialog = joinDialogModule.default}
      <JoinDialog />
    {/await}
  {/if}
{/if}

<style>
  .shell {
    display: flex;
    height: 100vh;
    width: 100vw;
    overflow: hidden;
  }

  .sidebar-panel {
    flex-shrink: 0;
    background: var(--surface-sidebar);
    border-right: 1px solid var(--border-subtle);
    transition: width var(--transition-slow);
    overflow: hidden;
  }

  .editor-panel {
    flex: 1;
    min-width: 0;
    background: var(--surface);
    position: relative;
  }

  .invite-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--surface-sidebar);
  }

  .invite-banner.info {
    background: var(--surface-sidebar);
  }

  .invite-banner.success {
    background: color-mix(in srgb, var(--surface-sidebar) 78%, #dff4e8 22%);
  }

  .invite-banner.error {
    background: color-mix(in srgb, var(--surface-sidebar) 78%, #f7ded8 22%);
  }

  .invite-copy {
    min-width: 0;
  }

  .invite-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .invite-body {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 2px;
  }

  .invite-action {
    flex-shrink: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    background: var(--surface);
    border: 1px solid var(--border-default);
    border-radius: 999px;
    padding: 6px 10px;
  }

  .main-view {
    height: 100%;
  }

  .main-view.hidden {
    display: none;
  }

  .right-sidebar-panel {
    flex-shrink: 0;
    border-left: 1px solid var(--border-subtle);
    background: var(--surface-sidebar);
    transition: width var(--transition-slow);
    overflow: hidden;
  }

  .app-message {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: var(--text-secondary);
  }

  .inline-message,
  .empty-state {
    padding: 24px;
  }

  .app-message .title,
  .empty-state .title {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .app-message .body,
  .empty-state .body {
    max-width: 420px;
    text-align: center;
  }
</style>
