<script lang="ts">
  import { getActiveDoc } from '../../state/documents.svelte.js';
  import { presenceState } from '../../state/presence.svelte.js';

  const activeDoc = $derived(getActiveDoc());

  const peersInDoc = $derived(
    activeDoc
      ? presenceState.peers.filter(
          (p) => activeDoc.activePeers.includes(p.id) && p.online,
        )
      : [],
  );
</script>

<div class="editor-header">
  <h1 class="doc-title">
    {activeDoc?.title ?? 'Untitled'}
  </h1>

  {#if peersInDoc.length > 0}
    <div class="peer-avatars">
      {#each peersInDoc.slice(0, 5) as peer (peer.id)}
        <div
          class="avatar"
          style="background: {peer.cursorColor}; border-color: {peer.cursorColor}"
          title="{peer.alias} (online)"
        >
          {peer.alias[0]?.toUpperCase() ?? '?'}
        </div>
      {/each}
      {#if peersInDoc.length > 5}
        <div class="avatar-overflow">+{peersInDoc.length - 5}</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .editor-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 24px 0;
    gap: 12px;
  }

  .doc-title {
    font-family: var(--font-display);
    font-size: 28px;
    font-weight: 300;
    letter-spacing: 0.01em;
    color: var(--black);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .peer-avatars {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .avatar {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--white);
    border: 2px solid;
  }

  .avatar-overflow {
    font-size: 11px;
    font-weight: 500;
    color: var(--black);
    padding-left: 2px;
  }
</style>
