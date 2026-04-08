<script lang="ts">
  import { untrack } from 'svelte';
  import type { Editor, EditorAdapter } from '../../editor/setup.js';
  import type { AdapterChange } from '../../editor/automerge-prosemirror-adapter.js';
  import { createEditorAdapter, textToEditorHtml } from '../../editor/setup.js';
  import { getProjectPeerById, getRemoteCursorsForDoc } from '../../state/presence.svelte.js';
  import { syncState } from '../../state/sync.svelte.js';
  import {
    bindEditorAdapter,
    editorSessionState,
    handleBoundEditorChange,
    setLocalCursorPresence,
  } from '../../session/editor-session.svelte.js';
  import { versionState } from '../../state/versions.svelte.js';
  import { showSavePrompt, versionReviewState } from '../../state/version-review.svelte.js';
  import { computeBlockDiff, getDiffBlockTargetId } from '../../utils/diff.js';
  import { getSelectedDoc, getWorkspaceRoute, isHistoryRoute, navigateBackToLive } from '../../navigation/workspace-router.svelte.js';
  import TimelineScrubber from './TimelineScrubber.svelte';
  import SaveVersionBar from './SaveVersionBar.svelte';
  import ChangeMinibar from './ChangeMinibar.svelte';
  import type { Peer } from '../../types/index.js';

  function isOnlinePeer(peer: Peer | null): peer is Peer {
    return Boolean(peer?.online);
  }


  let editorElement = $state<HTMLDivElement | null>(null);
  let editor = $state<Editor | null>(null);
  let editorAdapter = $state<EditorAdapter | null>(null);
  let diffOverlayElement = $state<HTMLDivElement | null>(null);

  const activeDoc = $derived(getSelectedDoc());
  const isHistoryReview = $derived.by(() => isHistoryRoute(getWorkspaceRoute()));
  const peersInDoc = $derived(
    activeDoc
      ? activeDoc.activePeers
          .map((peerId) => getProjectPeerById(activeDoc.projectId, peerId))
          .filter(isOnlinePeer)
      : [],
  );
  const remoteCursors = $derived(activeDoc ? getRemoteCursorsForDoc(activeDoc.id) : []);
  const connectionLabel = $derived(
    syncState.connection === 'connected'
      ? 'connected'
      : syncState.connection === 'slow'
        ? 'syncing'
        : syncState.connection === 'local'
          ? 'local'
          : 'offline',
  );

  const previewReady = $derived(
    isHistoryReview
      && versionReviewState.status !== 'error'
      && versionReviewState.previewText != null,
  );
  const hasHistorySurface = $derived(
    isHistoryReview && (versionReviewState.previewText != null || !!versionReviewState.previewError),
  );
  const showDiffView = $derived(previewReady && versionReviewState.viewMode === 'diff');
  const showSnapshotView = $derived(previewReady && versionReviewState.viewMode === 'snapshot');
  const showHistoryOverlay = $derived(hasHistorySurface);

  const diffBlocks = $derived.by(() => {
    if (showDiffView && versionReviewState.previewText != null) {
      return computeBlockDiff(versionReviewState.previewText, editorSessionState.text);
    }
    return [];
  });
  const visibleDiffBlocks = $derived(diffBlocks.filter((block) => block.type !== 'unchanged'));

  // Total lines for minibar calculation
  const totalLines = $derived(
    editorSessionState.text.split('\n').length,
  );

  function scrollToDiffRegion(targetId: string) {
    const scrollRoot = diffOverlayElement;
    if (!scrollRoot) return;

    const target = scrollRoot.querySelector<HTMLElement>(`[data-diff-target="${targetId}"]`);
    if (!target) return;

    const scrollRootRect = scrollRoot.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const nextTop = scrollRoot.scrollTop + (targetRect.top - scrollRootRect.top) - 48;

    scrollRoot.scrollTo({ top: Math.max(0, nextTop), behavior: 'smooth' });
  }

  $effect(() => {
    const el = editorElement;
    if (!el) return;

    const adapter = createEditorAdapter(el, {
      onChange: (change: AdapterChange) => {
        if (untrack(() => isHistoryReview)) return;
        handleBoundEditorChange(change);
        editor = adapter.getEditor();
      },
      onSelectionChange: (cursorPos: number | null, selection: [number, number] | null) => {
      if (untrack(() => isHistoryReview)) {
        setLocalCursorPresence(null, null);
        return;
      }
      setLocalCursorPresence(cursorPos, selection);
      },
      onFocusChange: (focused: boolean) => {
      if (!focused) {
        setLocalCursorPresence(null, null);
      }
      },
      getProjectId: () => untrack(() => editorSessionState.projectId),
    });
    editorAdapter = adapter;
    bindEditorAdapter(adapter);
    editor = adapter.getEditor();

    return () => {
      bindEditorAdapter(null);
      adapter.detach();
      setLocalCursorPresence(null, null);
      editorAdapter = null;
      editor = null;
    };
  });

  $effect(() => {
    if (!editorAdapter) return;
    editorAdapter.updateRemotePresence(isHistoryReview ? [] : remoteCursors);
  });

  $effect(() => {
    editorSessionState.revision;
    editor = untrack(() => editorAdapter?.getEditor() ?? null);
  });

  // When entering/leaving review mode, toggle editor editability
  $effect(() => {
    const reviewing = isHistoryReview;
    const ed = untrack(() => editor);
    if (ed) {
      ed.setEditable(!reviewing && editorSessionState.canEdit);
    } else if (reviewing) {
      setLocalCursorPresence(null, null);
    }
  });

  // Exit version review when the active document changes
  let prevDocId: string | null = null;
  $effect(() => {
    const docId = editorSessionState.docId;
    if (prevDocId !== null && docId !== prevDocId) {
      if (untrack(() => isHistoryReview)) {
        navigateBackToLive();
      }
    }
    prevDocId = docId;
  });

  // Cmd+S handler — create a named version
  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && isHistoryReview) {
      e.preventDefault();
      navigateBackToLive();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
      e.preventDefault();
      if (!isHistoryReview && versionState.supported && editorSessionState.projectId && editorSessionState.docId) {
        showSavePrompt();
      }
    }
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="editor-pane" data-testid="editor-pane">
  <div class="editor-drag" data-tauri-drag-region>
    <div class="drag-spacer" data-tauri-drag-region></div>
    <div class="drag-right" style="-webkit-app-region: no-drag">
      {#if peersInDoc.length > 0}
        <div class="peer-avatars">
          {#each peersInDoc.slice(0, 3) as peer (peer.id)}
            <div class="avatar" style="background: {peer.cursorColor}" title={peer.alias}>
              {peer.alias[0]?.toLowerCase() ?? '?'}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>

  {#if isHistoryReview}
    <TimelineScrubber />
  {/if}
  <SaveVersionBar />

  {#if activeDoc}
    <div class="editor-scroll">
      {#if showDiffView}
        <ChangeMinibar diffBlocks={visibleDiffBlocks} {totalLines} onClickRegion={scrollToDiffRegion} />
      {/if}

      {#if showDiffView && visibleDiffBlocks.length > 0}
        <div class="diff-overlay" data-testid="history-diff-view" bind:this={diffOverlayElement}>
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <div class="diff-view">
              {#each visibleDiffBlocks as block, index (getDiffBlockTargetId(block, index))}
                <div
                  class="diff-block diff-{block.type}"
                  data-diff-target={getDiffBlockTargetId(block, index)}
                >
                  {@html textToEditorHtml(block.content)}
                </div>
              {/each}
            </div>
          </div>
        </div>
      {:else if showDiffView && visibleDiffBlocks.length === 0}
        <div class="diff-overlay" data-testid="history-diff-identical">
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <div class="diff-identical">
              <p>this version matches the current live note</p>
            </div>
          </div>
        </div>
      {:else if showSnapshotView}
        <div class="diff-overlay" data-testid="history-snapshot-view">
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <div class="snapshot-view">{@html textToEditorHtml(versionReviewState.previewText ?? '')}</div>
          </div>
        </div>
      {:else if isHistoryReview && versionReviewState.previewError}
        <div class="diff-overlay" data-testid="history-error-view">
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <p class="editor-error">{versionReviewState.previewError}</p>
            <p class="history-loading">return to live or pick another version.</p>
          </div>
        </div>
      {/if}

      <!-- Editor (always mounted, never moves) -->
      <div class="editor-content-wrap" class:editor-hidden={showHistoryOverlay}>
        <h1 class="doc-title" data-testid="editor-doc-title">{activeDoc.title}</h1>
        {#if editorSessionState.lastError && !isHistoryReview}
          <p class="editor-error">{editorSessionState.lastError}</p>
        {/if}
        {#if isHistoryReview && versionReviewState.previewLoading}
          <p class="history-loading">loading version...</p>
        {/if}
        <div class="editor-mount" data-testid="editor-mount" bind:this={editorElement}></div>
      </div>
    </div>

    <div class="bottom-bar">
      {#if !isHistoryReview}
        <span class="md-hints">**bold  _italic_  # heading  - list  - [ ] task  > quote  `code`</span>
      {/if}
      <span class="connection-status" data-testid="connection-status" data-state={syncState.connection} class:connected={syncState.connection === 'connected'} class:slow={syncState.connection === 'slow'} class:offline={syncState.connection === 'offline'} class:local={syncState.connection === 'local'}>{connectionLabel}</span>
    </div>
  {:else}
    <div class="empty-state" data-testid="editor-empty-state">
      <p class="empty-title">no document selected</p>
      <p class="empty-hint">pick a note from the sidebar, or create a new one</p>
    </div>
  {/if}
</div>

<style>
  .editor-pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    position: relative;
  }

  .editor-drag {
    height: 44px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    padding: 0 20px;
    -webkit-app-region: drag;
  }

  .drag-spacer {
    flex: 1;
  }

  .drag-right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .peer-avatars {
    display: flex;
    align-items: center;
  }

  .avatar {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 9px;
    font-weight: 500;
    color: var(--white);
    margin-left: -3px;
    border: 1.5px solid var(--surface);
  }

  .avatar:first-child {
    margin-left: 0;
  }

  .editor-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 48px 100px;
    position: relative;
  }

  .editor-content-wrap {
    max-width: 660px;
    margin: 0 auto;
    min-height: 100%;
  }

  .editor-content-wrap.editor-hidden {
    visibility: hidden;
    pointer-events: none;
  }

  .diff-overlay {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 2;
    background: var(--surface);
    overflow-y: auto;
    padding: 0 48px 100px;
  }

  .doc-title {
    font-family: var(--font-body);
    font-size: 34px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text-primary);
    margin-bottom: 24px;
    line-height: 1.15;
  }

  .editor-error {
    margin-bottom: 12px;
    color: var(--danger-fg);
    font-size: 13px;
  }

  .history-loading {
    margin-bottom: 12px;
    color: var(--text-tertiary);
    font-size: 13px;
    font-style: italic;
  }

  .editor-mount :global(.editor-content) {
    outline: none;
    font-family: var(--font-body);
    font-size: 16px;
    line-height: 1.85;
    letter-spacing: 0.005em;
    color: var(--text-primary);
    min-height: 40vh;
  }

  .editor-mount :global(.editor-content p) {
    margin-bottom: 0.8em;
  }

  .editor-mount :global(.remote-selection) {
    background: color-mix(in srgb, var(--remote-selection-color) 18%, transparent);
    border-radius: 3px;
  }

  .editor-mount :global(.remote-caret) {
    position: relative;
    display: inline-block;
    width: 2px;
    height: 1.35em;
    margin-left: -1px;
    margin-right: -1px;
    vertical-align: text-bottom;
    background: var(--remote-caret-color);
    pointer-events: none;
  }

  .editor-mount :global(.remote-caret-label) {
    position: absolute;
    top: -1.55em;
    left: -1px;
    transform: translateX(-8%);
    background: var(--remote-caret-color);
    color: white;
    border-radius: 999px;
    padding: 2px 6px;
    font-size: 10px;
    line-height: 1.2;
    font-weight: 600;
    white-space: nowrap;
    box-shadow: 0 4px 12px rgb(15 23 42 / 0.18);
  }

  /* ── Diff View ── */

  .diff-view {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-family: var(--font-body);
    font-size: 16px;
    line-height: 1.85;
    letter-spacing: 0.005em;
    color: var(--text-primary);
    min-height: 40vh;
  }

  .snapshot-view {
    font-family: var(--font-body);
    font-size: 16px;
    line-height: 1.85;
    letter-spacing: 0.005em;
    color: var(--text-primary);
    min-height: 40vh;
  }

  .snapshot-view :global(p) {
    margin-bottom: 0.8em;
  }

  .diff-block {
    padding: 2px 12px;
    border-radius: 6px;
    border-left: 3px solid transparent;
  }

  .diff-block :global(p) { margin-bottom: 0.8em; }
  .diff-block :global(p:last-child) { margin-bottom: 0; }
  .diff-block :global(h1), .diff-block :global(h2), .diff-block :global(h3) {
    font-weight: 600;
    line-height: 1.3;
    margin-bottom: 0.5em;
  }
  .diff-block :global(h1) { font-size: 1.5em; }
  .diff-block :global(h2) { font-size: 1.25em; }
  .diff-block :global(h3) { font-size: 1.1em; }
  .diff-block :global(code) {
    font-family: var(--font-mono);
    font-size: 0.9em;
    background: var(--surface-active);
    padding: 1px 4px;
    border-radius: 3px;
  }
  .diff-block :global(strong) { font-weight: 600; }
  .diff-block :global(em) { font-style: italic; }

  .diff-unchanged { opacity: 1; }

  .diff-added {
    background: var(--diff-added-bg);
    border-left-color: var(--diff-added);
  }

  .diff-removed {
    background: var(--diff-removed-bg);
    border-left-color: var(--diff-removed);
    text-decoration: line-through;
    text-decoration-color: var(--diff-removed-decoration);
    color: var(--diff-removed-text);
  }

  .diff-changed {
    background: var(--diff-changed-bg);
    border-left-color: var(--diff-changed);
  }

  .diff-identical {
    padding: 16px 12px;
    text-align: center;
    font-size: 13px;
    font-style: italic;
    color: var(--diff-neutral-text);
  }

  /* ── Bottom Bar ── */

  .bottom-bar {
    height: 36px;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 24px;
    color: var(--text-tertiary);
    font-size: 12px;
  }

  .md-hints {
    font-family: var(--font-mono);
    font-size: 11px;
    letter-spacing: 0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .connection-status {
    position: absolute;
    right: 24px;
    font-size: 11px;
    white-space: nowrap;
  }

  .connection-status.connected {
    color: var(--accent);
  }

  .connection-status.slow {
    color: var(--accent);
  }

  .connection-status.offline {
    color: var(--text-tertiary);
  }

  .connection-status.local {
    color: var(--text-tertiary);
  }

  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: var(--text-tertiary);
  }

  .empty-title {
    color: var(--text-primary);
    font-size: 22px;
    font-weight: 600;
  }

</style>
