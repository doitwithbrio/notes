import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import TodosSection from './TodosSection.svelte';

const mockState = vi.hoisted(() => ({
  documentState: {
    docs: [] as Array<{ id: string; title: string }>,
  },
  todoState: {
    loadingProjectIds: [] as string[],
    hydratedProjectIds: [] as string[],
    todos: [] as Array<{
      id: string;
      todoId: string;
      source: 'manual' | 'inline';
      projectId: string;
      text: string;
      done: boolean;
      createdBy?: string;
      createdAt?: string;
      linkedDocId?: string;
    }>,
  },
  addTodo: vi.fn(),
  toggleTodo: vi.fn(),
  removeTodo: vi.fn(),
  getProject: vi.fn(),
  getSelectedDoc: vi.fn(),
  getSelectedProjectId: vi.fn(),
  getWorkspaceContextRoute: vi.fn(),
  isLiveDocRoute: vi.fn(),
  getActiveSession: vi.fn<() => { projectId: string; docId: string } | null>(() => null),
}));

vi.mock('../../state/documents.svelte.js', () => ({
  documentState: mockState.documentState,
}));

vi.mock('../../state/todos.svelte.js', () => ({
  todoState: mockState.todoState,
  addTodo: mockState.addTodo,
  toggleTodo: mockState.toggleTodo,
  removeTodo: mockState.removeTodo,
}));

vi.mock('../../state/projects.svelte.js', () => ({
  getProject: mockState.getProject,
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  getSelectedDoc: mockState.getSelectedDoc,
  getSelectedProjectId: mockState.getSelectedProjectId,
  getWorkspaceContextRoute: mockState.getWorkspaceContextRoute,
  isLiveDocRoute: mockState.isLiveDocRoute,
}));

vi.mock('../../session/editor-session.svelte.js', () => ({
  getActiveSession: mockState.getActiveSession,
}));

describe('TodosSection', () => {
  beforeEach(() => {
    mockState.documentState.docs = [{ id: 'doc-a', title: 'alpha' }, { id: 'doc-b', title: 'beta' }];
    mockState.todoState.todos = [
      {
        id: 'todo-a',
        todoId: 'todo-a',
        source: 'manual',
        projectId: 'project-1',
        text: 'todo for alpha',
        done: false,
        createdBy: 'peer-1',
        createdAt: '2026-03-31T11:00:00Z',
        linkedDocId: 'doc-a',
      },
      {
        id: 'todo-b',
        todoId: 'todo-b',
        source: 'inline',
        projectId: 'project-1',
        text: 'todo for beta',
        done: false,
        linkedDocId: 'doc-b',
      },
    ];
    mockState.todoState.loadingProjectIds = [];
    mockState.todoState.hydratedProjectIds = ['project-1'];
    mockState.getProject.mockReturnValue({ canEdit: true });
    mockState.getSelectedProjectId.mockReturnValue('project-1');
    mockState.getSelectedDoc.mockReturnValue({ id: 'doc-a', title: 'alpha' });
    mockState.getWorkspaceContextRoute.mockReturnValue({ kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' });
    mockState.isLiveDocRoute.mockImplementation((route) => route?.kind === 'doc' && route.mode === 'live');
    mockState.getActiveSession.mockReturnValue(null);
  });

  afterEach(() => {
    cleanup();
  });

  it('shows only todos linked to the active doc in file mode', () => {
    render(TodosSection);

    expect(screen.getByText('todo for alpha')).toBeTruthy();
    expect(screen.queryByText('todo for beta')).toBeNull();
    expect(screen.getByText(/todos · alpha/i)).toBeTruthy();
    expect(screen.getByRole('checkbox', { name: 'todo for alpha' })).toBeTruthy();
  });

  it('hides entry controls when the current project is read-only', () => {
    mockState.getProject.mockReturnValue({ canEdit: false });

    render(TodosSection);

    expect(screen.queryByPlaceholderText(/add a todo/i)).toBeNull();
    expect(screen.getByRole('checkbox')).toHaveProperty('disabled', true);
  });

  it('enables inline toggles when the linked note is the active session', async () => {
    mockState.todoState.todos.push({
      id: 'todo-manual',
      todoId: 'todo-manual',
      source: 'manual',
      projectId: 'project-1',
      text: 'manual todo',
      done: false,
      linkedDocId: 'doc-b',
    });
    mockState.getActiveSession.mockReturnValue({ projectId: 'project-1', docId: 'doc-b' });
    mockState.getSelectedDoc.mockReturnValue({ id: 'doc-b', title: 'beta' });
    mockState.getWorkspaceContextRoute.mockReturnValue({ kind: 'doc', projectId: 'project-1', docId: 'doc-b', mode: 'live' });

    render(TodosSection);

    expect(screen.getByText('todo for beta')).toBeTruthy();
    expect(screen.getByText('manual todo')).toBeTruthy();
    expect(screen.getByRole('checkbox', { name: 'todo for beta' })).toHaveProperty('disabled', false);
    expect(screen.getByRole('checkbox', { name: 'manual todo' })).toHaveProperty('disabled', false);

    await fireEvent.click(screen.getByRole('checkbox', { name: 'todo for beta' }));

    expect(mockState.toggleTodo).toHaveBeenCalledWith('todo-b');
  });

  it('shows a loading state before project todos are hydrated', () => {
    mockState.todoState.loadingProjectIds = ['project-1'];
    mockState.todoState.hydratedProjectIds = [];

    render(TodosSection);

    expect(screen.getByText(/loading todos/i)).toBeTruthy();
    expect(screen.queryByText('todo for alpha')).toBeNull();
  });
});
