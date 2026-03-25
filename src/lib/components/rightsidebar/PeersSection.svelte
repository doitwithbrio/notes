<script lang="ts">
  import { presenceState } from '../../state/presence.svelte.js';

  const onlinePeers = $derived(presenceState.peers.filter((p) => p.online));
  const offlinePeers = $derived(presenceState.peers.filter((p) => !p.online));
</script>

<section class="section">
  <div class="section-header">
    <span class="section-title">peers</span>
    {#if presenceState.peers.length > 0}
      <span class="section-count">{presenceState.peers.length}</span>
    {/if}
  </div>

  <div class="section-body">
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
  </div>
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    border-bottom: 1px solid var(--border-subtle);
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 12px 16px 8px;
    flex-shrink: 0;
  }

  .section-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 16px 12px;
  }

  .section-title {
    font-size: 13px;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--text-primary);
  }

  .section-count {
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
    color: var(--text-tertiary);
  }

  .empty-text {
    font-size: 13px;
    color: var(--text-tertiary);
    padding: 4px 6px;
  }
</style>
