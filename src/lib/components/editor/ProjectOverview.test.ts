import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import ProjectOverview from './ProjectOverview.svelte';

const mockState = vi.hoisted(() => ({
  documentState: {
    docs: [] as Array<{
      id: string;
      projectId: string;
      path: string;
      title: string;
      syncStatus: 'synced' | 'syncing' | 'local-only';
      wordCount: number;
      activePeers: string[];
      hasUnread: boolean;
    }>,
  },
  todoState: {
    loadingProjectIds: [] as string[],
    hydratedProjectIds: [] as string[],
    todos: [] as Array<{ id: string; todoId: string; source: 'manual' | 'inline'; projectId: string; text: string; done: boolean; linkedDocId?: string }>,
  },
  getVisibleProjectPeers: vi.fn(() => []),
  getProjectPeerById: vi.fn(() => null),
  navigateToDoc: vi.fn(),
  consoleError: vi.fn(),
  openShareDialog: vi.fn(),
  addTodo: vi.fn(),
  toggleTodo: vi.fn(),
  removeTodo: vi.fn(),
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

vi.mock('../../state/presence.svelte.js', () => ({
  getVisibleProjectPeers: mockState.getVisibleProjectPeers,
  getProjectPeerById: mockState.getProjectPeerById,
}));

vi.mock('../../state/invite.svelte.js', () => ({
  openShareDialog: mockState.openShareDialog,
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  navigateToDoc: mockState.navigateToDoc,
}));

vi.mock('../../session/editor-session.svelte.js', () => ({
  getActiveSession: mockState.getActiveSession,
}));

describe('ProjectOverview', () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    mockState.documentState.docs = [
      {
        id: 'doc-a',
        projectId: 'project-1',
        path: 'alpha.md',
        title: 'alpha',
        syncStatus: 'synced',
        wordCount: 42,
        activePeers: [],
        hasUnread: false,
      },
      {
        id: 'doc-b',
        projectId: 'project-2',
        path: 'beta.md',
        title: 'beta',
        syncStatus: 'synced',
        wordCount: 10,
        activePeers: [],
        hasUnread: false,
      },
    ];
    mockState.todoState.todos = [];
    mockState.todoState.loadingProjectIds = [];
    mockState.todoState.hydratedProjectIds = [];
    mockState.getVisibleProjectPeers.mockReset();
    mockState.getVisibleProjectPeers.mockReturnValue([]);
    mockState.getProjectPeerById.mockReset();
    mockState.getProjectPeerById.mockReturnValue(null);
    mockState.navigateToDoc.mockReset();
    mockState.openShareDialog.mockReset();
    mockState.toggleTodo.mockReset();
    mockState.getActiveSession.mockReturnValue(null);
    mockState.consoleError.mockReset();
    vi.spyOn(console, 'error').mockImplementation(mockState.consoleError);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('opens a note via the workspace router', async () => {
    render(ProjectOverview, {
      project: {
        id: 'project-1',
        name: 'alpha',
        path: 'alpha',
        shared: false,
        peerCount: 0,
        role: 'owner',
        accessState: 'local-owner',
        canEdit: true,
        canManagePeers: true,
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: /alpha/i }));

    expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-1', 'doc-a');
  });

  it('shows only notes from the current project and still allows sharing', async () => {
    render(ProjectOverview, {
      project: {
        id: 'project-1',
        name: 'alpha',
        path: 'alpha',
        shared: true,
        peerCount: 1,
        role: 'owner',
        accessState: 'owner',
        canEdit: true,
        canManagePeers: true,
      },
    });

    expect(screen.getAllByRole('button', { name: /alpha/i })).toHaveLength(1);
    expect(screen.queryByRole('button', { name: /beta/i })).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: /share/i }));

    expect(mockState.openShareDialog).toHaveBeenCalledWith('project-1');
  });

  it('handles note open failures without leaking the rejection', async () => {
    mockState.navigateToDoc.mockImplementation(async () => {
      throw new Error('open failed');
    });

    render(ProjectOverview, {
      project: {
        id: 'project-1',
        name: 'alpha',
        path: 'alpha',
        shared: false,
        peerCount: 0,
        role: 'owner',
        accessState: 'local-owner',
        canEdit: true,
        canManagePeers: true,
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: /alpha/i }));

    expect(mockState.consoleError).toHaveBeenCalled();
  });

  it('shows identity mismatch instead of viewer-like owner actions', async () => {
    render(ProjectOverview, {
      project: {
        id: 'project-1',
        name: 'alpha',
        path: 'alpha',
        shared: true,
        peerCount: 1,
        role: null,
        accessState: 'identity-mismatch',
        canEdit: false,
        canManagePeers: false,
      },
    });

    expect(screen.getByText(/identity mismatch/i)).toBeTruthy();
    expect(screen.getByText(/different device identity/i)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /share/i })).toBeNull();
  });

  it('does not expose todo entry controls to viewers', () => {
    render(ProjectOverview, {
      project: {
        id: 'project-1',
        name: 'alpha',
        path: 'alpha',
        shared: true,
        peerCount: 1,
        role: 'viewer',
        accessState: 'viewer',
        canEdit: false,
        canManagePeers: false,
      },
    });

    expect(screen.queryByPlaceholderText(/add a todo/i)).toBeNull();
    expect(screen.queryByRole('button', { name: /share/i })).toBeNull();
  });

  it('shows a loading message while project todos are hydrating', () => {
    mockState.todoState.loadingProjectIds = ['project-1'];

    render(ProjectOverview, {
      project: {
        id: 'project-1',
        name: 'alpha',
        path: 'alpha',
        shared: false,
        peerCount: 0,
        role: 'owner',
        accessState: 'local-owner',
        canEdit: true,
        canManagePeers: true,
      },
    });

    expect(screen.getByText(/loading todos/i)).toBeTruthy();
  });

  it('enables inline toggles when the linked note is the active session', async () => {
    mockState.todoState.todos = [
      {
        id: 'todo-manual',
        todoId: 'todo-manual',
        source: 'manual',
        projectId: 'project-1',
        text: 'manual alpha',
        done: false,
      },
      {
        id: 'inline:doc-a:todo-a',
        todoId: 'todo-a',
        source: 'inline',
        projectId: 'project-1',
        text: 'inline alpha',
        done: false,
        linkedDocId: 'doc-a',
      },
    ];
    mockState.getActiveSession.mockReturnValue({ projectId: 'project-1', docId: 'doc-a' });

    render(ProjectOverview, {
      project: {
        id: 'project-1',
        name: 'alpha',
        path: 'alpha',
        shared: false,
        peerCount: 0,
        role: 'owner',
        accessState: 'local-owner',
        canEdit: true,
        canManagePeers: true,
      },
    });

    expect(screen.getByText('manual alpha')).toBeTruthy();
    expect(screen.getByText('inline alpha')).toBeTruthy();
    expect(screen.getByRole('checkbox', { name: 'manual alpha' })).toHaveProperty('disabled', false);
    expect(screen.getByRole('checkbox', { name: 'inline alpha' })).toHaveProperty('disabled', false);

    await fireEvent.click(screen.getByRole('checkbox', { name: 'inline alpha' }));

    expect(mockState.toggleTodo).toHaveBeenCalledWith('inline:doc-a:todo-a');
  });
});
