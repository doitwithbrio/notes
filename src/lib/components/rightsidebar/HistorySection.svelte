<script lang="ts">
  import { getActiveDoc } from '../../state/documents.svelte.js';
  import { historyState } from '../../state/history.svelte.js';
  import { Clock } from 'lucide-svelte';

  const activeDoc = $derived(getActiveDoc());
</script>

<section class="section">
  <div class="section-header">
    <span class="section-title">history</span>
  </div>

  {#if activeDoc}
    {#if historyState.loading}
      <p class="empty-text">loading history...</p>
    {:else if historyState.error}
      <p class="empty-text">{historyState.error}</p>
    {:else if historyState.sessions.length === 0}
      <div class="placeholder">
        <Clock size={16} strokeWidth={1.5} />
        <p>no saved edit sessions yet</p>
        <p class="sub">history will appear after the first committed changes</p>
      </div>
    {:else}
      <div class="history-list">
        {#each historyState.sessions as session (session.id)}
          <div class="history-item">
            <p class="history-title">{session.actor.slice(0, 12)} edited</p>
            <p class="history-meta">
              {new Date(session.startedAt * 1000).toLocaleString()} · {session.changeCount} changes
            </p>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <p class="empty-text">select a document to see history</p>
  {/if}
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

  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 20px 12px;
    color: var(--text-primary);
    text-align: center;
    font-size: 12px;
    line-height: 1.5;
  }

  .placeholder .sub {
    font-size: 11px;
    opacity: 0.7;
  }

  .empty-text {
    font-size: 12px;
    color: var(--text-primary);
    padding: 4px 6px;
  }

  .history-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .history-item {
    padding: 10px;
    border-radius: 8px;
    background: var(--surface-hover);
  }

  .history-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .history-meta {
    font-size: 11px;
    color: var(--text-primary);
    margin-top: 2px;
  }
</style>
