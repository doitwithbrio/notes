<script lang="ts">
  import type { Project } from '../../types/index.js';
  import { documentState } from '../../state/documents.svelte.js';
  import { todoState, addTodo, toggleTodo, removeTodo } from '../../state/todos.svelte.js';
  import { presenceState } from '../../state/presence.svelte.js';
  import { openShareDialog } from '../../state/invite.svelte.js';
  import { FileText, X, Share2 } from 'lucide-svelte';
  import { navigateToDoc } from '../../navigation/workspace-router.svelte.js';

  let { project }: { project: Project } = $props();

  const projectDocs = $derived(
    documentState.docs.filter((d) => d.projectId === project.id),
  );

  const todos = $derived(
    todoState.todos.filter((t) => t.projectId === project.id),
  );

  const pendingTodos = $derived(todos.filter((t) => !t.done));
  const doneTodos = $derived(todos.filter((t) => t.done));

  const onlinePeerCount = $derived(
    presenceState.peers.filter((p) => p.online).length,
  );

  let newTodoText = $state('');

  function handleTodoKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submitTodo();
    }
  }

  function submitTodo() {
    const text = newTodoText.trim();
    if (!text) return;
    addTodo(project.id, text);
    newTodoText = '';
  }

  async function openDoc(docId: string) {
    try {
      await navigateToDoc(project.id, docId);
    } catch (error) {
      console.error('Failed to open note from project overview:', error);
    }
  }

  function getLinkedDocTitle(docId: string | undefined): string | null {
    if (!docId) return null;
    return documentState.docs.find((d) => d.id === docId)?.title ?? null;
  }

  const metaLine = $derived(() => {
    const parts: string[] = [project.role];
    if (project.shared && onlinePeerCount > 0) {
      parts.push(`${onlinePeerCount} peer${onlinePeerCount > 1 ? 's' : ''} online`);
    }
    if (projectDocs.length > 0) {
      parts.push(`${projectDocs.length} note${projectDocs.length > 1 ? 's' : ''}`);
    }
    return parts.join(' · ');
  });
</script>

