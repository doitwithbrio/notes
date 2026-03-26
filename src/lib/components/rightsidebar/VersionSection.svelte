<script lang="ts">
  import { getActiveDoc } from '../../state/documents.svelte.js';
  import { editorSessionState } from '../../session/editor-session.svelte.js';
  import {
    versionState,
    getSignificantVersions,
  } from '../../state/versions.svelte.js';
  import { groupByDate } from '../../utils/time.js';
  import { formatShortTime } from '../../utils/time.js';
  import { navigateToHistory } from '../../navigation/workspace-router.svelte.js';


  const activeDoc = $derived(getActiveDoc());
  const significant = $derived(getSignificantVersions());
  const versionCount = $derived(significant.length);

  const grouped = $derived(
    groupByDate(significant, (v) => v.createdAt),
  );

  function handleSelectVersion(versionId: string) {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    void navigateToHistory(
      editorSessionState.projectId,
      editorSessionState.docId,
      versionId,
    );
  }

  function formatStats(v: typeof significant[0]): string {
    const parts: string[] = [];
    if (v.charsAdded > 0) parts.push(`+${v.charsAdded}`);
    if (v.charsRemoved > 0) parts.push(`-${v.charsRemoved}`);
    if (v.blocksChanged > 0) parts.push(`${v.blocksChanged} blocks`);
    if (parts.length === 0) parts.push(`${v.changeCount} changes`);
    return parts.join(' · ');
  }
</script>

<section class="section">
  <div class="section-header">
    <span class="section-title">versions</span>
    {#if versionCount > 0}
      <span class="section-count">{versionCount}</span>
    {/if}
  </div>

  <div class="section-body">
    {#if activeDoc}
      {#if !versionState.supported}
        <p class="empty-text">
          restart the desktop app to reload the updated version-history backend
        </p>
      {:else if versionState.loading}
        <p class="empty-text">loading versions...</p>
      {:else if versionState.error}
        <p class="empty-text">{versionState.error}</p>
      {:else if significant.length === 0}
        <p class="empty-text">no versions yet · press ⌘S to save one</p>
      {:else}
        <div class="version-list">
          {#each grouped as [dateLabel, versions] (dateLabel)}
            <div class="date-group">
              <span class="date-label">{dateLabel}</span>
              {#each versions as version (version.id)}
                <button
                  class="version-item"
                  class:selected={versionState.selectedVersionId === version.id}
                  class:named={version.type === 'named'}
                  onclick={() => handleSelectVersion(version.id)}
                  type="button"
                >
                  <div class="row-1">
                    {#if version.type === 'named'}
                      <span class="star">★</span>
                    {:else}
                      <span class="dot"></span>
                    {/if}
                    <span class="name">{version.name}</span>
                    {#if version.createdAt > 0}
                      <span class="time">{formatShortTime(version.createdAt)}</span>
                    {/if}
                  </div>
                  {#if version.label}
                    <div class="row-label">"{version.label}"</div>
                  {/if}
                  <div class="row-2">{formatStats(version)}</div>
                </button>
              {/each}
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      <p class="empty-text">select a document to see versions</p>
    {/if}
  </div>
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;

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

  .empty-text {
    font-size: 13px;
    color: var(--text-tertiary);
    padding: 4px 6px;
  }

  .version-list {
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
    font-size: 11px;
    font-weight: 500;
    color: var(--text-tertiary);
    padding: 0 10px;
    margin-bottom: 4px;
  }

  .version-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: pointer;
    transition: background var(--transition-fast);
    text-align: left;
  }

  .version-item:hover {
    background: var(--surface-hover);
  }

  .version-item.selected {
    background: var(--surface-active);
    border-left: 2px solid var(--accent);
    padding-left: 8px;
  }

  .row-1 {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .star {
    font-size: 11px;
    color: var(--accent);
    flex-shrink: 0;
    line-height: 1;
  }

  .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--text-tertiary);
    flex-shrink: 0;
  }

  .name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .version-item.named .name {
    font-weight: 700;
  }

  .time {
    font-size: 11px;
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  .row-label {
    font-size: 11px;
    font-style: italic;
    color: var(--text-secondary);
    padding-left: 17px;
  }

  .row-2 {
    font-size: 11px;
    color: var(--text-tertiary);
    padding-left: 17px;
  }
</style>
