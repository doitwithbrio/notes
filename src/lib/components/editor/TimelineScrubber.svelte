<script lang="ts">
  import { ChevronLeft, ChevronRight } from 'lucide-svelte';
  import { editorSessionState } from '../../session/editor-session.svelte.js';
  import { getSignificantVersions } from '../../state/versions.svelte.js';
  import { setVersionViewMode, versionReviewState } from '../../state/version-review.svelte.js';
  import { formatShortTime } from '../../utils/time.js';
  import {
    getHistoryVersionId,
    getWorkspaceRoute,
    isHistoryRoute,
    navigateBackToLive,
    navigateHistoryNewer,
    navigateHistoryOlder,
    navigateToHistory,
    restoreHistoryVersion,
  } from '../../navigation/workspace-router.svelte.js';

  const versions = $derived(getSignificantVersions());
  const hasVersions = $derived(versions.length > 0);
  const isReviewing = $derived.by(() => isHistoryRoute(getWorkspaceRoute()));
  const selectedVersionId = $derived(getHistoryVersionId());
  const selectedIdx = $derived.by(() => versions.findIndex((v) => v.id === selectedVersionId));
  const canGoPrev = $derived(selectedIdx >= 0 && selectedIdx < versions.length - 1);
  const canGoNext = $derived(selectedIdx > 0);

  function handleTickClick(versionId: string) {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    void navigateToHistory(editorSessionState.projectId, editorSessionState.docId, versionId);
  }

  function handlePrev() {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    if (!isReviewing && versions.length > 0) {
      void navigateToHistory(editorSessionState.projectId, editorSessionState.docId, versions[0]!.id);
      return;
    }
    void navigateHistoryOlder(editorSessionState.projectId, editorSessionState.docId);
  }

  function handleNext() {
    if (!editorSessionState.projectId || !editorSessionState.docId || !canGoNext) return;
    void navigateHistoryNewer(editorSessionState.projectId, editorSessionState.docId);
  }

  function tickPosition(index: number, total: number): number {
    if (total <= 1) return 50;
    return ((total - 1 - index) / (total - 1)) * 100;
  }

  const selectedVersion = $derived(versions.find((v) => v.id === selectedVersionId));
  const canRestore = $derived(
    !!selectedVersionId
      && versionReviewState.status === 'ready'
      && !!editorSessionState.projectId
      && !!editorSessionState.docId
      && editorSessionState.canEdit,
  );

  let restoreConfirmVisible = $state(false);
  let restorePending = $state(false);
  let restoreError = $state<string | null>(null);

  const historyStatus = $derived.by(() => {
    if (!selectedVersion) return 'history';

    const parts = [selectedVersion.name];
    if (selectedVersion.createdAt > 0) parts.push(formatShortTime(selectedVersion.createdAt));
    return parts.join(' · ');
  });
  const historyStateLabel = $derived.by(() => {
    if (versionReviewState.status === 'loading') return 'Loading version...';
    if (versionReviewState.status === 'error') return versionReviewState.previewError ?? 'Version unavailable';
    return 'Reviewing saved version';
  });
  const restoreCopy = $derived.by(() => {
    if (!selectedVersion) return 'Restore this version to live?';
    const parts = [`Restore ${selectedVersion.name}`];
    if (selectedVersion.createdAt > 0) {
      parts.push(formatShortTime(selectedVersion.createdAt));
    }
    if (selectedVersion.label) {
      parts.push(`"${selectedVersion.label}"`);
    }
    return `${parts.join(' · ')} to the live note?`;
  });

  $effect(() => {
    selectedVersionId;
    isReviewing;
    restoreConfirmVisible = false;
    restorePending = false;
    restoreError = null;
  });

  function handleRestoreIntent() {
    if (!canRestore) return;
    restoreConfirmVisible = true;
    restoreError = null;
  }

  function handleRestoreCancel() {
    restoreConfirmVisible = false;
    restorePending = false;
    restoreError = null;
  }

  async function handleRestoreConfirm() {
    if (!selectedVersionId || !editorSessionState.projectId || !editorSessionState.docId || !canRestore) return;

    restorePending = true;
    restoreError = null;
    try {
      const restored = await restoreHistoryVersion(editorSessionState.projectId, editorSessionState.docId, selectedVersionId);
      if (!restored) {
        restoreError = 'Restore is unavailable right now.';
        restoreConfirmVisible = false;
      }
    } catch (error) {
      restoreError = error instanceof Error ? error.message : 'Failed to restore version';
      restoreConfirmVisible = false;
    } finally {
      restorePending = false;
    }
  }