<div class="overview-pane">
  <div class="overview-drag" data-tauri-drag-region>
    <div class="drag-spacer" data-tauri-drag-region></div>
  </div>

  <div class="overview-scroll">
    <div class="overview-content">
      <!-- Project header -->
      <div class="project-header">
        <div class="project-title-row">
          <h1 class="project-title">{project.name}</h1>
          {#if project.role === 'owner'}
            <button class="share-btn" onclick={() => openShareDialog(project.id)}>
              <Share2 size={13} strokeWidth={1.5} />
              share
            </button>
          {/if}
        </div>
        <p class="project-meta">{metaLine()}</p>
      </div>

      <!-- Todos section -->
      <div class="section">
        <div class="section-header">
          <span class="section-title">todos</span>
          {#if pendingTodos.length > 0}
            <span class="section-count">{pendingTodos.length}</span>
          {/if}
        </div>

        <div class="todo-input-wrap">
          <input
            class="todo-input"
            type="text"
            placeholder="add a todo..."
            bind:value={newTodoText}
            onkeydown={handleTodoKeydown}
          />
        </div>

        <div class="todo-list">
          {#each pendingTodos as todo (todo.id)}
            <div class="todo-item">
              <label class="todo-check">
                <input
                  type="checkbox"
                  checked={todo.done}
                  onchange={() => toggleTodo(todo.id)}
                />
              </label>
              <div class="todo-body">
                <span class="todo-text">{todo.text}</span>
                {#if todo.linkedDocId}
                  {@const title = getLinkedDocTitle(todo.linkedDocId)}
                  {#if title}
                    <span class="todo-link">
                      <FileText size={10} strokeWidth={1.5} />
                      {title}
                    </span>
                  {/if}
                {/if}
              </div>
              <button class="todo-remove" onclick={() => removeTodo(todo.id)} aria-label="remove">
                <X size={12} strokeWidth={1.5} />
              </button>
            </div>
          {/each}

          {#each doneTodos as todo (todo.id)}
            <div class="todo-item done">
              <label class="todo-check">
                <input
                  type="checkbox"
                  checked={todo.done}
                  onchange={() => toggleTodo(todo.id)}
                />
              </label>
              <div class="todo-body">
                <span class="todo-text">{todo.text}</span>
              </div>
              <button class="todo-remove" onclick={() => removeTodo(todo.id)} aria-label="remove">
                <X size={12} strokeWidth={1.5} />
              </button>
            </div>
          {/each}

          {#if todos.length === 0}
            <p class="empty-hint">no todos yet</p>
          {/if}
        </div>
      </div>

      <!-- Files section -->
      <div class="section">
        <div class="section-header">
          <span class="section-title">notes</span>
          {#if projectDocs.length > 0}
            <span class="section-count">{projectDocs.length}</span>
          {/if}
        </div>

        <div class="file-list">
          {#each projectDocs as doc (doc.id)}
            <button class="file-row" onclick={() => openDoc(doc.id)}>
              <FileText size={14} strokeWidth={1.5} class="file-icon" />
              <span class="file-info">
                <span class="file-name">{doc.title}</span>
                {#if doc.wordCount > 0}
                  <span class="file-meta">{doc.wordCount} words</span>
                {/if}
              </span>
              <span class="file-trailing">
                {#if doc.activePeers.length > 0}
                  <span class="active-peer-dots">
                    {#each doc.activePeers.slice(0, 3) as peerId (peerId)}
                      {@const peer = presenceState.peers.find((p) => p.id === peerId)}
                      {#if peer}
                        <span class="file-peer-dot" style="background: {peer.cursorColor}"></span>
                      {/if}
                    {/each}
                  </span>
                {/if}
                <span
                  class="sync-dot"
                  class:synced={doc.syncStatus === 'synced'}
                  class:syncing={doc.syncStatus === 'syncing'}
                  class:local-only={doc.syncStatus === 'local-only'}
                ></span>
              </span>
            </button>
          {/each}

          {#if projectDocs.length === 0}
            <p class="empty-hint">no notes yet</p>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  /* ── Layout ── */

  .overview-pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .overview-drag {
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

  .overview-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 48px 80px;
  }

  .overview-content {
    max-width: 640px;
    margin: 0 auto;
  }

  /* ── Project Header ── */

  .project-header {
    margin-bottom: 40px;
  }

  .project-title-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .share-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    padding: 5px 12px;
    border-radius: 8px;
    border: 1px solid var(--border-subtle);
    transition: background var(--transition-fast);
    flex-shrink: 0;
    white-space: nowrap;
  }

  .share-btn:hover {
    background: var(--surface-hover);
  }

  .project-title {
    font-family: var(--font-body);
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--text-primary);
    line-height: 1.25;
    margin-bottom: 6px;
  }

  .project-meta {
    font-size: 13px;
    color: var(--text-tertiary);
    font-weight: 400;
    line-height: 1.5;
  }

  /* ── Sections ── */

  .section {
    margin-bottom: 32px;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 12px;
  }

  .section-title {
    font-size: 13px;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--text-primary);
  }

  .section-count {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  /* ── Todos ── */

  .todo-input-wrap {
    margin-bottom: 12px;
  }

  .todo-input {
    width: 100%;
    padding: 9px 12px;
    font-family: var(--font-body);
    font-size: 14px;
    color: var(--text-primary);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    outline: none;
    transition: border-color var(--transition-fast);
  }

  .todo-input:focus {
    border-color: var(--accent);
  }

  .todo-input::placeholder {
    color: var(--text-secondary);
  }

  .todo-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .todo-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 6px 8px;
    border-radius: 8px;
    transition: background var(--transition-fast);
  }

  .todo-item:hover {
    background: var(--surface-hover);
  }

  .todo-item.done .todo-text {
    text-decoration: line-through;
    text-decoration-color: var(--text-tertiary);
    color: var(--text-tertiary);
  }

  .todo-check {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    padding-top: 2px;
  }

  .todo-check input[type='checkbox'] {
    width: 15px;
    height: 15px;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .todo-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .todo-text {
    font-size: 14px;
    color: var(--text-primary);
    line-height: 1.5;
    word-break: break-word;
  }

  .todo-link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11px;
    color: var(--text-tertiary);
    font-weight: 450;
  }

  .todo-remove {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    color: var(--text-tertiary);
    border-radius: 4px;
    opacity: 0;
    transition: opacity var(--transition-fast), color var(--transition-fast);
  }

  .todo-item:hover .todo-remove {
    opacity: 1;
  }

  .todo-remove:hover {
    color: var(--text-primary);
  }

  /* ── Files ── */

  .file-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .file-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: 8px;
    text-align: left;
    width: 100%;
    transition: background var(--transition-fast);
    cursor: pointer;
  }

  .file-row:hover {
    background: var(--surface-hover);
  }

  .file-row:hover .file-name {
    color: var(--accent);
  }

  .file-row :global(svg) {
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  .file-row:hover :global(svg) {
    color: var(--accent);
  }

  .file-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .file-name {
    font-size: 13px;
    font-weight: 450;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color var(--transition-fast);
  }

  .file-meta {
    font-size: 12px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }

  .file-trailing {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .active-peer-dots {
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .file-peer-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
  }

  .sync-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .sync-dot.synced {
    background: var(--accent);
  }

  .sync-dot.syncing {
    background: transparent;
    border: 1.5px solid var(--accent);
  }

  .sync-dot.local-only {
    background: var(--text-tertiary);
  }

  /* ── Empty states ── */

  .empty-hint {
    font-size: 13px;
    color: var(--text-tertiary);
    padding: 4px 0;
  }
</style>
