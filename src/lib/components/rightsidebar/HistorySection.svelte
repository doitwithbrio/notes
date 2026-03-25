<script lang="ts">
  import { getActiveDoc } from '../../state/documents.svelte.js';
  import { historyState, selectSession } from '../../state/history.svelte.js';
  import { editorSessionState } from '../../session/editor-session.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import { groupByDate } from '../../utils/time.js';
  import HistorySessionItem from './HistorySessionItem.svelte';

  const activeDoc = $derived(getActiveDoc());
  const sessionCount = $derived(historyState.sessions.length);

  const grouped = $derived(
    groupByDate(historyState.sessions, (s) => s.startedAt),
  );

  function handleSelectSession(sessionId: string) {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    uiState.view = 'history-review';
    uiState.historyReviewSessionId = sessionId;
    selectSession(
      editorSessionState.projectId,
      editorSessionState.docId,
      sessionId,
    );
  }
</script>

<section class="section">
  <div class="section-label">
    <span class="section-rule"></span>
    <span class="section-name">history</span>
    {#if sessionCount > 0}
      <span class="section-count">{sessionCount}</span>
    {/if}
  </div>

  {#if activeDoc}
    {#if historyState.loading}
      <p class="empty-text">loading history...</p>
    {:else if historyState.error}
      <p class="empty-text">{historyState.error}</p>
    {:else if historyState.sessions.length === 0}
      <p class="empty-text">no edit history yet</p>
    {:else}
      <div class="history-list">
        {#each grouped as [dateLabel, sessions] (dateLabel)}
          <div class="date-group">
            <span class="date-label">{dateLabel}</span>
            {#each sessions as session (session.id)}
              <HistorySessionItem
                {session}
                selected={historyState.selectedSessionId === session.id}
                onclick={() => handleSelectSession(session.id)}
              />
            {/each}
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

  .section-label {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }

  .section-rule {
    width: 12px;
    height: 1px;
    background: rgba(182, 141, 94, 0.50);
    flex-shrink: 0;
  }

  .section-name {
    font-size: 11px;
    font-weight: 500;
    color: rgba(0, 0, 0, 0.35);
    letter-spacing: 0.06em;
  }

  .section-count {
    font-size: 10px;
    font-weight: 600;
    color: rgba(0, 0, 0, 0.35);
    background: var(--surface-active);
    padding: 0 5px;
    border-radius: 8px;
    line-height: 16px;
  }

  .empty-text {
    font-size: 12px;
    font-style: italic;
    color: rgba(0, 0, 0, 0.30);
    padding: 4px 6px;
  }

  .history-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .date-group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .date-label {
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0.04em;
    color: rgba(0, 0, 0, 0.30);
    padding: 0 10px;
    margin-bottom: 4px;
  }
</style>
