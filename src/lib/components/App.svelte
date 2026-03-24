<script lang="ts">
  import { uiState } from '../state/ui.svelte.js';
  import { projectState } from '../state/projects.svelte.js';
  import { documentState, setActiveDoc } from '../state/documents.svelte.js';
  import { presenceState } from '../state/presence.svelte.js';
  import { syncState } from '../state/sync.svelte.js';
  import { isMac } from '../utils/platform.js';
  import TitleBar from './TitleBar.svelte';
  import Sidebar from './sidebar/Sidebar.svelte';
  import EditorPane from './editor/EditorPane.svelte';
  import StatusBar from './StatusBar.svelte';
  import QuickOpen from './sidebar/QuickOpen.svelte';
  import type { CURSOR_COLORS } from '../types/index.js';

  // ── Mock data for development ──

  function seedMockData() {
    projectState.projects = [
      { id: 'p1', name: 'Personal', path: '~/Notes/personal', shared: false, role: 'owner' },
      { id: 'p2', name: 'Startup Team', path: '~/Notes/startup-team', shared: true, role: 'owner' },
    ];

    documentState.docs = [
      { id: 'd1', projectId: 'p1', path: 'journal.md', title: 'Journal', syncStatus: 'synced', wordCount: 342, activePeers: [] },
      { id: 'd2', projectId: 'p1', path: 'ideas.md', title: 'Ideas', syncStatus: 'synced', wordCount: 89, activePeers: [] },
      { id: 'd3', projectId: 'p2', path: 'roadmap.md', title: 'Roadmap', syncStatus: 'syncing', wordCount: 1205, activePeers: ['peer-1', 'peer-2'] },
      { id: 'd4', projectId: 'p2', path: 'meeting-notes/2026-03-24.md', title: '2026-03-24', syncStatus: 'synced', wordCount: 567, activePeers: ['peer-1'] },
      { id: 'd5', projectId: 'p2', path: 'design-spec.md', title: 'Design Spec', syncStatus: 'local-only', wordCount: 0, activePeers: [] },
    ];

    presenceState.peers = [
      { id: 'peer-1', alias: 'Alice', online: true, cursorColor: '#FF0000' },
      { id: 'peer-2', alias: 'Bob', online: true, cursorColor: '#0000FF' },
      { id: 'peer-3', alias: 'Carol', online: false, cursorColor: '#FF00FF' },
    ];

    syncState.connection = 'connected';
    syncState.peerCount = 2;
    syncState.unsentChanges = 0;

    setActiveDoc('d3');
  }

  seedMockData();

  // ── Keyboard shortcuts ──

  function handleKeydown(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (mod && e.key === 'p') {
      e.preventDefault();
      uiState.quickOpenVisible = !uiState.quickOpenVisible;
    }
    if (e.key === 'Escape') {
      uiState.quickOpenVisible = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="app" class:sidebar-collapsed={!uiState.sidebarOpen}>
  <div class="titlebar-area">
    <TitleBar />
  </div>

  {#if uiState.sidebarOpen}
    <div class="sidebar-area">
      <Sidebar />
    </div>
  {/if}

  <div class="editor-area">
    <EditorPane />
  </div>

  <div class="statusbar-area">
    <StatusBar />
  </div>
</div>

{#if uiState.quickOpenVisible}
  <QuickOpen />
{/if}

<style>
  .app {
    display: grid;
    grid-template-rows: var(--titlebar-height) 1fr var(--statusbar-height);
    grid-template-columns: var(--sidebar-width) 1fr;
    grid-template-areas:
      "titlebar titlebar"
      "sidebar editor"
      "statusbar statusbar";
    height: 100vh;
    width: 100vw;
    overflow: hidden;
  }

  .app.sidebar-collapsed {
    grid-template-columns: 0 1fr;
  }

  .titlebar-area {
    grid-area: titlebar;
  }

  .sidebar-area {
    grid-area: sidebar;
    border-right: var(--border);
    overflow: hidden;
  }

  .editor-area {
    grid-area: editor;
    overflow: hidden;
  }

  .statusbar-area {
    grid-area: statusbar;
    border-top: var(--border);
  }
</style>
