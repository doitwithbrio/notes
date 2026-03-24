<script lang="ts">
  import { isMac } from '../utils/platform.js';
  import { toggleSidebar, uiState } from '../state/ui.svelte.js';
</script>

<header class="titlebar" data-tauri-drag-region>
  {#if isMac}
    <div class="traffic-lights-spacer"></div>
  {/if}

  <button
    class="sidebar-toggle"
    onclick={toggleSidebar}
    aria-label={uiState.sidebarOpen ? 'Hide sidebar' : 'Show sidebar'}
  >
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
      <rect x="1" y="3" width="14" height="1.5" fill="currentColor" />
      <rect x="1" y="7.25" width="14" height="1.5" fill="currentColor" />
      <rect x="1" y="11.5" width="14" height="1.5" fill="currentColor" />
    </svg>
  </button>

  <span class="app-title" data-tauri-drag-region>Notes</span>

  <div class="spacer" data-tauri-drag-region></div>

  {#if !isMac}
    <div class="window-controls">
      <button class="window-btn minimize" aria-label="Minimize">
        <svg width="10" height="1" viewBox="0 0 10 1"><rect width="10" height="1" fill="currentColor" /></svg>
      </button>
      <button class="window-btn maximize" aria-label="Maximize">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" /></svg>
      </button>
      <button class="window-btn close" aria-label="Close">
        <svg width="10" height="10" viewBox="0 0 10 10"><line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" stroke-width="1.2" /><line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" stroke-width="1.2" /></svg>
      </button>
    </div>
  {/if}
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    height: var(--titlebar-height);
    border-bottom: var(--border);
    background: var(--white);
    padding: 0 12px;
    gap: 8px;
    user-select: none;
    -webkit-user-select: none;
  }

  .traffic-lights-spacer {
    width: 68px;
    flex-shrink: 0;
  }

  .sidebar-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    color: var(--black);
  }

  .sidebar-toggle:hover {
    color: var(--accent);
  }

  .app-title {
    font-family: var(--font-display);
    font-size: 18px;
    letter-spacing: 0.02em;
    color: var(--black);
    flex-shrink: 0;
  }

  .spacer {
    flex: 1;
  }

  .window-controls {
    display: flex;
    gap: 0;
    flex-shrink: 0;
  }

  .window-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: var(--titlebar-height);
    color: var(--black);
  }

  .window-btn:hover {
    background: var(--accent);
    color: var(--white);
  }

  .window-btn.close:hover {
    background: var(--black);
    color: var(--white);
  }
</style>
