<script lang="ts">
  import { presenceState } from '../../state/presence.svelte.js';
  import { getProject } from '../../state/projects.svelte.js';
  import { openShareDialog, removePeer } from '../../state/invite.svelte.js';
  import PeerItem from './PeerItem.svelte';
  import { getWorkspaceProjectId } from '../../navigation/workspace-router.svelte.js';

  const onlinePeers = $derived(presenceState.peers.filter((p) => p.online));
  const offlinePeers = $derived(presenceState.peers.filter((p) => !p.online));

  const activeProject = $derived(getProject(getWorkspaceProjectId()));
  const isOwner = $derived(activeProject?.role === 'owner');

  function handleInvite() {
    if (activeProject) {
      openShareDialog(activeProject.id);
    }
  }

  function handleRemovePeer(peerId: string) {
    if (activeProject) {
      void removePeer(activeProject.id, peerId);
    }
  }
</script>

<section class="section" data-testid="peers-section">
  <div class="section-header">
    <span class="section-title">peers</span>
    {#if presenceState.peers.length > 0}
      <span class="section-count">{presenceState.peers.length}</span>
    {/if}
  </div>

  <div class="section-body">
    <div class="peer-list">
      {#each onlinePeers as peer (peer.id)}
        <PeerItem {peer} {isOwner} onremove={() => handleRemovePeer(peer.id)} />
      {/each}

      {#each offlinePeers as peer (peer.id)}
        <PeerItem {peer} {isOwner} onremove={() => handleRemovePeer(peer.id)} />
      {/each}

      {#if presenceState.peers.length === 0}
        <p class="empty-text" data-testid="peers-empty">no peers connected</p>
      {/if}
    </div>

    {#if isOwner}
      <button class="invite-btn" data-testid="peers-invite-trigger" onclick={handleInvite}>+ invite</button>
    {/if}
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
    margin-bottom: 8px;
  }

  .empty-text {
    font-size: 13px;
    color: var(--text-tertiary);
    padding: 4px 6px;
    margin-bottom: 8px;
  }

  .invite-btn {
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
    padding: 4px 6px;
    border-radius: 6px;
    transition: background var(--transition-fast);
  }

  .invite-btn:hover {
    background: var(--surface-hover);
  }
</style>
