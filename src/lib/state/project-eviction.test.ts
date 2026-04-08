import { beforeEach, describe, expect, it, vi } from 'vitest';

const mockState = vi.hoisted(() => ({
  editorSessionState: { projectId: 'project-1', docId: 'doc-a' },
  closeEditorSession: vi.fn(async () => undefined),
  clearProjectDocs: vi.fn(),
  removeProject: vi.fn(),
  clearProjectTodos: vi.fn(),
  clearProjectPeers: vi.fn(),
  getOnlinePeers: vi.fn(() => []),
  hasAnySharedPeers: vi.fn(() => false),
  clearVersions: vi.fn(),
  clearVersionPreview: vi.fn(),
  closeQuickOpen: vi.fn(),
  showRevokedProjectNotice: vi.fn(),
  clearRevokedProjectNotice: vi.fn(),
  handleDeletedProject: vi.fn(),
  setPeerCount: vi.fn(),
  setSharedProject: vi.fn(),
  removeProjectOrder: vi.fn(),
}));

vi.mock('../session/editor-session.svelte.js', () => ({
  closeEditorSession: mockState.closeEditorSession,
  editorSessionState: mockState.editorSessionState,
}));
vi.mock('./documents.svelte.js', () => ({ clearProjectDocs: mockState.clearProjectDocs }));
vi.mock('./projects.svelte.js', () => ({ removeProject: mockState.removeProject }));
vi.mock('./todos.svelte.js', () => ({ clearProjectTodos: mockState.clearProjectTodos }));
vi.mock('./presence.svelte.js', () => ({
  clearProjectPeers: mockState.clearProjectPeers,
  getOnlinePeers: mockState.getOnlinePeers,
  hasAnySharedPeers: mockState.hasAnySharedPeers,
}));
vi.mock('./versions.svelte.js', () => ({ clearVersions: mockState.clearVersions }));
vi.mock('./version-review.svelte.js', () => ({ clearVersionPreview: mockState.clearVersionPreview }));
vi.mock('./ui.svelte.js', () => ({
  closeQuickOpen: mockState.closeQuickOpen,
  showRevokedProjectNotice: mockState.showRevokedProjectNotice,
  clearRevokedProjectNotice: mockState.clearRevokedProjectNotice,
}));
vi.mock('../navigation/workspace-router.svelte.js', () => ({ handleDeletedProject: mockState.handleDeletedProject }));
vi.mock('./sync.svelte.js', () => ({ setPeerCount: mockState.setPeerCount, setSharedProject: mockState.setSharedProject }));
vi.mock('./ordering.svelte.js', () => ({ removeProjectOrder: mockState.removeProjectOrder }));

describe('project eviction', () => {
  beforeEach(() => {
    mockState.editorSessionState.projectId = 'project-1';
    mockState.editorSessionState.docId = 'doc-a';
    mockState.closeEditorSession.mockReset();
    mockState.clearProjectDocs.mockReset();
    mockState.removeProject.mockReset();
    mockState.clearProjectTodos.mockReset();
    mockState.clearProjectPeers.mockReset();
    mockState.clearVersions.mockReset();
    mockState.clearVersionPreview.mockReset();
    mockState.closeQuickOpen.mockReset();
    mockState.showRevokedProjectNotice.mockReset();
    mockState.handleDeletedProject.mockReset();
    mockState.setPeerCount.mockReset();
    mockState.setSharedProject.mockReset();
    mockState.removeProjectOrder.mockReset();
  });

  it('evicts a project from all frontend state and shows a notice', async () => {
    const eviction = await import('./project-eviction.svelte.js');

    await eviction.evictProject('project-1', 'access-revoked', 'Secret Project');

    expect(mockState.closeEditorSession).toHaveBeenCalledTimes(1);
    expect(mockState.clearVersions).toHaveBeenCalledTimes(1);
    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
    expect(mockState.clearProjectPeers).toHaveBeenCalledWith('project-1');
    expect(mockState.clearProjectTodos).toHaveBeenCalledWith('project-1');
    expect(mockState.clearProjectDocs).toHaveBeenCalledWith('project-1');
    expect(mockState.removeProject).toHaveBeenCalledWith('project-1');
    expect(mockState.removeProjectOrder).toHaveBeenCalledWith('project-1');
    expect(mockState.handleDeletedProject).toHaveBeenCalledWith('project-1');
    expect(mockState.closeQuickOpen).toHaveBeenCalledTimes(1);
    expect(mockState.showRevokedProjectNotice).toHaveBeenCalledWith('project-1', 'project-1', 'Secret Project', 'access-revoked');
  });
});
