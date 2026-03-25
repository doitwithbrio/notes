<script lang="ts">
  import type { Project } from '../../types/index.js';
  import { documentState } from '../../state/documents.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import { todoState, addTodo, toggleTodo, removeTodo } from '../../state/todos.svelte.js';
  import { presenceState } from '../../state/presence.svelte.js';
  import { syncState } from '../../state/sync.svelte.js';
  import { openEditorSession } from '../../session/editor-session.svelte.js';
  import { FileText, X, Users } from 'lucide-svelte';

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

  function openDoc(docId: string) {
    void openEditorSession(project.id, docId);
  }

  function getLinkedDocTitle(docId: string | undefined): string | null {
    if (!docId) return null;
    return documentState.docs.find((d) => d.id === docId)?.title ?? null;
  }

  function syncIcon(status: string): string {
    if (status === 'synced') return '✓';
    if (status === 'syncing') return '↻';
    return '—';
  }
</script>

<div class="overview-pane">
  <!-- Drag region (matches editor height) -->
  <div class="overview-drag" data-tauri-drag-region>
    <div class="drag-spacer" data-tauri-drag-region></div>
  </div>

  <div class="overview-scroll">
    <div class="overview-content">
      <!-- Project header -->
      <div class="project-header">
        <h1 class="project-title">{project.name}</h1>
        {#if project.shared}
          <div class="shared-badge">
            <Users size={12} strokeWidth={1.5} />
            <span>shared · {onlinePeerCount} online</span>
          </div>
        {/if}
        <p class="role-line">you are the {project.role}</p>
      </div>

      <!-- Todos section -->
      <div class="section">
        <div class="section-header">
          <span class="section-title">todos</span>
          {#if pendingTodos.length > 0}
            <span class="section-count">{pendingTodos.length}</span>
          {/if}
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
            <p class="empty-hint">no todos yet — add one below</p>
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
      </div>

      <!-- Files section -->
      <div class="section">
        <div class="section-header">
          <span class="section-title">files</span>
          <span class="section-count">{projectDocs.length}</span>
        </div>

        <div class="file-list">
          {#each projectDocs as doc (doc.id)}
            <button class="file-row" onclick={() => openDoc(doc.id)}>
              <FileText size={14} strokeWidth={1.5} />
              <span class="file-name">{doc.title}</span>
              <span class="file-meta">
                {#if doc.wordCount > 0}
                  <span class="word-count">{doc.wordCount}w</span>
                {/if}
                <span class="sync-icon" class:synced={doc.syncStatus === 'synced'} class:syncing={doc.syncStatus === 'syncing'}>
                  {syncIcon(doc.syncStatus)}
                </span>
              </span>
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
            </button>
          {/each}

          {#if projectDocs.length === 0}
            <p class="empty-hint">no files yet</p>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>

<style>
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
    padding: 0 48px 100px;
  }

  .overview-content {
    max-width: 660px;
    margin: 0 auto;
  }

  /* Project header */
  .project-header {
    margin-bottom: 48px;
  }

  .project-title {
    font-family: var(--font-body);
    font-size: 34px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text-primary);
    line-height: 1.15;
    margin-bottom: 10px;
  }

  .shared-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 10px;
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border-radius: 20px;
    margin-bottom: 8px;
  }

  .role-line {
    font-size: 13px;
    color: var(--text-tertiary);
    font-weight: 400;
  }

  /* Sections */
  .section {
    margin-bottom: 40px;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 14px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .section-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-tertiary);
  }

  .section-count {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
    background: var(--surface-active);
    padding: 0 6px;
    border-radius: 8px;
    line-height: 18px;
  }

  /* Todos */
  .todo-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 12px;
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

  .todo-item.done {
    opacity: 0.4;
  }

  .todo-check {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    padding-top: 2px;
  }

  .todo-check input[type='checkbox'] {
    width: 14px;
    height: 14px;
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

  .todo-item.done .todo-text {
    text-decoration: line-through;
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

  .todo-input-wrap {
    padding: 0 8px;
  }

  .todo-input {
    width: 100%;
    padding: 8px 0;
    font-size: 14px;
    color: var(--text-primary);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    outline: none;
    transition: border-color var(--transition-fast);
  }

  .todo-input:focus {
    border-color: var(--accent);
  }

  .todo-input::placeholder {
    color: var(--text-tertiary);
  }

  /* Files */
  .file-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .file-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    text-align: left;
    transition: background var(--transition-fast);
    width: 100%;
  }

  .file-row:hover {
    background: var(--surface-hover);
  }

  .file-row :global(svg) {
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  .file-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 450;
  }

  .file-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .word-count {
    font-variant-numeric: tabular-nums;
  }

  .sync-icon {
    font-size: 11px;
  }

  .sync-icon.synced {
    color: var(--accent);
  }

  .sync-icon.syncing {
    color: var(--text-tertiary);
  }

  .active-peer-dots {
    display: flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
  }

  .file-peer-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .empty-hint {
    font-size: 13px;
    color: var(--text-tertiary);
    padding: 8px;
  }
</style>
