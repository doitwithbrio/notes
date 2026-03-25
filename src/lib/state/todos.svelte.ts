import type { TodoItem } from '../types/index.js';

export const todoState = $state({
  todos: [] as TodoItem[],
});

export function getTodosForProject(projectId: string): TodoItem[] {
  return todoState.todos.filter((t) => t.projectId === projectId);
}

export function addTodo(projectId: string, text: string, linkedDocId?: string) {
  todoState.todos.push({
    id: `todo_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
    projectId,
    text,
    done: false,
    linkedDocId,
    createdAt: Date.now(),
  });
}

export function toggleTodo(id: string) {
  const todo = todoState.todos.find((t) => t.id === id);
  if (todo) todo.done = !todo.done;
}

export function removeTodo(id: string) {
  const idx = todoState.todos.findIndex((t) => t.id === id);
  if (idx >= 0) todoState.todos.splice(idx, 1);
}

export function linkTodoToDoc(todoId: string, docId: string | undefined) {
  const todo = todoState.todos.find((t) => t.id === todoId);
  if (todo) todo.linkedDocId = docId;
}

export function updateTodoText(id: string, text: string) {
  const todo = todoState.todos.find((t) => t.id === id);
  if (todo) todo.text = text;
}
