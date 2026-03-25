<script lang="ts">
  import type { Project, Document } from '../../types/index.js';
  import { uiState } from '../../state/ui.svelte.js';
  import FileItem from './FileItem.svelte';
  import { sortable } from '../../actions/sortable.js';
  import { FilePlus, ChevronRight } from 'lucide-svelte';

  let {
    project,
    docs,
    loading = false,
    hydrated = false,
    collapsed: sidebarCollapsed = false,
    editing = false,
    editingDocId = null,
    editMode = false,
    oncommit,
    oncancel,
    onnewnote,
    onprojectclick,
    ondoccommit,
    ondoccancel,
    ondocopen,
    onreorderdocs,
    ondoccontextmenu,
    onprojectcontextmenu,
  }: {
    project: Project;
    docs: Document[];
    loading?: boolean;
    hydrated?: boolean;
    collapsed?: boolean;
    editing?: boolean;
    editingDocId?: string | null;
    editMode?: boolean;
    oncommit?: (name: string) => void;
    oncancel?: () => void;
    onnewnote?: () => void;
    onprojectclick?: () => void;
    ondoccommit?: (title: string) => void;
    ondoccancel?: () => void;
    ondocopen?: (docId: string) => void;
    onreorderdocs?: (detail: { fromIndex: number; toIndex: number }) => void;
    ondoccontextmenu?: (detail: { x: number; y: number; docId: string }) => void;
    onprojectcontextmenu?: (detail: { x: number; y: number }) => void;
  } = $props();

  let folded = $state(false);

  let inputValue = $state('');

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
    requestAnimationFrame(() => el.focus());
  }

  function handleAddNote(e: MouseEvent) {
    e.stopPropagation();
    onnewnote?.();
  }

  function handleProjectClick(e: MouseEvent) {
    e.stopPropagation();
    onprojectclick?.();
  }

  function handleFoldToggle(e: MouseEvent) {
    e.stopPropagation();
    folded = !folded;
  }

  const isActiveProject = $derived(uiState.activeProjectId === project.id && uiState.view === 'project-overview');
</script>

<div class="project-group">
  {#if !sidebarCollapsed}
    {#if editing}
      <div class="project-header editing">
        <input
          class="inline-input"
          type="text"
          placeholder="project name"
          bind:value={inputValue}
          onkeydown={handleInputKeydown}
          onblur={handleInputBlur}
          use:focusInput
        />
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="project-header"
        class:drag-mode={editMode}
        data-drag-handle={editMode || undefined}
        oncontextmenu={(e) => {
          if (onprojectcontextmenu) {
            e.preventDefault();
            onprojectcontextmenu({ x: e.clientX, y: e.clientY });
          }
        }}
      >
        <button
          class="project-name-btn"
          class:active={isActiveProject}
          onclick={handleProjectClick}
          disabled={editMode}
        >
          {project.name}
        </button>
        {#if !editMode}
          <button class="add-note-btn" onclick={handleAddNote} aria-label="new note in {project.name}">
            <FilePlus size={13} strokeWidth={1.5} />
          </button>
        {/if}
        <button
          class="fold-btn"
          class:unfolded={!folded}
          onclick={handleFoldToggle}
          aria-label={folded ? 'expand files' : 'collapse files'}
        >
          <ChevronRight size={12} strokeWidth={1.5} />
        </button>
      </div>
    {/if}
  {/if}

  {#if !folded}
    <div
      class="file-list"
      use:sortable={{
        onReorder: (detail) => onreorderdocs?.(detail),
        enabled: !sidebarCollapsed && editMode && !!onreorderdocs,
      }}
    >
      {#if loading && !sidebarCollapsed}
        <p class="project-meta">loading notes...</p>
      {:else if hydrated && docs.length === 0 && !sidebarCollapsed}
        <p class="project-meta">no notes yet</p>
      {/if}
      {#each docs as doc (doc.id)}
        <FileItem
          {doc}
          collapsed={sidebarCollapsed}
          editing={editingDocId === doc.id}
          {editMode}
          onopen={ondocopen}
          oncommit={ondoccommit}
          oncancel={ondoccancel}
          oncontextmenu={ondoccontextmenu}
        />
      {/each}
    </div>
  {/if}
</div>

<style>
  .project-group {
    margin-bottom: 0;
  }

  .project-header {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 8px 6px 4px 10px;
    font-size: 13px;
    font-weight: 700;
    text-align: left;
    letter-spacing: -0.01em;
    color: var(--text-primary);
    border-radius: 6px;
    transition: color var(--transition-fast);
  }

  .project-header.editing {
    padding: 6px 10px 4px;
    cursor: default;
  }

  .project-header.drag-mode {
    cursor: grab;
    user-select: none;
  }

  .project-header.drag-mode:active {
    cursor: grabbing;
  }

  .project-name-btn {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    font-weight: 700;
    font-size: 13px;
    letter-spacing: -0.01em;
    color: var(--text-primary);
    padding: 0;
    border-radius: 4px;
    transition: color var(--transition-fast);
  }

  .project-name-btn:hover {
    color: var(--accent);
  }

  .project-name-btn.active {
    color: var(--accent);
  }

  .project-name-btn:disabled {
    color: var(--text-primary);
    cursor: grab;
  }

  .add-note-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-primary);
    opacity: 0;
    transition: opacity var(--transition-fast), color var(--transition-fast);
    flex-shrink: 0;
    padding: 2px;
    border-radius: 4px;
  }

  .project-header:hover .add-note-btn {
    opacity: 1;
  }

  .add-note-btn:hover {
    color: var(--text-primary);
  }

  .fold-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    color: var(--text-primary);
    border-radius: 4px;
    transition: color var(--transition-fast), transform var(--transition-fast);
  }

  .fold-btn:hover {
    color: var(--text-primary);
  }

  .fold-btn.unfolded {
    transform: rotate(90deg);
  }

  .file-list {
    display: flex;
    flex-direction: column;
  }

  .project-meta {
    padding: 2px 10px 6px 14px;
    font-size: 12px;
    color: var(--text-tertiary);
  }

  /* Inline editing input */
  .inline-input {
    width: 100%;
    font-family: var(--font-body);
    font-size: 13px;
    font-weight: 700;
    letter-spacing: -0.01em;
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
    font-weight: 400;
  }
</style>
