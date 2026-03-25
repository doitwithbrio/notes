<script lang="ts">
  import type { Document } from '../../types/index.js';
  import { documentState } from '../../state/documents.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import { getPeerById } from '../../state/presence.svelte.js';

  let {
    doc,
    collapsed = false,
    editing = false,
    editMode = false,
    onopen,
    oncommit,
    oncancel,
    oncontextmenu: onCtxMenu,
  }: {
    doc: Document;
    collapsed?: boolean;
    editing?: boolean;
    editMode?: boolean;
    onopen?: (docId: string) => void;
    oncommit?: (title: string) => void;
    oncancel?: () => void;
    oncontextmenu?: (detail: { x: number; y: number; docId: string }) => void;
  } = $props();

  const isActive = $derived(documentState.activeDocId === doc.id && uiState.view === 'editor');

  const peersHere = $derived.by(() =>
    doc.activePeers
      .map((peerId) => getPeerById(peerId))
      .filter((peer): peer is NonNullable<typeof peer> => !!peer?.online)
      .slice(0, 3),
  );

  let inputValue = $state('');

  // Pre-populate input with current title when entering edit mode
  $effect(() => {
    if (editing) {
      inputValue = doc.title;
    }
  });

  function handleInputKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitEdit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      oncancel?.();
    }
  }

  function handleInputBlur() {
    commitEdit();
  }

  function commitEdit() {
    const trimmed = inputValue.trim();
    if (trimmed) {
      oncommit?.(trimmed);
    } else {
      oncancel?.();
    }
  }

  function focusInput(el: HTMLInputElement) {
    // Tick delay to ensure it's in the DOM
    requestAnimationFrame(() => el.focus());
  }
</script>

{#if editing}
  <div class="file-item editing">
    <input
      class="inline-input"
      type="text"
      placeholder="note title"
      bind:value={inputValue}
      onkeydown={handleInputKeydown}
      onblur={handleInputBlur}
      use:focusInput
    />
  </div>
{:else}
  <button
    class="file-item"
    class:active={isActive && !editMode}
    class:collapsed
    class:drag-mode={editMode}
    onclick={() => { if (!editMode) onopen?.(doc.id); }}
    oncontextmenu={(e) => {
      if (onCtxMenu) {
        e.preventDefault();
        onCtxMenu({ x: e.clientX, y: e.clientY, docId: doc.id });
      }
    }}
  >
    {#if collapsed}
      <span class="file-initial" class:has-unread={doc.hasUnread} class:has-peers={peersHere.length > 0}>
        {doc.title[0]?.toLowerCase() ?? '?'}
      </span>
    {:else}
      {#if doc.hasUnread}
        <span class="unread-dot"></span>
      {/if}
      <span class="file-name">{doc.title}</span>

      {#if peersHere.length > 0}
        <span class="peer-indicators">
          {#each peersHere.slice(0, 3) as peer (peer.id)}
            <span class="peer-dot" style="background: {peer.cursorColor}"></span>
          {/each}
        </span>
      {/if}
    {/if}
  </button>
{/if}

<style>
  .file-item {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 6px 10px;
    font-size: 13px;
    text-align: left;
    color: var(--text-primary);
    position: relative;
    border-radius: 8px;
    transition: background var(--transition-fast);
    gap: 8px;
  }

  .file-item.editing {
    padding: 4px 10px;
  }

  .file-item.collapsed {
    padding: 4px 8px;
    justify-content: center;
  }

  .file-item:hover:not(.drag-mode) {
    background: var(--surface-hover);
  }

  .file-item.active {
    background: var(--surface-active);
    font-weight: 500;
  }

  .file-item.drag-mode {
    cursor: grab;
    user-select: none;
  }

  .file-item.drag-mode:active {
    cursor: grabbing;
  }

  .file-initial {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-primary);
    position: relative;
  }

  .file-initial.has-unread::after {
    content: '';
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    position: absolute;
    top: -1px;
    right: 4px;
  }

  .file-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Unread: static accent dot to the left of file name */
  .unread-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
  }

  /* Peers: small colored dots on the right */
  .peer-indicators {
    display: flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
    margin-left: auto;
  }

  .peer-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
  }

  /* Inline editing input */
  .inline-input {
    width: 100%;
    font-family: var(--font-body);
    font-size: 13px;
    color: var(--text-primary);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    outline: none;
    padding: 9px 12px;
    transition: border-color var(--transition-fast);
  }

  .inline-input:focus {
    border-color: var(--accent);
  }

  .inline-input::placeholder {
    color: var(--text-secondary);
  }
</style>
