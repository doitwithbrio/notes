import { tauriApi } from '../api/tauri.js';
import type { BackendTodoItem, TodoItem } from '../types/index.js';
import type { EditorDocument } from '../editor/schema.js';
import { buildStoredDocumentUpdate, loadEditorDocument } from '../editor/document-adapter.js';
import { extractInlineTodos, toggleInlineTodoInDocument } from '../editor/inline-todos.js';
import { getVisibleTextFromDocument } from '../editor/schema.js';

type ProjectTodoLoadState = {
  promise: Promise<void> | null;
  reloadRequested: boolean;
};

export const todoState = $state({
  todos: [] as TodoItem[],
  loadingProjectIds: [] as string[],
  hydratedProjectIds: [] as string[],
});

const projectLoadStates = new Map<string, ProjectTodoLoadState>();

function getProjectLoadState(projectId: string): ProjectTodoLoadState {
  let state = projectLoadStates.get(projectId);
  if (!state) {
    state = { promise: null, reloadRequested: false };
    projectLoadStates.set(projectId, state);
  }
  return state;
}

function setProjectLoading(projectId: string, loading: boolean) {
  if (loading) {
    if (!todoState.loadingProjectIds.includes(projectId)) {
      todoState.loadingProjectIds = [...todoState.loadingProjectIds, projectId];
    }
    return;
  }

  if (todoState.loadingProjectIds.includes(projectId)) {
    todoState.loadingProjectIds = todoState.loadingProjectIds.filter((id) => id !== projectId);
  }
}

function markProjectHydrated(projectId: string) {
  if (!todoState.hydratedProjectIds.includes(projectId)) {
    todoState.hydratedProjectIds = [...todoState.hydratedProjectIds, projectId];
  }
}

function mapTodo(projectId: string, todo: BackendTodoItem): TodoItem {
  return {
    id: todo.id,
    todoId: todo.id,
    projectId,
    source: 'manual',
    text: todo.text,
    done: todo.done,
    createdBy: todo.createdBy,
    createdAt: todo.createdAt,
    linkedDocId: todo.linkedDocId,
  };
}

function mapInlineTodo(projectId: string, docId: string, todo: ReturnType<typeof extractInlineTodos>[number]): TodoItem {
  return {
    id: `inline:${docId}:${todo.id}`,
    todoId: todo.id,
    projectId,
    source: 'inline',
    text: todo.text,
    done: todo.done,
    linkedDocId: docId,
    order: todo.order,
    depth: todo.depth,
  };
}

function replaceProjectTodos(projectId: string, todos: TodoItem[]) {
  todoState.todos = [
    ...todoState.todos.filter((todo) => todo.projectId !== projectId),
    ...todos,
  ];
}

function replaceInlineTodosForDoc(projectId: string, docId: string, todos: TodoItem[]) {
  todoState.todos = [
    ...todoState.todos.filter((todo) => !(todo.projectId === projectId && todo.source === 'inline' && todo.linkedDocId === docId)),
    ...todos,
  ];
}

async function fetchProjectTodos(projectId: string): Promise<TodoItem[]> {
  const [todos, files] = await Promise.all([
    tauriApi.listProjectTodos(projectId),
    tauriApi.listFiles(projectId),
  ]);

  const inlineTodoGroups = await Promise.allSettled(
    files
      .filter((file) => file.fileType === 'note')
      .map(async (file) => {
        const binary = await tauriApi.getDocBinary(projectId, file.id);
        const loaded = loadEditorDocument(binary);
        return extractInlineTodos(loaded.editorDocument).map((todo) => mapInlineTodo(projectId, file.id, todo));
      }),
  );

  return [
    ...todos.map((todo) => mapTodo(projectId, todo)),
    ...inlineTodoGroups.flatMap((result) => {
      if (result.status === 'fulfilled') return result.value;
      console.error(`Failed to hydrate inline todos for ${projectId}`, result.reason);
      return [];
    }),
  ];
}

export function getTodosForProject(projectId: string): TodoItem[] {
  return todoState.todos.filter((todo) => todo.projectId === projectId);
}

export function hasHydratedProjectTodos(projectId: string) {
  return todoState.hydratedProjectIds.includes(projectId);
}

