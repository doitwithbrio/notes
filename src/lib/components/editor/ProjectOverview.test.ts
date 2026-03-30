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
    todos: [] as Array<{ id: string; projectId: string; text: string; done: boolean; linkedDocId?: string }>,
  },
  presenceState: {
    peers: [] as Array<{ id: string; online: boolean; cursorColor: string }>,
  },
  navigateToDoc: vi.fn(),
  consoleError: vi.fn(),
  openShareDialog: vi.fn(),
  addTodo: vi.fn(),
  toggleTodo: vi.fn(),
  removeTodo: vi.fn(),
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
  presenceState: mockState.presenceState,
}));

vi.mock('../../state/invite.svelte.js', () => ({
  openShareDialog: mockState.openShareDialog,
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  navigateToDoc: mockState.navigateToDoc,
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
    mockState.presenceState.peers = [];
    mockState.navigateToDoc.mockReset();
    mockState.openShareDialog.mockReset();
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
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: /alpha/i }));

    expect(mockState.consoleError).toHaveBeenCalled();
  });
});
