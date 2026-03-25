<script lang="ts">
  import { presenceState } from '../../state/presence.svelte.js';
  import { syncState } from '../../state/sync.svelte.js';

  const onlinePeers = $derived(presenceState.peers.filter((p) => p.online));
  const offlinePeers = $derived(presenceState.peers.filter((p) => !p.online));

  const connectionLabel = $derived(
    syncState.connection === 'connected'
      ? 'connected'
      : syncState.connection === 'slow'
        ? 'slow connection'
        : 'offline',
  );
</script>

<section class="section">
  <div class="section-header">
    <span class="section-title">peers</span>
    {#if presenceState.peers.length > 0}
      <span class="section-badge">{presenceState.peers.length}</span>
    {/if}
  </div>

  <div class="peer-list">
    {#each onlinePeers as peer (peer.id)}
      <div class="peer-row" title="{peer.alias} · online">
        <span class="peer-dot" style="background: {peer.cursorColor}"></span>
        <span class="peer-name">{peer.alias}</span>
      </div>
    {/each}

    {#each offlinePeers as peer (peer.id)}
      <div class="peer-row offline" title="{peer.alias} · offline">
        <span class="peer-dot offline-dot"></span>
        <span class="peer-name">{peer.alias}</span>
      </div>
    {/each}

    {#if presenceState.peers.length === 0}
      <p class="empty-text">no peers connected</p>
    {/if}
  </div>

  <div class="sync-status">
    <div
      class="sync-indicator"
      class:connected={syncState.connection === 'connected'}
      class:slow={syncState.connection === 'slow'}
      class:offline={syncState.connection === 'offline'}
    ></div>
    <span class="sync-label">{connectionLabel}</span>
    {#if syncState.unsentChanges > 0}
      <span class="unsent">{syncState.unsentChanges} unsent</span>
    {/if}
  </div>
</section>

<style>
  .section {
    padding: 16px;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 12px;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: var(--text-primary);
  }

  .section-badge {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-primary);
    background: var(--surface-active);
    padding: 0 5px;
    border-radius: 8px;
    line-height: 16px;
  }

  .peer-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 12px;
  }

  .peer-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    border-radius: 6px;
    transition: background var(--transition-fast);
    cursor: default;
  }

  .peer-row:hover {
    background: var(--surface-hover);
  }

  .peer-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .offline-dot {
    background: var(--text-tertiary);
  }

  .peer-name {
    font-size: 13px;
    font-weight: 450;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .peer-row.offline .peer-name {
    color: var(--text-primary);
    opacity: 0.5;
  }

  .empty-text {
    font-size: 12px;
    color: var(--text-primary);
    padding: 4px 6px;
  }

  .sync-status {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 6px 0;
  }

  .sync-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .sync-indicator.connected {
    background: var(--accent);
  }

  .sync-indicator.slow {
    background: transparent;
    border: 1.5px solid var(--accent);
  }

  .sync-indicator.offline {
    background: var(--text-tertiary);
  }

  .sync-label {
    font-size: 12px;
    color: var(--text-primary);
  }

  .unsent {
    font-size: 11px;
    color: var(--accent);
    font-weight: 500;
    margin-left: auto;
  }
</style>