export async function loadProjectTodos(projectId: string, options?: { force?: boolean }) {
  if (!options?.force && hasHydratedProjectTodos(projectId)) {
    return;
  }

  const loadState = getProjectLoadState(projectId);
  loadState.reloadRequested = loadState.reloadRequested || !!options?.force || !hasHydratedProjectTodos(projectId);

  if (loadState.promise) {
    return loadState.promise;
  }

  loadState.promise = (async () => {
    setProjectLoading(projectId, true);
    try {
      do {
        const shouldReload = loadState.reloadRequested;
        loadState.reloadRequested = false;

        if (!shouldReload) continue;

        const todos = await fetchProjectTodos(projectId);
        replaceProjectTodos(projectId, todos);
        markProjectHydrated(projectId);
      } while (loadState.reloadRequested);
    } finally {
      setProjectLoading(projectId, false);
      loadState.promise = null;
    }
  })();

  return loadState.promise;
}

export function clearProjectTodos(projectId: string) {
  todoState.todos = todoState.todos.filter((todo) => todo.projectId !== projectId);
  todoState.loadingProjectIds = todoState.loadingProjectIds.filter((id) => id !== projectId);
  todoState.hydratedProjectIds = todoState.hydratedProjectIds.filter((id) => id !== projectId);
  projectLoadStates.delete(projectId);
}

export function syncInlineTodosForDoc(projectId: string, docId: string, document: EditorDocument) {
  const todos = extractInlineTodos(document).map((todo) => mapInlineTodo(projectId, docId, todo));
  replaceInlineTodosForDoc(projectId, docId, todos);
}

async function toggleInlineTodo(todo: TodoItem) {
  if (!todo.linkedDocId) return;

  const editorSession = await import('../session/editor-session.svelte.js');
  if (editorSession.toggleInlineTodoInActiveSession(todo.projectId, todo.linkedDocId, todo.todoId)) {
    return;
  }

  let shouldCloseDoc = false;
  let nextDocument: EditorDocument | null = null;

  try {
    const binary = await tauriApi.getDocBinary(todo.projectId, todo.linkedDocId);
    shouldCloseDoc = true;
    const loaded = loadEditorDocument(binary);
    const toggled = toggleInlineTodoInDocument(loaded.editorDocument, todo.todoId);
    if (!toggled) {
      await loadProjectTodos(todo.projectId, { force: true });
      return;
    }

    const update = buildStoredDocumentUpdate(
      loaded.storageDoc,
      toggled.document,
      getVisibleTextFromDocument(toggled.document),
    );

    await tauriApi.applyChanges(todo.projectId, todo.linkedDocId, update.incremental);
    await tauriApi.saveDoc(todo.projectId, todo.linkedDocId);
    nextDocument = toggled.document;
  } finally {
    if (shouldCloseDoc) {
      try {
        await tauriApi.closeDoc(todo.projectId, todo.linkedDocId);
      } catch (error) {
        console.error(`Failed to close inline todo doc ${todo.linkedDocId}`, error);
      }
    }
  }

  if (nextDocument) {
    syncInlineTodosForDoc(todo.projectId, todo.linkedDocId, nextDocument);
  }
}

export async function addTodo(projectId: string, text: string, linkedDocId?: string) {
  const normalized = text.trim();
  if (!normalized) return;
  await tauriApi.addProjectTodo(projectId, normalized, linkedDocId);
  await loadProjectTodos(projectId, { force: true });
}

export async function toggleTodo(id: string) {
  const todo = todoState.todos.find((item) => item.id === id);
  if (!todo) return;
  if (todo.source === 'inline') {
    await toggleInlineTodo(todo);
    return;
  }
  await tauriApi.toggleProjectTodo(todo.projectId, id);
  await loadProjectTodos(todo.projectId, { force: true });
}

export async function removeTodo(id: string) {
  const todo = todoState.todos.find((item) => item.id === id);
  if (!todo) return;
  if (todo.source !== 'manual') return;
  await tauriApi.removeProjectTodo(todo.projectId, id);
  await loadProjectTodos(todo.projectId, { force: true });
}

export function linkTodoToDoc(todoId: string, docId: string | undefined) {
  const todo = todoState.todos.find((item) => item.id === todoId);
  if (todo) todo.linkedDocId = docId;
}

export async function updateTodoText(id: string, text: string) {
  const todo = todoState.todos.find((item) => item.id === id);
  const normalized = text.trim();
  if (!todo || !normalized) return;
  if (todo.source !== 'manual') return;
  await tauriApi.updateProjectTodo(todo.projectId, id, normalized);
  await loadProjectTodos(todo.projectId, { force: true });
}