</script>

{#if hasVersions}
  <div class="history-bar" class:reviewing={isReviewing}>
    <div class="history-nav">
      <button
        class="nav-btn"
        onclick={handlePrev}
        aria-label="Older version"
        disabled={isReviewing && !canGoPrev}
        title="Older version"
      >
        <ChevronLeft size={14} strokeWidth={2} />
      </button>

      <div class="track">
        <div class="track-line"></div>

        {#each versions as version, i (version.id)}
          <button
            class="tick"
            class:named={version.type === 'named'}
            class:active={version.id === selectedVersionId}
            style="left: {tickPosition(i, versions.length)}%"
            aria-label="Open version {version.name}"
            onclick={() => handleTickClick(version.id)}
            title="{version.name} · {version.createdAt > 0 ? formatShortTime(version.createdAt) : ''}{version.label ? ` · "${version.label}"` : ''}"
          >
            <span class="tick-mark"></span>
          </button>
        {/each}
      </div>

      <button
        class="nav-btn"
        onclick={handleNext}
        aria-label="Newer version"
        disabled={!canGoNext}
        title="Newer version"
      >
        <ChevronRight size={14} strokeWidth={2} />
      </button>
    </div>

    <div class="history-meta">
      <div class="meta-main">
        <div class="meta-heading">
          <span class="history-label">History</span>
          <span class="mode-badge">Read only</span>
        </div>
        <span class="version-label">{historyStatus}</span>
        <span class="history-state">{historyStateLabel}</span>
        {#if selectedVersion?.label}
          <span class="user-label">"{selectedVersion.label}"</span>
        {/if}
      </div>
    </div>

    <div class="history-actions">
      <div class="view-toggle" role="group" aria-label="History view mode">
        <button
          class:selected={versionReviewState.viewMode === 'snapshot'}
          aria-pressed={versionReviewState.viewMode === 'snapshot'}
          onclick={() => setVersionViewMode('snapshot')}
          type="button"
        >
          Snapshot
        </button>
        <button
          class:selected={versionReviewState.viewMode === 'diff'}
          aria-pressed={versionReviewState.viewMode === 'diff'}
          onclick={() => setVersionViewMode('diff')}
          type="button"
        >
          Diff
        </button>
      </div>

      {#if restoreConfirmVisible}
        <div class="restore-shell restore-shell-confirming">
        <div class="restore-confirm">
          <div class="restore-copy-wrap">
            <span class="restore-copy">{restoreCopy}</span>
            <span class="restore-note">This applies as a new live change.</span>
          </div>
          <button class="restore-cancel" onclick={handleRestoreCancel} disabled={restorePending} type="button">Cancel</button>
          <button class="restore-confirm-btn" onclick={handleRestoreConfirm} disabled={restorePending} type="button">
            {restorePending ? 'Restoring...' : 'Confirm restore'}
          </button>
        </div>
        </div>
      {:else}
        <div class="restore-shell">
          <button class="restore-btn" onclick={handleRestoreIntent} disabled={!canRestore} type="button">Restore</button>
        </div>
      {/if}

      <div class="action-separator" aria-hidden="true"></div>
      <button class="back-btn" onclick={navigateBackToLive} type="button">Back to live</button>
    </div>
  </div>
  {#if restoreError}
    <div class="history-error">{restoreError}</div>
  {/if}
{/if}

<style>
  .history-bar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 18px;
    min-height: 64px;
    background: var(--surface);
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }

  .history-bar.reviewing {
    box-shadow: inset 0 -1px 0 color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .history-nav {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1 1 38%;
    min-width: 240px;
  }

  .nav-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    color: var(--text-secondary);
    flex-shrink: 0;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .nav-btn:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .nav-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .track {
    flex: 1;
    position: relative;
    height: 24px;
    display: flex;
    align-items: center;
  }

  .track-line {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 3px;
    background: color-mix(in srgb, var(--border-default) 76%, transparent);
    border-radius: 1px;
    transform: translateY(-50%);
  }

  .tick {
    position: absolute;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    z-index: 1;
    padding: 8px 6px;
    cursor: pointer;
  }

  .tick-mark {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-tertiary);
    transition: transform var(--transition-fast), background var(--transition-fast);
  }

  .tick.named .tick-mark {
    width: 10px;
    height: 10px;
    background: var(--accent);
  }

  .tick.active .tick-mark {
    transform: scale(1.5);
    background: var(--accent);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .tick:hover .tick-mark {
    transform: scale(1.3);
    background: var(--text-primary);
  }

  .history-meta {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    flex: 0 1 36%;
  }

  .meta-main {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    min-width: 0;
  }

  .meta-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .history-label {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  .version-label {
    font-size: 15px;
    font-weight: 700;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mode-badge {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--surface-hover) 72%, transparent);
    padding: 3px 8px;
    border-radius: 999px;
  }

  .user-label {
    font-size: 12px;
    font-weight: 400;
    color: var(--text-secondary);
    font-style: italic;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .history-state {
    font-size: 12px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .history-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    flex: 0 1 30%;
    min-width: 280px;
  }

  .view-toggle {
    display: inline-flex;
    align-items: center;
    padding: 3px;
    background: var(--surface-hover);
    border-radius: 999px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--border-default) 72%, transparent);
  }

  .restore-shell {
    min-width: 248px;
    display: flex;
    justify-content: flex-end;
  }

  .restore-shell-confirming {
    flex: 1 1 auto;
  }

  .action-separator {
    width: 1px;
    height: 22px;
    background: color-mix(in srgb, var(--border-default) 70%, transparent);
    flex-shrink: 0;
  }

  .view-toggle button,
  .restore-btn,
  .restore-cancel,
  .restore-confirm-btn,
  .back-btn {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    padding: 6px 10px;
    border-radius: 999px;
    transition: background var(--transition-fast);
  }

  .view-toggle button.selected {
    background: var(--surface);
    box-shadow: 0 0 0 1px var(--border-default);
  }

  .back-btn {
    background: var(--surface-hover);
  }

  .restore-btn {
    background: color-mix(in srgb, var(--accent) 10%, var(--surface-hover));
  }

  .restore-confirm {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: 999px;
    background: var(--surface-hover);
    min-height: 40px;
    max-width: 100%;
  }

  .restore-copy-wrap {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .restore-copy {
    font-size: 12px;
    color: var(--text-primary);
    white-space: normal;
    line-height: 1.25;
    max-width: 320px;
  }

  .restore-note {
    font-size: 10px;
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .restore-cancel {
    background: transparent;
  }

  .restore-confirm-btn {
    background: color-mix(in srgb, var(--accent) 14%, var(--surface));
  }

  .view-toggle button:hover,
  .restore-btn:hover,
  .restore-cancel:hover,
  .restore-confirm-btn:hover,
  .back-btn:hover {
    background: var(--surface-active);
  }

  .restore-btn:disabled,
  .restore-cancel:disabled,
  .restore-confirm-btn:disabled,
  .back-btn:disabled,
  .view-toggle button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .history-error {
    padding: 0 16px 10px;
    font-size: 12px;
    color: var(--danger-fg);
    background: var(--surface);
    border-bottom: 1px solid var(--border-default);
  }

  @media (max-width: 1100px) {
    .history-bar {
      flex-wrap: wrap;
      gap: 12px;
    }

    .history-nav,
    .history-meta,
    .history-actions {
      min-width: 0;
      flex: 1 1 100%;
    }

    .history-actions {
      justify-content: flex-start;
      flex-wrap: wrap;
      gap: 10px;
    }

    .restore-shell,
    .restore-shell-confirming {
      min-width: 0;
      flex: 1 1 100%;
      justify-content: flex-start;
    }

    .restore-confirm {
      border-radius: 18px;
      flex-wrap: wrap;
      width: 100%;
    }

    .restore-copy {
      max-width: none;
      white-space: normal;
    }

    .restore-note {
      white-space: normal;
    }

    .action-separator {
      display: none;
    }
  }
</style>
