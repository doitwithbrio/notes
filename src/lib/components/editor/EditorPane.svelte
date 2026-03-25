<script lang="ts">
  import type { Editor } from '@tiptap/core';
  import { createEditor, editorToPlainText, textToEditorHtml } from '../../editor/setup.js';
  import { getActiveDoc } from '../../state/documents.svelte.js';
  import { presenceState } from '../../state/presence.svelte.js';
  import { syncState } from '../../state/sync.svelte.js';
  import { editorSessionState, updateEditorText } from '../../session/editor-session.svelte.js';

  let editorElement = $state<HTMLDivElement | null>(null);
  let editor = $state<Editor | null>(null);
  let applyingRemoteText = false;

  const activeDoc = $derived(getActiveDoc());
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
        : 'offline',
  );

  function syncEditorContent(text: string) {
    if (!editor) return;
    const current = editorToPlainText(editor);
    if (current === text) return;

    applyingRemoteText = true;
    editor.commands.setContent(textToEditorHtml(text), { emitUpdate: false });
    applyingRemoteText = false;
  }

  $effect(() => {
    const el = editorElement;
    if (!el) return;

    const ed = createEditor(el, editorSessionState.text, (text) => {
      if (applyingRemoteText) return;
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
    syncEditorContent(editorSessionState.text);
  });
</script>

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

  {#if activeDoc}
    <div class="editor-scroll">
      <div class="editor-content-wrap">
        <h1 class="doc-title">{activeDoc.title}</h1>
        {#if editorSessionState.lastError}
          <p class="editor-error">{editorSessionState.lastError}</p>
        {/if}
        <div class="editor-mount" bind:this={editorElement}></div>
      </div>
    </div>

    <div class="bottom-bar">
      <span class="md-hints">**bold  _italic_  # heading  - list  [] task  > quote  `code`</span>
      <span class="connection-status" class:connected={syncState.connection === 'connected'} class:slow={syncState.connection === 'slow'} class:offline={syncState.connection === 'offline'}>{connectionLabel}</span>
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
  }

  .editor-content-wrap {
    max-width: 660px;
    margin: 0 auto;
    min-height: 100%;
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
    color: #a04130;
    font-size: 13px;
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
