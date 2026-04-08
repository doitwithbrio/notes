import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as Automerge from '@automerge/automerge';

import type { BackendDocInfo, BackendTodoItem } from '../types/index.js';
import { loadEditorDocument } from '../editor/document-adapter.js';

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const tauriApiMock = vi.hoisted(() => ({
  listProjectTodos: vi.fn<() => Promise<BackendTodoItem[]>>(async () => []),
  listFiles: vi.fn<() => Promise<BackendDocInfo[]>>(async () => []),
  getDocBinary: vi.fn<() => Promise<Uint8Array>>(async () => new Uint8Array()),
  applyChanges: vi.fn(async () => undefined),
  saveDoc: vi.fn(async () => undefined),
  closeDoc: vi.fn(async () => undefined),
  addProjectTodo: vi.fn(async () => 'todo-1'),
  toggleProjectTodo: vi.fn(async () => undefined),
  removeProjectTodo: vi.fn(async () => undefined),
  updateProjectTodo: vi.fn(async () => undefined),
}));

const sessionMock = vi.hoisted(() => ({
  toggleInlineTodoInActiveSession: vi.fn(() => false),
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: tauriApiMock,
}));

vi.mock('../session/editor-session.svelte.js', () => sessionMock);

async function loadFreshTodos() {
  vi.resetModules();
  const todos = await import('./todos.svelte.js');
  todos.todoState.todos = [];
  return todos;
}

function makeGraphBinaryWithTask(text: string, todoId = 'inline-1', checked = false) {
  return new Uint8Array(Automerge.save(Automerge.from({
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [{
        type: 'task_list',
        content: [{
          type: 'task_item',
          attrs: { checked, todoId },
          content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
        }],
      }],
    },
  })));
}

