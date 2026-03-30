import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Sidebar from './Sidebar.svelte';

const mockState = vi.hoisted(() => ({
  documentState: {
    docs: [
      { id: 'doc-a', projectId: 'project-1', path: 'alpha.md', title: 'alpha', activePeers: [], hasUnread: false, syncStatus: 'local-only', wordCount: 0 },
    ],
    loading: false,
    loadingProjectIds: [] as string[],
    hydratedProjectIds: ['project-1', 'project-2'],
  },
  projectState: {
    projects: [
      { id: 'project-1', name: 'Project 1', path: 'project-1', shared: false, role: 'owner', peerCount: 0 },
      { id: 'project-2', name: 'Project 2', path: 'project-2', shared: false, role: 'owner', peerCount: 0 },
    ],
  },
  uiState: { sidebarOpen: true, rightSidebarOpen: false, quickOpenVisible: false },
  editorSessionState: { projectId: 'project-1', docId: 'doc-a' },
  tauriApi: {
    deleteProject: vi.fn(async () => undefined),
    createNote: vi.fn(async () => 'doc-new'),
    getDocBinary: vi.fn(async () => new Uint8Array([1])),
    openDoc: vi.fn(async () => undefined),
    applyChanges: vi.fn(async () => undefined),
    saveDoc: vi.fn(async () => undefined),
    closeDoc: vi.fn(async () => undefined),
    deleteNote: vi.fn(async () => undefined),
    renameNote: vi.fn(async () => undefined),
  },
  loadProjectDocs: vi.fn(async () => undefined),
  reorderDocs: vi.fn(),
  deleteDoc: vi.fn(async () => undefined),
  removeDoc: vi.fn(),
  getDocById: vi.fn((docId: string) => mockState.documentState.docs.find((doc) => doc.id === docId) ?? null),
  hasHydratedProject: vi.fn((projectId: string) => mockState.documentState.hydratedProjectIds.includes(projectId)),
  isProjectLoading: vi.fn(() => false),
  setDocPath: vi.fn(),
  createProject: vi.fn(async () => null),
  reorderProject: vi.fn(),
  removeProject: vi.fn(),
  openJoinDialog: vi.fn(),
  closeEditorSession: vi.fn(async () => undefined),
  beginMovedDoc: vi.fn(),
  clearMovedDoc: vi.fn(),
  getWorkspaceRoute: vi.fn<() => any>(() => ({ kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' })),
  getWorkspaceProjectId: vi.fn(() => 'project-1'),
  handleDeletedProject: vi.fn(),
  handleDeletedDoc: vi.fn(),
  handleMovedDoc: vi.fn(),
  navigateToDoc: vi.fn(async () => undefined),
  navigateToProject: vi.fn(async () => undefined),
  consoleError: vi.fn(),
}));

vi.mock('../../actions/sortable.js', () => ({ sortable: () => ({ destroy() {} }) }));
vi.mock('../../utils/platform.js', () => ({ isMac: false, modKey: 'Ctrl' }));
vi.mock('../../state/documents.svelte.js', () => ({
  documentState: mockState.documentState,
  loadProjectDocs: mockState.loadProjectDocs,
  reorderDocs: mockState.reorderDocs,
  deleteDoc: mockState.deleteDoc,
  removeDoc: mockState.removeDoc,
  getDocById: mockState.getDocById,
  hasHydratedProject: mockState.hasHydratedProject,
  isProjectLoading: mockState.isProjectLoading,
  setDocPath: mockState.setDocPath,
}));
vi.mock('../../state/projects.svelte.js', () => ({
  projectState: mockState.projectState,
  createProject: mockState.createProject,
  reorderProject: mockState.reorderProject,
  removeProject: mockState.removeProject,
}));
vi.mock('../../state/ui.svelte.js', () => ({
  openQuickOpen: vi.fn(),
  uiState: mockState.uiState,
  toggleSidebar: vi.fn(),
}));
vi.mock('../../session/editor-session.svelte.js', () => ({
  closeEditorSession: mockState.closeEditorSession,
  editorSessionState: mockState.editorSessionState,
}));
vi.mock('../../api/tauri.js', () => ({ tauriApi: mockState.tauriApi }));
vi.mock('../../state/invite.svelte.js', () => ({ openJoinDialog: mockState.openJoinDialog }));
vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  beginMovedDoc: mockState.beginMovedDoc,
  clearMovedDoc: mockState.clearMovedDoc,
  getWorkspaceRoute: mockState.getWorkspaceRoute,
  getWorkspaceProjectId: mockState.getWorkspaceProjectId,
  handleDeletedProject: mockState.handleDeletedProject,
  handleDeletedDoc: mockState.handleDeletedDoc,
  handleMovedDoc: mockState.handleMovedDoc,
  navigateToDoc: mockState.navigateToDoc,
  navigateToProject: mockState.navigateToProject,
}));
vi.mock('./ProjectGroup.svelte', () => import('./__test_mocks__/ProjectGroupMock.svelte'));
vi.mock('./ContextMenu.svelte', () => import('./__test_mocks__/ContextMenuMock.svelte'));

