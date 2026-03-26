<script lang="ts">
  import { untrack } from 'svelte';
  import type { Editor } from '@tiptap/core';
  import { createEditor, editorToPlainText, textToEditorHtml } from '../../editor/setup.js';
  import { getActiveDoc } from '../../state/documents.svelte.js';
  import { presenceState } from '../../state/presence.svelte.js';
  import { syncState } from '../../state/sync.svelte.js';
  import { editorSessionState, updateEditorText, reloadActiveSession } from '../../session/editor-session.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import { versionState, leaveHistoryReview, showSavePrompt } from '../../state/versions.svelte.js';
  import { computeBlockDiff } from '../../utils/diff.js';
  import TimelineScrubber from './TimelineScrubber.svelte';
  import SaveVersionBar from './SaveVersionBar.svelte';
  import ChangeMinibar from './ChangeMinibar.svelte';


  let editorElement = $state<HTMLDivElement | null>(null);
  let editor = $state<Editor | null>(null);
  let applyingRemoteText = false;

  const activeDoc = $derived(getActiveDoc());
  const isHistoryReview = $derived(uiState.view === 'history-review');
  const peersInDoc = $derived(
    activeDoc
      ? presenceState.peers.filter((peer) => activeDoc.activePeers.includes(peer.id) && peer.online)
      : [],
  );
  const connectionLabel = $derived(
    syncState.connection === 'connected'
      ? 'connected'
      : syncState.connection === 'slow'
        ? 'syncing'
        : syncState.connection === 'local'
          ? 'local'
          : 'offline',
  );

  // Compute diff blocks when in history review mode
  // Use untrack on editorSessionState.text to avoid reactive feedback loops —
  // the user can't type during history review, so the text is static.
  const diffBlocks = $derived.by(() => {
    if (isHistoryReview && versionState.previewText != null) {
      const currentText = untrack(() => editorSessionState.text);
      return computeBlockDiff(versionState.previewText, currentText);
    }
    return [];
  });

  // Total lines for minibar calculation
  const totalLines = $derived(
    editorSessionState.text.split('\n').length,
  );

  function syncEditorContent(text: string) {
    if (!editor || isHistoryReview) return; // Don't sync while reviewing history
    const current = editorToPlainText(editor);
    if (current === text) return;

    applyingRemoteText = true;
    editor.commands.setContent(textToEditorHtml(text), { emitUpdate: false });
    applyingRemoteText = false;
  }

  $effect(() => {
    const el = editorElement;
    if (!el) return;

    const initialText = untrack(() => editorSessionState.text);
    const ed = createEditor(el, initialText, (text) => {
      if (applyingRemoteText) return;
      if (untrack(() => isHistoryReview)) return; // Block edits in review mode
      updateEditorText(text);
    });
    editor = ed;

    return () => {
      ed.destroy();
      editor = null;
    };
  });

  $effect(() => {
    editorSessionState.revision;
    const text = editorSessionState.text;
    untrack(() => syncEditorContent(text));
  });

  $effect(() => {
    const reviewing = isHistoryReview;
    if (!reviewing) {
      const text = editorSessionState.text;
      untrack(() => syncEditorContent(text));
    }
  });

  // When entering/leaving review mode, toggle editor editability
  $effect(() => {
    const reviewing = isHistoryReview;
    const ed = untrack(() => editor);
    if (ed) {
      ed.setEditable(!reviewing);
    }
  });

  // When version preview text loads, show it in the editor
  $effect(() => {
    if (isHistoryReview && versionState.previewText != null) {
      const ed = untrack(() => editor);
      if (!ed) return;
      applyingRemoteText = true;
      ed.commands.setContent(textToEditorHtml(versionState.previewText), { emitUpdate: false });
      applyingRemoteText = false;
    }
  });

  // Exit version review when the active document changes
  let prevDocId: string | null = null;
  $effect(() => {
    const docId = editorSessionState.docId;
    if (prevDocId !== null && docId !== prevDocId) {
      if (untrack(() => isHistoryReview)) {
        leaveHistoryReview();
      }
    }
    prevDocId = docId;
  });

  // Cmd+S handler — create a named version
  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && isHistoryReview) {
      e.preventDefault();
      leaveHistoryReview();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
      e.preventDefault();
      if (!isHistoryReview && versionState.supported && editorSessionState.projectId && editorSessionState.docId) {
        showSavePrompt();
      }
    }
  }

  async function handleRestore() {
    // Reload the active session to reflect restored content
    await reloadActiveSession();
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="editor-pane">
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
      {#if isHistoryReview}
        <ChangeMinibar {diffBlocks} {totalLines} />
      {/if}

      <!-- Diff overlay (covers editor during version review) -->
      {#if isHistoryReview && diffBlocks.length > 0}
        <div class="diff-overlay">
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <div class="diff-view">
              {#each diffBlocks as block}
                <div class="diff-block diff-{block.type}">
                  {@html textToEditorHtml(block.content)}
                </div>
              {/each}
            </div>
          </div>
        </div>
      {:else if isHistoryReview && !versionState.previewLoading && !versionState.previewError && versionState.previewText != null}
        <div class="diff-overlay">
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <div class="diff-identical">
              <p>this version is identical to the current document</p>
            </div>
          </div>
        </div>
      {:else if isHistoryReview && versionState.previewLoading}
        <div class="diff-overlay">
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <p class="history-loading">loading version...</p>
          </div>
        </div>
      {:else if isHistoryReview && versionState.previewError}
        <div class="diff-overlay">
          <div class="editor-content-wrap">
            <h1 class="doc-title">{activeDoc.title}</h1>
            <p class="editor-error">{versionState.previewError}</p>
          </div>
        </div>
      {/if}

      <!-- Editor (always mounted, never moves) -->
      <div class="editor-content-wrap" class:editor-hidden={isHistoryReview}>
        <h1 class="doc-title">{activeDoc.title}</h1>
        {#if editorSessionState.lastError && !isHistoryReview}
          <p class="editor-error">{editorSessionState.lastError}</p>
        {/if}
        <div class="editor-mount" bind:this={editorElement}></div>
      </div>
    </div>

    <div class="bottom-bar">
      {#if isHistoryReview}
        <span class="history-hint">viewing history · read only</span>
      {:else}
        <span class="md-hints">**bold  _italic_  # heading  - list  [] task  > quote  `code`</span>
      {/if}
      <span class="connection-status" class:connected={syncState.connection === 'connected'} class:slow={syncState.connection === 'slow'} class:offline={syncState.connection === 'offline'} class:local={syncState.connection === 'local'}>{connectionLabel}</span>
    </div>
  {:else}
    <div class="empty-state">
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

  .history-hint {
    font-size: 11px;
    font-style: italic;
    color: var(--text-tertiary);
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
