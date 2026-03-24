<script lang="ts">
  import { onMount } from 'svelte';
  import type { Editor } from '@tiptap/core';
  import { createEditor } from '../../editor/setup.js';
  import { getActiveDoc, documentState } from '../../state/documents.svelte.js';
  import EditorHeader from './EditorHeader.svelte';
  import Toolbar from './Toolbar.svelte';

  let editorElement = $state<HTMLDivElement | null>(null);
  let editor = $state<Editor | null>(null);

  const activeDoc = $derived(getActiveDoc());

  onMount(() => {
    if (!editorElement) return;
    editor = createEditor(editorElement);

    // Update word count on content change
    editor.on('update', ({ editor: e }) => {
      const doc = getActiveDoc();
      if (doc) {
        const text = e.state.doc.textContent;
        const words = text.trim() ? text.trim().split(/\s+/).length : 0;
        const docIndex = documentState.docs.findIndex((d) => d.id === doc.id);
        if (docIndex >= 0) {
          documentState.docs[docIndex]!.wordCount = words;
        }
      }
    });

    return () => {
      editor?.destroy();
    };
  });
</script>

<div class="editor-pane">
  {#if activeDoc}
    <EditorHeader />
    <Toolbar {editor} />
    <div class="editor-scroll">
      <div class="editor-mount" bind:this={editorElement}></div>
    </div>
  {:else}
    <div class="empty-state">
      <p class="empty-title">No document selected</p>
      <p class="empty-hint">Pick a file from the sidebar or create a new note.</p>
    </div>
  {/if}
</div>

<style>
  .editor-pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--white);
  }

  .editor-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 16px 24px 48px;
  }

  .editor-mount {
    max-width: 720px;
    margin: 0 auto;
    min-height: 100%;
  }

  /* TipTap editor content styling */
  .editor-mount :global(.editor-content) {
    outline: none;
    font-family: var(--font-body);
    font-size: 15px;
    line-height: 1.7;
    color: var(--black);
  }

  .editor-mount :global(.editor-content h1),
  .editor-mount :global(.editor-content h2),
  .editor-mount :global(.editor-content h3),
  .editor-mount :global(.editor-content h4),
  .editor-mount :global(.editor-content h5),
  .editor-mount :global(.editor-content h6) {
    font-family: var(--font-display);
    font-weight: 300;
    color: var(--black);
    margin-top: 1.5em;
    margin-bottom: 0.5em;
  }

  .editor-mount :global(.editor-content h1) { font-size: 32px; }
  .editor-mount :global(.editor-content h2) { font-size: 26px; }
  .editor-mount :global(.editor-content h3) { font-size: 22px; }

  .editor-mount :global(.editor-content p) {
    margin-bottom: 0.75em;
  }

  .editor-mount :global(.editor-content a) {
    color: var(--accent);
    text-decoration: underline;
  }

  .editor-mount :global(.editor-content code) {
    font-family: var(--font-mono);
    font-size: 13px;
    background: var(--black);
    color: var(--white);
    padding: 2px 5px;
  }

  .editor-mount :global(.editor-content pre) {
    font-family: var(--font-mono);
    font-size: 13px;
    background: var(--black);
    color: var(--white);
    padding: 16px;
    margin: 1em 0;
    overflow-x: auto;
  }

  .editor-mount :global(.editor-content pre code) {
    background: none;
    padding: 0;
  }

  .editor-mount :global(.editor-content blockquote) {
    border-left: 3px solid var(--accent);
    padding-left: 16px;
    margin: 1em 0;
  }

  .editor-mount :global(.editor-content hr) {
    border: none;
    border-top: var(--border);
    margin: 2em 0;
  }

  .editor-mount :global(.editor-content ul),
  .editor-mount :global(.editor-content ol) {
    padding-left: 24px;
    margin-bottom: 0.75em;
  }

  .editor-mount :global(.editor-content ul[data-type='taskList']) {
    list-style: none;
    padding-left: 0;
  }

  .editor-mount :global(.editor-content ul[data-type='taskList'] li) {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .editor-mount :global(.editor-content ul[data-type='taskList'] input[type='checkbox']) {
    margin-top: 4px;
    accent-color: var(--accent);
  }

  .editor-mount :global(.editor-content img) {
    max-width: 100%;
    height: auto;
    margin: 1em 0;
  }

  .editor-mount :global(.editor-content table) {
    border-collapse: collapse;
    width: 100%;
    margin: 1em 0;
  }

  .editor-mount :global(.editor-content th),
  .editor-mount :global(.editor-content td) {
    border: var(--border);
    padding: 8px 12px;
    text-align: left;
  }

  .editor-mount :global(.editor-content th) {
    font-weight: 600;
    background: var(--black);
    color: var(--white);
  }

  .editor-mount :global(.editor-content .is-empty::before) {
    content: attr(data-placeholder);
    color: var(--black);
    pointer-events: none;
    float: left;
    height: 0;
  }

  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .empty-title {
    font-family: var(--font-display);
    font-size: 24px;
    color: var(--black);
  }

  .empty-hint {
    font-size: 13px;
    color: var(--black);
  }
</style>
