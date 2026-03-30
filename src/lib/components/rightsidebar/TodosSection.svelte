<script lang="ts">
  import { documentState } from '../../state/documents.svelte.js';
  import { todoState, addTodo, toggleTodo, removeTodo } from '../../state/todos.svelte.js';
  import { FileText, X } from 'lucide-svelte';
  import { getSelectedDoc, getSelectedProjectId, getWorkspaceContextRoute, isLiveDocRoute } from '../../navigation/workspace-router.svelte.js';

  // Derive context
  const activeDoc = $derived(getSelectedDoc());

  const projectId = $derived(getSelectedProjectId());

  // In editor view with an active doc: show file-specific todos only
  // In project overview: show all project todos
  const isFileMode = $derived.by(() => {
    const route = getWorkspaceContextRoute();
    return isLiveDocRoute(route) && !!activeDoc && route.docId === activeDoc.id;
  });

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
  <div class="section-header">
    <span class="section-title">{headerLabel}</span>
    {#if pendingCount > 0}
      <span class="section-count">{pendingCount}</span>
    {/if}
  </div>

  <div class="section-body">
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
  </div>
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    border-bottom: 1px solid var(--border-subtle);
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 12px 16px 8px;
    flex-shrink: 0;
  }

  .section-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 16px 12px;
  }

  .section-title {
    font-size: 13px;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .section-count {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-primary);
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
    background: var(--surface-hover);
  }

  .todo-item.done {
    opacity: 0.5;
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
    text-decoration-color: var(--text-tertiary);
    color: var(--text-tertiary);
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
    width: 18px;
    height: 18px;
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
  }

  .todo-input::placeholder {
    color: var(--text-secondary);
  }

  .empty-text {
    font-size: 13px;
    color: var(--text-tertiary);
    padding: 4px 6px;
  }
</style>
