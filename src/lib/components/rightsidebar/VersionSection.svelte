<script lang="ts">
  import DsSection from '../../design-system/DsSection.svelte';
  import {
    versionState,
    getSignificantVersions,
  } from '../../state/versions.svelte.js';
  import { groupByDate } from '../../utils/time.js';
  import { formatShortTime } from '../../utils/time.js';
  import {
    getSelectedDoc,
    getSelectedHistoryVersionId,
    getSelectedProjectId,
    navigateToHistory,
  } from '../../navigation/workspace-router.svelte.js';


  const activeDoc = $derived(getSelectedDoc());
  const significant = $derived(getSignificantVersions());
  const versionCount = $derived(significant.length);

  const grouped = $derived(
    groupByDate(significant, (v) => v.createdAt),
  );

  function handleSelectVersion(versionId: string) {
    const projectId = getSelectedProjectId();
    if (!projectId || !activeDoc) return;
    void navigateToHistory(
      projectId,
      activeDoc.id,
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

<DsSection className="section-shell" count={versionCount > 0 ? versionCount : null} title="versions">
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
            <section class="date-group" aria-label={dateLabel}>
              <h3 class="date-label">{dateLabel}</h3>
              <ul class="version-group-list">
                {#each versions as version (version.id)}
                  <li class="version-list-item">
                    <button
                      aria-current={getSelectedHistoryVersionId() === version.id ? 'page' : undefined}
                      class="version-item"
                      class:selected={getSelectedHistoryVersionId() === version.id}
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
                  </li>
                {/each}
              </ul>
            </section>
          {/each}
        </div>
      {/if}
    {:else}
      <p class="empty-text">select a document to see versions</p>
    {/if}
  </div>
</DsSection>

<style>
  :global(.section-shell) {
    flex: 1;
  }

  .section-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 16px 12px;
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

  .version-group-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    list-style: none;
  }

  .version-list-item {
    list-style: none;
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
