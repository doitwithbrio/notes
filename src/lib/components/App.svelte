<script lang="ts">
  import { onMount } from 'svelte';

  import { initializeApp, appSessionState, teardownAppSession } from '../state/app-session.svelte.js';
  import { closeEditorSession, editorSessionState } from '../session/editor-session.svelte.js';
  import { teardownAppearance } from '../state/appearance.svelte.js';
  import { closeQuickOpen, toggleQuickOpen, uiState } from '../state/ui.svelte.js';
  import { projectState } from '../state/projects.svelte.js';
  import { isMac } from '../utils/platform.js';
  import { getSelectedDoc, getWorkspaceRoute, isProjectRoute, reconcileMissingSelectedDoc } from '../navigation/workspace-router.svelte.js';
  import Sidebar from './sidebar/Sidebar.svelte';
  import ProjectOverview from './editor/ProjectOverview.svelte';
  import RightSidebar from './rightsidebar/RightSidebar.svelte';
  import UpdateBanner from './UpdateBanner.svelte';
  import { inviteState } from '../state/invite.svelte.js';
  import { installE2EBridge, teardownE2EBridge } from '../e2e/bridge.js';

  const activeDoc = $derived(getSelectedDoc());
  const route = $derived(getWorkspaceRoute());
  const activeProject = $derived(
    isProjectRoute(route)
      ? (projectState.projects.find((project) => project.id === route.projectId) ?? null)
      : null,
  );

  let editorPanePromise = $state<Promise<typeof import('./editor/EditorPane.svelte')> | null>(null);
  let settingsPanePromise = $state<Promise<typeof import('./settings/SettingsPane.svelte')> | null>(null);
  let quickOpenPromise = $state<Promise<typeof import('./sidebar/QuickOpen.svelte')> | null>(null);
  let shareDialogPromise = $state<Promise<typeof import('./dialogs/ShareDialog.svelte')> | null>(null);
  let joinDialogPromise = $state<Promise<typeof import('./dialogs/JoinDialog.svelte')> | null>(null);

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
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (mod && e.key === 'f') {
      e.preventDefault();
      toggleQuickOpen();
    }
    if (e.key === 'Escape') {
      closeQuickOpen();
    }
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
