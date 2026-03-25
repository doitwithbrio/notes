<script lang="ts">
  import { isMac } from '../utils/platform.js';
  import { getActiveDoc } from '../state/documents.svelte.js';
  import { presenceState } from '../state/presence.svelte.js';
  import { syncState } from '../state/sync.svelte.js';

  const activeDoc = $derived(getActiveDoc());

  const peersInDoc = $derived(
    activeDoc
      ? presenceState.peers.filter(
          (p) => activeDoc.activePeers.includes(p.id) && p.online,
        )
      : [],
  );
</script>

<div class="drag-region" data-tauri-drag-region>
  {#if isMac}
    <div class="traffic-lights-spacer"></div>
  {/if}

  <div class="spacer" data-tauri-drag-region></div>

  <div class="top-right">
    {#if peersInDoc.length > 0}
      <div class="peer-avatars">
        {#each peersInDoc.slice(0, 5) as peer (peer.id)}
          <div
            class="avatar"
            style="background: {peer.cursorColor}"
            title="{peer.alias}"
          >
            {peer.alias[0]?.toLowerCase() ?? '?'}
          </div>
        {/each}
        {#if peersInDoc.length > 5}
          <span class="avatar-overflow">+{peersInDoc.length - 5}</span>
        {/if}
      </div>
    {/if}

    {#if syncState.connection !== 'local'}
    <div
      class="sync-dot"
      class:connected={syncState.connection === 'connected'}
      class:slow={syncState.connection === 'slow'}
      class:offline={syncState.connection === 'offline'}
      title={syncState.connection === 'connected'
        ? 'synced'
        : syncState.connection === 'slow'
          ? 'sync slow'
          : 'offline'}
    ></div>
    {/if}
  </div>
</div>

<style>
  .drag-region {
    display: flex;
    align-items: center;
    height: var(--drag-height);
    padding: 0 20px;
    user-select: none;
    -webkit-user-select: none;
    position: relative;
    z-index: 10;
  }

  .traffic-lights-spacer {
    width: 68px;
    flex-shrink: 0;
  }

  .spacer {
    flex: 1;
  }

  .top-right {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
    padding-right: 4px;
  }

  .peer-avatars {
    display: flex;
    align-items: center;
    gap: -4px;
  }

  .avatar {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 500;
    color: var(--white);
    margin-left: -4px;
  }

  .avatar:first-child {
    margin-left: 0;
  }

  .avatar-overflow {
    font-size: 10px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-left: 4px;
  }

  .sync-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background var(--transition-normal);
  }

  .sync-dot.connected {
    background: var(--accent);
  }

  .sync-dot.slow {
    background: transparent;
    border: 1.5px solid var(--accent);
  }

  .sync-dot.offline {
    background: var(--text-tertiary);
  }
</style>
