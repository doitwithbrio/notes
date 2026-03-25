<script lang="ts">
  import { uiState } from '../../state/ui.svelte.js';
  import { documentState, getActiveDoc } from '../../state/documents.svelte.js';
  import { todoState, addTodo, toggleTodo, removeTodo } from '../../state/todos.svelte.js';
  import { FileText, X } from 'lucide-svelte';

  // Derive context
  const activeDoc = $derived(getActiveDoc());

  const projectId = $derived(
    uiState.activeProjectId
      ?? activeDoc?.projectId
      ?? null,
  );

  // In editor view with an active doc: show file-specific todos only
  // In project overview: show all project todos
  const isFileMode = $derived(
    uiState.view === 'editor' && !!activeDoc,
  );

  const todos = $derived.by(() => {
    if (!projectId) return [];
    const all = todoState.todos.filter((t) => t.projectId === projectId);
    if (isFileMode && activeDoc) {
      return all.filter((t) => t.linkedDocId === activeDoc.id);
    }
    return all;
  });

  const pendingTodos = $derived(todos.filter((t) => !t.done));
  const doneTodos = $derived(todos.filter((t) => t.done));
  const pendingCount = $derived(pendingTodos.length);

  // Section header label
  const headerLabel = $derived(
    isFileMode && activeDoc
      ? `todos · ${activeDoc.title}`
      : 'todos',
  );

  let newTodoText = $state('');

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submitTodo();
    }
  }

  function submitTodo() {
    const text = newTodoText.trim();
    if (!text || !projectId) return;
    // Auto-link to the active doc when in file mode
    const linkedDocId = isFileMode && activeDoc ? activeDoc.id : undefined;
    addTodo(projectId, text, linkedDocId);
    newTodoText = '';
  }

  function getLinkedDocTitle(docId: string | undefined): string | null {
    if (!docId) return null;
    return documentState.docs.find((d) => d.id === docId)?.title ?? null;
  }
</script>

<section class="section">
  <div class="section-label">
    <span class="section-rule"></span>
    <span class="section-name">{headerLabel}</span>
    {#if pendingCount > 0}
      <span class="section-count">{pendingCount}</span>
    {/if}
  </div>

  {#if !projectId}
    <p class="empty-text">select a project or note</p>
  {:else}
    <div class="todo-input-wrap">
      <input
        class="todo-input"
        type="text"
        placeholder={isFileMode ? 'add a todo for this file...' : 'add a todo...'}
        bind:value={newTodoText}
        onkeydown={handleKeydown}
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
          <div class="todo-content">
            <span class="todo-text">{todo.text}</span>
            {#if todo.linkedDocId && !isFileMode}
              {@const title = getLinkedDocTitle(todo.linkedDocId)}
              {#if title}
                <span class="todo-link">
                  <FileText size={10} strokeWidth={1.5} />
                  {title}
                </span>
              {/if}
            {/if}
          </div>
          <button class="todo-remove" onclick={() => removeTodo(todo.id)} aria-label="remove todo">
            <X size={11} strokeWidth={1.5} />
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
          <div class="todo-content">
            <span class="todo-text">{todo.text}</span>
          </div>
          <button class="todo-remove" onclick={() => removeTodo(todo.id)} aria-label="remove todo">
            <X size={11} strokeWidth={1.5} />
          </button>
        </div>
      {/each}

      {#if todos.length === 0}
        <p class="empty-text">{isFileMode ? 'no todos for this file' : 'no todos yet'}</p>
      {/if}
    </div>
  {/if}
</section>

<style>
  .section {
    padding: 16px;
  }

  .section-label {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }

  .section-rule {
    width: 12px;
    height: 1px;
    background: rgba(182, 141, 94, 0.50);
    flex-shrink: 0;
  }

  .section-name {
    font-size: 11px;
    font-weight: 500;
    color: rgba(0, 0, 0, 0.35);
    letter-spacing: 0.06em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .section-count {
    font-size: 10px;
    font-weight: 600;
    color: rgba(0, 0, 0, 0.35);
    background: var(--surface-active);
    padding: 0 5px;
    border-radius: 8px;
    line-height: 16px;
    flex-shrink: 0;
  }

  .todo-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin-bottom: 10px;
  }

  .todo-item {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 4px 6px;
    border-radius: 6px;
    transition: background var(--transition-fast);
  }

  .todo-item:hover {
    background: rgba(182, 141, 94, 0.06);
  }

  .todo-item.done {
    opacity: 0.35;
  }

  .todo-check {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    padding-top: 2px;
  }

  .todo-check input[type='checkbox'] {
    width: 13px;
    height: 13px;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .todo-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .todo-text {
    font-size: 13px;
    color: var(--text-primary);
    line-height: 1.4;
    word-break: break-word;
  }

  .todo-item.done .todo-text {
    text-decoration: line-through;
    text-decoration-color: rgba(0, 0, 0, 0.20);
    color: rgba(0, 0, 0, 0.35);
  }

  .todo-link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11px;
    color: rgba(0, 0, 0, 0.35);
    font-weight: 450;
  }

  .todo-remove {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    color: rgba(0, 0, 0, 0.25);
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
    margin-bottom: 10px;
  }

  .todo-input {
    width: 100%;
    padding: 7px 10px;
    font-family: var(--font-body);
    font-size: 13px;
    color: var(--text-primary);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    outline: none;
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .todo-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(182, 141, 94, 0.10);
  }

  .todo-input::placeholder {
    color: rgba(0, 0, 0, 0.30);
  }

  .empty-text {
    font-size: 12px;
    font-style: italic;
    color: rgba(0, 0, 0, 0.30);
    padding: 4px 6px;
  }
</style>