describe('todos state', () => {
  beforeEach(() => {
    tauriApiMock.listProjectTodos.mockClear();
    tauriApiMock.listFiles.mockClear();
    tauriApiMock.getDocBinary.mockClear();
    tauriApiMock.addProjectTodo.mockClear();
    tauriApiMock.applyChanges.mockClear();
    tauriApiMock.saveDoc.mockClear();
    tauriApiMock.closeDoc.mockClear();
    tauriApiMock.toggleProjectTodo.mockClear();
    tauriApiMock.removeProjectTodo.mockClear();
    tauriApiMock.updateProjectTodo.mockClear();
    tauriApiMock.listProjectTodos.mockImplementation(async () => []);
    tauriApiMock.listFiles.mockImplementation(async () => []);
    tauriApiMock.getDocBinary.mockImplementation(async () => new Uint8Array());
    tauriApiMock.applyChanges.mockImplementation(async () => undefined);
    tauriApiMock.saveDoc.mockImplementation(async () => undefined);
    tauriApiMock.closeDoc.mockImplementation(async () => undefined);
    sessionMock.toggleInlineTodoInActiveSession.mockReset();
    sessionMock.toggleInlineTodoInActiveSession.mockReturnValue(false);
  });

  it('hydrates project todos through the backend and maps project ids client-side', async () => {
    tauriApiMock.listProjectTodos.mockImplementation(async () => ([
      {
        id: 'todo-1',
        text: 'from backend',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
        linkedDocId: 'doc-a',
      },
    ]));
    tauriApiMock.listFiles.mockImplementation(async () => ([
      { id: 'doc-a', path: 'a.md', fileType: 'note', created: '2026-03-31T10:00:00Z' },
    ]));
    tauriApiMock.getDocBinary.mockImplementation(async () => makeGraphBinaryWithTask('inline task'));

    const todos = await loadFreshTodos();

    await todos.loadProjectTodos('project-1', { force: true });

    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledWith('project-1');
    expect(todos.todoState.todos).toEqual([
      {
        id: 'todo-1',
        todoId: 'todo-1',
        projectId: 'project-1',
        source: 'manual',
        text: 'from backend',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
        linkedDocId: 'doc-a',
      },
      {
        id: 'inline:doc-a:inline-1',
        todoId: 'inline-1',
        projectId: 'project-1',
        source: 'inline',
        text: 'inline task',
        done: false,
        linkedDocId: 'doc-a',
        order: 0,
        depth: 0,
      },
    ]);
  });

  it('skips a non-forced reload after the project todos are already hydrated', async () => {
    tauriApiMock.listProjectTodos.mockImplementation(async () => ([
      {
        id: 'todo-1',
        text: 'from backend',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ]));

    const todos = await loadFreshTodos();

    await todos.loadProjectTodos('project-1', { force: true });
    await todos.loadProjectTodos('project-1');

    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledTimes(1);

    todos.clearProjectTodos('project-1');
    await todos.loadProjectTodos('project-1');

    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledTimes(2);
  });

  it('addTodo persists through the backend instead of only mutating local state', async () => {
    tauriApiMock.listProjectTodos.mockImplementation(async () => ([
      {
        id: 'todo-1',
        text: 'buy milk',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ]));
    const todos = await loadFreshTodos();

    await todos.addTodo('project-1', 'buy milk');

    expect(tauriApiMock.addProjectTodo).toHaveBeenCalledWith('project-1', 'buy milk', undefined);
    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledWith('project-1');
  });

  it('toggleTodo persists through the backend', async () => {
    tauriApiMock.listProjectTodos.mockImplementation(async () => ([
      {
        id: 'todo-1',
        text: 'buy milk',
        done: true,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ]));
    const todos = await loadFreshTodos();
    todos.todoState.todos = [
      {
        id: 'todo-1',
        todoId: 'todo-1',
        projectId: 'project-1',
        source: 'manual',
        text: 'buy milk',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ];

    await todos.toggleTodo('todo-1');

    expect(tauriApiMock.toggleProjectTodo).toHaveBeenCalledWith('project-1', 'todo-1');
    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledWith('project-1');
    expect(todos.todoState.todos[0]?.done).toBe(true);
  });

  it('removeTodo persists through the backend', async () => {
    tauriApiMock.listProjectTodos.mockImplementation(async () => []);
    const todos = await loadFreshTodos();
    todos.todoState.todos = [
      {
        id: 'todo-1',
        todoId: 'todo-1',
        projectId: 'project-1',
        source: 'manual',
        text: 'buy milk',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ];

    await todos.removeTodo('todo-1');

    expect(tauriApiMock.removeProjectTodo).toHaveBeenCalledWith('project-1', 'todo-1');
    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledWith('project-1');
    expect(todos.todoState.todos).toEqual([]);
  });

  it('updateTodoText persists through the backend', async () => {
    tauriApiMock.listProjectTodos.mockImplementation(async () => ([
      {
        id: 'todo-1',
        text: 'buy oat milk',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ]));
    const todos = await loadFreshTodos();
    todos.todoState.todos = [
      {
        id: 'todo-1',
        todoId: 'todo-1',
        projectId: 'project-1',
        source: 'manual',
        text: 'buy milk',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ];

    await todos.updateTodoText('todo-1', 'buy oat milk');

    expect(tauriApiMock.updateProjectTodo).toHaveBeenCalledWith('project-1', 'todo-1', 'buy oat milk');
    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledWith('project-1');
    expect(todos.todoState.todos[0]?.text).toBe('buy oat milk');
  });

  it('queues a force reload requested while an earlier todo reload is in flight', async () => {
    const gate = deferred<BackendTodoItem[]>();
    tauriApiMock.listProjectTodos
      .mockImplementationOnce(() => gate.promise)
      .mockImplementationOnce(async () => ([
        {
          id: 'todo-2',
          text: 'second pass',
          done: false,
          createdBy: 'peer-1',
          createdAt: '2026-03-31T11:01:00Z',
        },
      ]));

    const todos = await loadFreshTodos();
    const firstLoad = todos.loadProjectTodos('project-1', { force: true });
    const secondLoad = todos.loadProjectTodos('project-1', { force: true });

    gate.resolve([
      {
        id: 'todo-1',
        text: 'first pass',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ]);

    await Promise.all([firstLoad, secondLoad]);

    expect(tauriApiMock.listProjectTodos).toHaveBeenCalledTimes(2);
    expect(todos.todoState.todos[0]?.id).toBe('todo-2');
  });

  it('includes extracted inline todos from note documents in the hydrated project list', async () => {
    tauriApiMock.listFiles.mockImplementation(async () => ([
      { id: 'doc-a', path: 'a.md', fileType: 'note', created: '2026-03-31T10:00:00Z' },
      { id: 'doc-b', path: 'b.md', fileType: 'note', created: '2026-03-31T10:01:00Z' },
    ]));
    tauriApiMock.getDocBinary
      .mockImplementationOnce(async () => makeGraphBinaryWithTask('inline a', 'todo-a'))
      .mockImplementationOnce(async () => makeGraphBinaryWithTask('inline b', 'todo-b', true));

    const todos = await loadFreshTodos();

    await todos.loadProjectTodos('project-1', { force: true });

    expect(todos.todoState.todos.filter((todo) => todo.source === 'inline')).toEqual([
      {
        id: 'inline:doc-a:todo-a',
        todoId: 'todo-a',
        projectId: 'project-1',
        source: 'inline',
        text: 'inline a',
        done: false,
        linkedDocId: 'doc-a',
        order: 0,
        depth: 0,
      },
      {
        id: 'inline:doc-b:todo-b',
        todoId: 'todo-b',
        projectId: 'project-1',
        source: 'inline',
        text: 'inline b',
        done: true,
        linkedDocId: 'doc-b',
        order: 0,
        depth: 0,
      },
    ]);
  });

  it('keeps manual todos hydrated even if one note fails inline extraction', async () => {
    tauriApiMock.listProjectTodos.mockImplementation(async () => ([
      {
        id: 'todo-1',
        text: 'manual todo',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
      },
    ]));
    tauriApiMock.listFiles.mockImplementation(async () => ([
      { id: 'doc-a', path: 'a.md', fileType: 'note', created: '2026-03-31T10:00:00Z' },
      { id: 'doc-b', path: 'b.md', fileType: 'note', created: '2026-03-31T10:01:00Z' },
    ]));
    tauriApiMock.getDocBinary
      .mockImplementationOnce(async () => makeGraphBinaryWithTask('inline ok', 'todo-a'))
      .mockImplementationOnce(async () => {
        throw new Error('bad note');
      });

    const todos = await loadFreshTodos();

    await todos.loadProjectTodos('project-1', { force: true });

    expect(todos.todoState.todos.map((todo) => todo.id)).toEqual([
      'todo-1',
      'inline:doc-a:todo-a',
    ]);
  });

  it('reprojects inline todos for an open document without a project reload', async () => {
    const todos = await loadFreshTodos();
    todos.todoState.todos = [{
      id: 'todo-1',
      todoId: 'todo-1',
      projectId: 'project-1',
      source: 'manual',
      text: 'manual',
      done: false,
    }];

    todos.syncInlineTodosForDoc('project-1', 'doc-a', loadEditorDocument(makeGraphBinaryWithTask('inline a', 'todo-a')).editorDocument);
    todos.syncInlineTodosForDoc('project-1', 'doc-a', loadEditorDocument(makeGraphBinaryWithTask('inline updated', 'todo-a', true)).editorDocument);

    expect(todos.todoState.todos).toEqual([
      {
        id: 'todo-1',
        todoId: 'todo-1',
        projectId: 'project-1',
        source: 'manual',
        text: 'manual',
        done: false,
      },
      {
        id: 'inline:doc-a:todo-a',
        todoId: 'todo-a',
        projectId: 'project-1',
        source: 'inline',
        text: 'inline updated',
        done: true,
        linkedDocId: 'doc-a',
        order: 0,
        depth: 0,
      },
    ]);
  });

  it('routes open-note inline toggles through the active session helper', async () => {
    sessionMock.toggleInlineTodoInActiveSession.mockReturnValue(true);
    const todos = await loadFreshTodos();
    todos.todoState.todos = [{
      id: 'inline:doc-a:todo-a',
      todoId: 'todo-a',
      projectId: 'project-1',
      source: 'inline',
      text: 'inline a',
      done: false,
      linkedDocId: 'doc-a',
      order: 0,
      depth: 0,
    }];

    await todos.toggleTodo('inline:doc-a:todo-a');

    expect(sessionMock.toggleInlineTodoInActiveSession).toHaveBeenCalledWith('project-1', 'doc-a', 'todo-a');
    expect(tauriApiMock.applyChanges).not.toHaveBeenCalled();
    expect(tauriApiMock.toggleProjectTodo).not.toHaveBeenCalled();
  });

  it('toggles closed-note inline todos through the document pipeline and refreshes projection', async () => {
    tauriApiMock.getDocBinary.mockImplementation(async () => makeGraphBinaryWithTask('inline a', 'todo-a'));
    const todos = await loadFreshTodos();
    todos.todoState.todos = [{
      id: 'inline:doc-a:todo-a',
      todoId: 'todo-a',
      projectId: 'project-1',
      source: 'inline',
      text: 'inline a',
      done: false,
      linkedDocId: 'doc-a',
      order: 0,
      depth: 0,
    }];

    await todos.toggleTodo('inline:doc-a:todo-a');

    expect(tauriApiMock.applyChanges).toHaveBeenCalledTimes(1);
    expect(tauriApiMock.saveDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(tauriApiMock.closeDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(todos.todoState.todos[0]?.done).toBe(true);
  });

  it('does not try to mutate inline todos through manifest commands', async () => {
    const todos = await loadFreshTodos();
    todos.todoState.todos = [{
      id: 'inline:doc-a:todo-a',
      todoId: 'todo-a',
      projectId: 'project-1',
      source: 'inline',
      text: 'inline a',
      done: false,
      linkedDocId: 'doc-a',
      order: 0,
      depth: 0,
    }];

    await todos.toggleTodo('inline:doc-a:todo-a');
    await todos.removeTodo('inline:doc-a:todo-a');
    await todos.updateTodoText('inline:doc-a:todo-a', 'changed');

    expect(tauriApiMock.toggleProjectTodo).not.toHaveBeenCalled();
    expect(tauriApiMock.removeProjectTodo).not.toHaveBeenCalled();
    expect(tauriApiMock.updateProjectTodo).not.toHaveBeenCalled();
  });
});
