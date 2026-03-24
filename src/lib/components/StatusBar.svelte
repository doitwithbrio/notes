<script lang="ts">
  import { syncState } from '../state/sync.svelte.js';
  import { getActiveDoc } from '../state/documents.svelte.js';

  const activeDoc = $derived(getActiveDoc());

  const connectionLabel = $derived(
    syncState.connection === 'connected'
      ? `Connected to ${syncState.peerCount} peer${syncState.peerCount !== 1 ? 's' : ''}`
      : syncState.connection === 'slow'
        ? 'Sync slow'
        : 'Offline',
  );
</script>

<footer class="statusbar">
  <div class="status-left">
    <span
      class="connection-dot"
      class:connected={syncState.connection === 'connected'}
      class:slow={syncState.connection === 'slow'}
      class:offline={syncState.connection === 'offline'}
    ></span>
    <span class="connection-label">{connectionLabel}</span>
  </div>

  <div class="status-center">
    {#if activeDoc}
      <span>{activeDoc.title}</span>
      <span class="separator">&middot;</span>
      <span>{activeDoc.wordCount.toLocaleString()} words</span>
    {/if}
  </div>

  <div class="status-right">
    {#if syncState.unsentChanges > 0}
      <span class="unsent">{syncState.unsentChanges} unsent</span>
    {/if}
  </div>
</footer>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--statusbar-height);
    padding: 0 12px;
    font-size: 11px;
    background: var(--white);
    color: var(--black);
  }

  .status-left,
  .status-center,
  .status-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .status-left { flex: 1; }
  .status-center { flex: 1; justify-content: center; }
  .status-right { flex: 1; justify-content: flex-end; }

  .connection-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .connection-dot.connected {
    background: var(--accent);
  }

  .connection-dot.slow {
    background: var(--white);
    border: 2px solid var(--accent);
  }

  .connection-dot.offline {
    background: var(--black);
    /* slash through via pseudo */
    position: relative;
  }

  .connection-dot.offline::after {
    content: '';
    position: absolute;
    top: 50%;
    left: -1px;
    width: 10px;
    height: 1px;
    background: var(--white);
    transform: rotate(-45deg);
  }

  .separator {
    color: var(--black);
  }

  .unsent {
    color: var(--accent);
    font-weight: 500;
  }
</style>
