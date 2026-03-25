<script lang="ts">
  import { historyState, exitHistoryReview, restoreSession } from '../../state/history.svelte.js';
  import { editorSessionState } from '../../session/editor-session.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import { RotateCcw } from 'lucide-svelte';

  let { onRestore }: { onRestore?: () => void } = $props();
  let showConfirm = $state(false);
  let restoring = $state(false);
  let restoreError = $state<string | null>(null);

  const session = $derived(
    historyState.sessions.find((s) => s.id === historyState.selectedSessionId),
  );

  const displayName = $derived.by(() => {
    if (!session) return 'unknown version';
    const alias = historyState.actorAliases[session.actor];
    if (alias) return alias;
    return session.actor.slice(0, 8) + '...';
  });

  const dateStr = $derived.by(() => {
    if (!session) return '';
    return new Date(session.startedAt * 1000).toLocaleString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      hour12: true,
    }).toLowerCase();
  });

  function handleBack() {
    uiState.view = 'editor';
    uiState.historyReviewSessionId = null;
    exitHistoryReview();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (showConfirm) {
        handleCancel();
      } else {
        handleBack();
      }
    }
  }

  function handleRestoreClick() {
    restoreError = null;
    showConfirm = true;
  }

  function handleCancel() {
    showConfirm = false;
    restoreError = null;
  }

  async function handleConfirmRestore() {
    if (!editorSessionState.projectId || !editorSessionState.docId || !session) return;
    restoring = true;
    restoreError = null;
    try {
      await restoreSession(
        editorSessionState.projectId,
        editorSessionState.docId,
        session.id,
      );
      uiState.view = 'editor';
      uiState.historyReviewSessionId = null;
      onRestore?.();
    } catch (error) {
      restoreError = error instanceof Error ? error.message : 'restore failed';
      // Keep dialog open so user can see the error
    } finally {
      restoring = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="review-bar">
  <div class="bar-left">
    <RotateCcw size={14} strokeWidth={1.5} />
    <span class="label">viewing: {displayName} edited on {dateStr}</span>
  </div>
  <div class="bar-center">
    <span class="legend-item"><span class="legend-dot added"></span>added</span>
    <span class="legend-item"><span class="legend-dot removed"></span>removed</span>
    <span class="legend-item"><span class="legend-dot changed"></span>changed</span>
  </div>
  <div class="bar-right">
    <button class="btn-restore" onclick={handleRestoreClick} disabled={restoring}>restore</button>
    <button class="btn-back" onclick={handleBack}>back to live</button>
  </div>
</div>

{#if showConfirm}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="confirm-backdrop" onkeydown={(e) => { if (e.key === 'Escape') handleCancel(); }}>
    <div
      class="confirm-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="restore-dialog-title"
      aria-describedby="restore-dialog-desc"
      onclick={(e) => e.stopPropagation()}
    >
      <h3 id="restore-dialog-title">restore this version?</h3>
      <div id="restore-dialog-desc">
        <p>
          this will create a new edit that sets the document content to how
          it looked on {dateStr}.
        </p>
        <p>
          all collaborators will see the change. the current version is
          preserved in history and can be restored later.
        </p>
      </div>
      {#if restoreError}
        <p class="restore-error">{restoreError}</p>
      {/if}
      <div class="confirm-actions">
        <button class="btn-cancel" onclick={handleCancel}>cancel</button>
        <button class="btn-confirm" onclick={handleConfirmRestore} disabled={restoring}>
          {restoring ? 'restoring...' : 'restore'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .review-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 20px;
    height: 40px;
    background: color-mix(in srgb, var(--accent) 8%, var(--surface));
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }

  .bar-left {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);
  }

  .label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .bar-center {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: rgba(0, 0, 0, 0.40);
  }

  .legend-dot {
    width: 7px;
    height: 7px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .legend-dot.added { background: #6BAA8A; }
  .legend-dot.removed { background: #C4836A; }
  .legend-dot.changed { background: #C4A64E; }

  .bar-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .btn-restore {
    font-size: 11px;
    font-weight: 600;
    background: var(--accent);
    color: var(--white);
    padding: 4px 10px;
    border-radius: 10px;
    line-height: 1.3;
    transition: opacity var(--transition-fast);
  }

  .btn-restore:hover { opacity: 0.85; }
  .btn-restore:disabled { opacity: 0.5; cursor: default; }

  .btn-back {
    font-size: 11px;
    color: var(--text-secondary);
    transition: color var(--transition-fast);
  }

  .btn-back:hover {
    color: var(--text-primary);
    text-decoration: underline;
  }

  /* ── Confirm Dialog ── */

  .confirm-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.15);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .confirm-dialog {
    background: var(--surface);
    border-radius: 12px;
    padding: 28px;
    max-width: 380px;
    width: 90%;
    box-shadow: var(--shadow-overlay);
  }

  .confirm-dialog h3 {
    font-size: 17px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 12px;
  }

  .confirm-dialog p {
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.65;
    margin-bottom: 8px;
  }

  .restore-error {
    font-size: 12px;
    color: #a04130;
    margin-top: 8px;
    padding: 6px 10px;
    background: rgba(160, 65, 48, 0.06);
    border-radius: 6px;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }

  .btn-cancel {
    font-size: 13px;
    color: var(--text-secondary);
    padding: 6px 14px;
    border-radius: 8px;
    transition: background var(--transition-fast);
  }

  .btn-cancel:hover { background: var(--surface-hover); }

  .btn-confirm {
    font-size: 13px;
    font-weight: 600;
    background: var(--accent);
    color: var(--white);
    padding: 6px 14px;
    border-radius: 8px;
    transition: opacity var(--transition-fast);
  }

  .btn-confirm:hover { opacity: 0.85; }
  .btn-confirm:disabled { opacity: 0.5; cursor: default; }
</style>