describe('Sidebar doc reconciliation', () => {
  beforeEach(() => {
    mockState.documentState.docs = [
      { id: 'doc-a', projectId: 'project-1', path: 'alpha.md', title: 'alpha', activePeers: [], hasUnread: false, syncStatus: 'local-only', wordCount: 0 },
    ];
    mockState.editorSessionState.projectId = 'project-1';
    mockState.editorSessionState.docId = 'doc-a';
    mockState.deleteDoc.mockReset();
    mockState.closeEditorSession.mockReset();
    mockState.beginMovedDoc.mockReset();
    mockState.clearMovedDoc.mockReset();
    mockState.getWorkspaceRoute.mockReset();
    mockState.getWorkspaceRoute.mockImplementation(() => ({ kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' }));
    mockState.handleDeletedDoc.mockReset();
    mockState.handleMovedDoc.mockReset();
    mockState.navigateToDoc.mockReset();
    mockState.loadProjectDocs.mockReset();
    mockState.consoleError.mockReset();
    mockState.tauriApi.createNote.mockReset();
    mockState.tauriApi.getDocBinary.mockReset();
    mockState.tauriApi.openDoc.mockReset();
    mockState.tauriApi.applyChanges.mockReset();
    mockState.tauriApi.saveDoc.mockReset();
    mockState.tauriApi.closeDoc.mockReset();
    mockState.tauriApi.deleteNote.mockReset();
    mockState.tauriApi.createNote.mockImplementation(async () => 'doc-new');
    mockState.tauriApi.getDocBinary.mockImplementation(async () => new Uint8Array([1]));
    mockState.tauriApi.openDoc.mockImplementation(async () => undefined);
    mockState.tauriApi.applyChanges.mockImplementation(async () => undefined);
    mockState.tauriApi.saveDoc.mockImplementation(async () => undefined);
    mockState.tauriApi.closeDoc.mockImplementation(async () => undefined);
    mockState.tauriApi.deleteNote.mockImplementation(async () => undefined);
    vi.spyOn(console, 'error').mockImplementation(mockState.consoleError);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    cleanup();
  });

  it('deleting the selected doc closes the session and reconciles the route', async () => {
    render(Sidebar);

    await fireEvent.click(screen.getByTestId('doc-menu-doc-a'));
    await fireEvent.click(screen.getByTestId('menu-item-delete'));

    await waitFor(() => {
      expect(mockState.closeEditorSession).toHaveBeenCalledTimes(1);
      expect(mockState.deleteDoc).toHaveBeenCalledWith('project-1', 'doc-a');
      expect(mockState.handleDeletedDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    });
  });

  it('deleting an unselected doc does not close the current session', async () => {
    mockState.documentState.docs = [
      { id: 'doc-b', projectId: 'project-1', path: 'beta.md', title: 'beta', activePeers: [], hasUnread: false, syncStatus: 'local-only', wordCount: 0 },
    ];
    mockState.getWorkspaceRoute.mockImplementation(() => ({ kind: 'project', projectId: 'project-1' }));
    render(Sidebar);

    await fireEvent.click(screen.getByTestId('doc-menu-doc-b'));
    await fireEvent.click(screen.getByTestId('menu-item-delete'));

    await waitFor(() => {
      expect(mockState.closeEditorSession).not.toHaveBeenCalled();
      expect(mockState.handleDeletedDoc).toHaveBeenCalledWith('project-1', 'doc-b');
    });
  });

  it('moving the selected doc reloads projects before reconciling to the destination doc', async () => {
    render(Sidebar);

    await fireEvent.click(screen.getByTestId('doc-menu-doc-a'));
    await fireEvent.click(screen.getByTestId('menu-item-move to... > Project 2'));

    await waitFor(() => {
      expect(mockState.closeEditorSession).toHaveBeenCalledTimes(1);
      expect(mockState.beginMovedDoc).toHaveBeenCalledWith({
        fromProjectId: 'project-1',
        fromDocId: 'doc-a',
        toProjectId: 'project-2',
        toDocId: 'doc-new',
      });
      expect(mockState.deleteDoc).toHaveBeenCalledWith('project-1', 'doc-a');
      expect(mockState.loadProjectDocs).toHaveBeenCalledWith('project-1', { force: true });
      expect(mockState.loadProjectDocs).toHaveBeenCalledWith('project-2', { force: true });
    });

    await waitFor(() => {
      expect(mockState.handleMovedDoc).toHaveBeenCalledWith({
        fromProjectId: 'project-1',
        fromDocId: 'doc-a',
        toProjectId: 'project-2',
        toDocId: 'doc-new',
      });
      expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-2', 'doc-new');
    });

    const loadOrder = mockState.loadProjectDocs.mock.invocationCallOrder.at(-1) ?? 0;
    const moveOrder = mockState.handleMovedDoc.mock.invocationCallOrder[0] ?? 0;
    const openOrder = mockState.navigateToDoc.mock.invocationCallOrder[0] ?? 0;
    expect(moveOrder).toBeGreaterThan(loadOrder);
    expect(openOrder).toBeGreaterThan(moveOrder);
  });

  it('moving an unselected doc does not reopen the destination session', async () => {
    mockState.documentState.docs = [
      { id: 'doc-b', projectId: 'project-1', path: 'beta.md', title: 'beta', activePeers: [], hasUnread: false, syncStatus: 'local-only', wordCount: 0 },
    ];
    mockState.editorSessionState.docId = 'doc-b';
    mockState.getWorkspaceRoute.mockImplementation(() => ({ kind: 'project', projectId: 'project-1' }));
    render(Sidebar);

    await fireEvent.click(screen.getByTestId('doc-menu-doc-b'));
    await fireEvent.click(screen.getByTestId('menu-item-move to... > Project 2'));

    await waitFor(() => {
      expect(mockState.handleMovedDoc).toHaveBeenCalledWith({
        fromProjectId: 'project-1',
        fromDocId: 'doc-b',
        toProjectId: 'project-2',
        toDocId: 'doc-new',
      });
    });

    expect(mockState.navigateToDoc).not.toHaveBeenCalled();
  });

  it('handles direct doc-open failures without unhandled rejections', async () => {
    mockState.navigateToDoc.mockImplementation(async () => {
      throw new Error('open failed');
    });
    render(Sidebar);

    await fireEvent.click(screen.getByTestId('doc-open-doc-a'));

    expect(mockState.consoleError).toHaveBeenCalled();
  });

  it('does not delete the source doc when copying content during move fails', async () => {
    mockState.tauriApi.applyChanges.mockImplementationOnce(async () => {
      throw new Error('copy failed');
    });
    render(Sidebar);

    await fireEvent.click(screen.getByTestId('doc-menu-doc-a'));
    await fireEvent.click(screen.getByTestId('menu-item-move to... > Project 2'));

    await waitFor(() => {
      expect(mockState.tauriApi.deleteNote).toHaveBeenCalledWith('project-2', 'doc-new');
      expect(mockState.deleteDoc).not.toHaveBeenCalled();
      expect(mockState.clearMovedDoc).toHaveBeenCalledWith({ fromProjectId: 'project-1', fromDocId: 'doc-a' });
      expect(mockState.handleMovedDoc).not.toHaveBeenCalled();
      expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    });
  });

  it('keeps the destination route when a post-delete reload step fails', async () => {
    mockState.loadProjectDocs
      .mockImplementationOnce(async () => undefined)
      .mockImplementationOnce(async () => {
        throw new Error('reload failed');
      });
    render(Sidebar);

    await fireEvent.click(screen.getByTestId('doc-menu-doc-a'));
    await fireEvent.click(screen.getByTestId('menu-item-move to... > Project 2'));

    await waitFor(() => {
      expect(mockState.deleteDoc).toHaveBeenCalledWith('project-1', 'doc-a');
      expect(mockState.handleMovedDoc).toHaveBeenCalledWith({
        fromProjectId: 'project-1',
        fromDocId: 'doc-a',
        toProjectId: 'project-2',
        toDocId: 'doc-new',
      });
      expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-2', 'doc-new');
    });
  });
});
