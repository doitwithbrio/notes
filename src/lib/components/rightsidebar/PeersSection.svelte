<script lang="ts">
  import DsSection from '../../design-system/DsSection.svelte';
  import { getVisibleProjectPeers, isProjectPeersLoading } from '../../state/presence.svelte.js';
  import { getProject } from '../../state/projects.svelte.js';
  import { openShareDialog, removePeer } from '../../state/invite.svelte.js';
  import PeerItem from './PeerItem.svelte';
  import { getWorkspaceProjectId } from '../../navigation/workspace-router.svelte.js';

  const activeProject = $derived(getProject(getWorkspaceProjectId()));
  const isOwner = $derived(activeProject?.canManagePeers ?? false);
  const peers = $derived(getVisibleProjectPeers(activeProject?.id ?? null));
  const loading = $derived(isProjectPeersLoading(activeProject?.id ?? null));

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

<DsSection className="section-shell" count={peers.length > 0 ? peers.length : null} divider testId="peers-section" title="peers">
  <div class="section-body">
    <ul class="peer-list">
      {#each peers as peer (peer.id)}
        <li class="peer-list-item">
          <PeerItem {peer} {isOwner} onremove={() => handleRemovePeer(peer.id)} />
        </li>
      {/each}

      {#if !loading && peers.length === 0}
        <li class="peer-list-item">
          <p class="empty-text" data-testid="peers-empty">none</p>
        </li>
      {/if}
    </ul>

    {#if isOwner}
      <button class="invite-btn" data-testid="peers-invite-trigger" onclick={handleInvite}>+ invite</button>
    {/if}
  </div>
</DsSection>

<style>
  .section-body {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    padding: 0 16px 12px;
  }

  .peer-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 8px;
    list-style: none;
  }

  .peer-list-item {
    list-style: none;
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
