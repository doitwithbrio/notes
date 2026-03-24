<script lang="ts">
  import type { Document } from '../../types/index.js';
  import { documentState, setActiveDoc } from '../../state/documents.svelte.js';
  import { presenceState } from '../../state/presence.svelte.js';

  let { doc }: { doc: Document } = $props();

  const isActive = $derived(documentState.activeDocId === doc.id);

  const peersHere = $derived(
    presenceState.peers.filter((p) => doc.activePeers.includes(p.id) && p.online),
  );

  const syncIcon = $derived(
    doc.syncStatus === 'synced'
      ? '\u2713'
      : doc.syncStatus === 'syncing'
        ? '\u21BB'
        : '\u2601\u0338',
  );
</script>

<button class="file-item" class:active={isActive} onclick={() => setActiveDoc(doc.id)}>
  <span class="file-name">{doc.title}</span>

  <span class="file-meta">
    {#if peersHere.length > 0}
      <span class="presence-dots">
        {#each peersHere.slice(0, 3) as peer (peer.id)}
          <span class="dot" style="background: {peer.cursorColor}"></span>
        {/each}
        {#if peersHere.length > 3}
          <span class="dot-overflow">+{peersHere.length - 3}</span>
        {/if}
      </span>
    {/if}

    <span class="sync-icon" class:syncing={doc.syncStatus === 'syncing'}>{syncIcon}</span>
  </span>
</button>

<style>
  .file-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 4px 16px 4px 20px;
    font-size: 13px;
    text-align: left;
    color: var(--black);
    background: var(--white);
  }

  .file-item:hover {
    color: var(--accent);
  }

  .file-item.active {
    background: var(--accent);
    color: var(--white);
  }

  .file-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .presence-dots {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .dot-overflow {
    font-size: 9px;
    color: var(--black);
  }

  .file-item.active .dot-overflow {
    color: var(--white);
  }

  .sync-icon {
    font-size: 11px;
    width: 14px;
    text-align: center;
  }

  .sync-icon.syncing {
    color: var(--accent);
    animation: spin 1s linear infinite;
  }

  .file-item.active .sync-icon {
    color: var(--white);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
