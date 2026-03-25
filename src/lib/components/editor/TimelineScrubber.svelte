<script lang="ts">
  import { ChevronLeft, ChevronRight } from 'lucide-svelte';
  import { editorSessionState } from '../../session/editor-session.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import {
    versionState,
    getSignificantVersions,
    selectVersion,
    selectPrevVersion,
    selectNextVersion,
    exitVersionReview,
  } from '../../state/versions.svelte.js';
  import { formatShortTime } from '../../utils/time.js';

  const versions = $derived(getSignificantVersions());
  const hasVersions = $derived(versions.length > 0);
  const isReviewing = $derived(uiState.view === 'history-review');
  const selectedIdx = $derived(versionState.selectedVersionIndex);

  // Are we at the oldest/newest version?
  const canGoPrev = $derived(selectedIdx < versions.length - 1);
  const canGoNext = $derived(selectedIdx > 0 || isReviewing);

  function handleTickClick(versionId: string) {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    uiState.view = 'history-review';
    uiState.historyReviewSessionId = versionId;
    selectVersion(
      editorSessionState.projectId,
      editorSessionState.docId,
      versionId,
    );
  }

  function handlePrev() {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    if (!isReviewing && versions.length > 0) {
      // Enter review mode at the most recent version
      uiState.view = 'history-review';
      handleTickClick(versions[0]!.id);
      return;
    }
    uiState.view = 'history-review';
    selectPrevVersion(editorSessionState.projectId, editorSessionState.docId);
  }

  function handleNext() {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    if (selectedIdx <= 0) {
      // Go back to live
      uiState.view = 'editor';
      uiState.historyReviewSessionId = null;
      exitVersionReview();
      return;
    }
    selectNextVersion(editorSessionState.projectId, editorSessionState.docId);
  }

  // Get position percentage for a version on the timeline (0 = oldest, 100 = newest)
  function tickPosition(index: number, total: number): number {
    if (total <= 1) return 50;
    return ((total - 1 - index) / (total - 1)) * 100;
  }

  const selectedVersion = $derived(
    versions.find((v) => v.id === versionState.selectedVersionId),
  );
</script>

{#if hasVersions}
  <div class="scrubber" class:reviewing={isReviewing}>
    <button
      class="nav-btn"
      onclick={handlePrev}
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
          class:active={version.id === versionState.selectedVersionId}
          style="left: {tickPosition(i, versions.length)}%"
          onclick={() => handleTickClick(version.id)}
          title="{version.name} · {version.createdAt > 0 ? formatShortTime(version.createdAt) : ''}{version.label ? ` · "${version.label}"` : ''}"
        >
          <span class="tick-mark"></span>
        </button>
      {/each}

      <!-- Live indicator at the end -->
      <div class="live-dot" class:active={!isReviewing}>
        <span class="live-label">live</span>
      </div>
    </div>

    <button
      class="nav-btn"
      onclick={handleNext}
      disabled={!isReviewing}
      title={isReviewing && selectedIdx <= 0 ? 'Back to live' : 'Newer version'}
    >
      <ChevronRight size={14} strokeWidth={2} />
    </button>

    {#if isReviewing && selectedVersion}
      <span class="version-label">
        {selectedVersion.name}
        {#if selectedVersion.label}
          <span class="user-label">"{selectedVersion.label}"</span>
        {/if}
      </span>
    {/if}
  </div>
{/if}

<style>
  .scrubber {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 16px;
    height: 32px;
    background: var(--surface);
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
    opacity: 0.5;
    transition: opacity var(--transition-fast);
  }

  .scrubber:hover,
  .scrubber.reviewing {
    opacity: 1;
  }

  .nav-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
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
    height: 22px;
    display: flex;
    align-items: center;
  }

  .track-line {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 2px;
    background: var(--border-default);
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
    padding: 4px;
    cursor: pointer;
  }

  .tick-mark {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-tertiary);
    transition: transform var(--transition-fast), background var(--transition-fast);
  }

  .tick.named .tick-mark {
    width: 8px;
    height: 8px;
    background: var(--accent);
  }

  .tick.active .tick-mark {
    transform: scale(1.5);
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 20%, transparent);
  }

  .tick:hover .tick-mark {
    transform: scale(1.3);
    background: var(--text-primary);
  }

  .live-dot {
    position: absolute;
    right: -2px;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .live-dot::before {
    content: '';
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-tertiary);
    transition: background var(--transition-fast);
  }

  .live-dot.active::before {
    background: var(--accent);
  }

  .live-label {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-tertiary);
  }

  .live-dot.active .live-label {
    color: var(--accent);
  }

  .version-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    flex-shrink: 0;
    margin-left: 4px;
  }

  .user-label {
    font-weight: 400;
    color: var(--text-secondary);
    font-style: italic;
  }
</style>
