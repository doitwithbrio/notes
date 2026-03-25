<script lang="ts">
  import { onMount } from 'svelte';

  import { initializeApp, appSessionState, teardownAppSession } from '../state/app-session.svelte.js';
  import { closeEditorSession } from '../session/editor-session.svelte.js';
  import { uiState } from '../state/ui.svelte.js';
  import { projectState } from '../state/projects.svelte.js';
  import { getActiveDoc } from '../state/documents.svelte.js';
  import { isMac } from '../utils/platform.js';
  import Sidebar from './sidebar/Sidebar.svelte';
  import EditorPane from './editor/EditorPane.svelte';
  import ProjectOverview from './editor/ProjectOverview.svelte';
  import QuickOpen from './sidebar/QuickOpen.svelte';
  import RightSidebar from './rightsidebar/RightSidebar.svelte';

  const activeDoc = $derived(getActiveDoc());
  const activeProject = $derived(
    projectState.projects.find((project) => project.id === uiState.activeProjectId) ?? null,
  );

  onMount(() => {
    void initializeApp();

    const handleBeforeUnload = () => {
      void closeEditorSession();
    };

    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload);
      teardownAppSession();
      void closeEditorSession();
    };
  });

  function handleKeydown(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (mod && e.key === 'f') {
      e.preventDefault();
      uiState.quickOpenVisible = !uiState.quickOpenVisible;
    }
    if (e.key === 'Escape') {
      uiState.quickOpenVisible = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="shell">
  <div
    class="sidebar-panel"
    style="width: {uiState.sidebarOpen ? 'var(--sidebar-width)' : 'var(--sidebar-collapsed)'};"
  >
    <Sidebar />
  </div>

  <div class="editor-panel">
    {#if appSessionState.booting}
      <div class="app-message">
        <p class="title">loading notes...</p>
      </div>
    {:else if appSessionState.error}
      <div class="app-message">
        <p class="title">could not load notes</p>
        <p class="body">{appSessionState.error}</p>
      </div>
    {:else}
      <div class="main-view" class:hidden={uiState.view === 'project-overview'}>
        <EditorPane />
      </div>
      {#if uiState.view === 'project-overview' && activeProject}
        <ProjectOverview project={activeProject} />
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
  <QuickOpen />
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

  .app-message .title {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .app-message .body {
    max-width: 420px;
    text-align: center;
  }
</style>
