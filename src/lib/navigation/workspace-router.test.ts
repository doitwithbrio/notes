import { beforeEach, describe, expect, it, vi } from 'vitest';

const mockState = vi.hoisted(() => ({
  uiState: {
    sidebarOpen: true,
    rightSidebarOpen: false,
    quickOpenVisible: false,
  },
  docs: [
    { id: 'doc-a', projectId: 'project-1', title: 'Alpha', activePeers: ['peer-1'] },
    { id: 'doc-b', projectId: 'project-1', title: 'Beta', activePeers: [] },
  ] as Array<{ id: string; projectId: string; title: string; activePeers: string[] }>,
  projects: [
    { id: 'project-1', name: 'Project 1' },
    { id: 'project-2', name: 'Project 2' },
  ] as Array<{ id: string; name: string }>,
  hydratedProjects: new Set<string>(),
  versionState: {},
  versionReviewState: {
    previewVersionId: null as string | null,
  },
  tauriApi: {
    openProject: vi.fn(async () => undefined),
  },
  closeEditorSession: vi.fn(async () => undefined),
  openEditorSession: vi.fn(async () => undefined),
  loadProjectDocs: vi.fn(async (projectId: string) => {
    mockState.hydratedProjects.add(projectId);
  }),
  previewVersion: vi.fn(async (_projectId: string, _docId: string, versionId: string) => {
    mockState.versionReviewState.previewVersionId = versionId;
  }),
  getAdjacentSignificantVersionId: vi.fn((versionId: string, direction: 'older' | 'newer') => {
    if (versionId === 'version-1' && direction === 'older') return 'version-2';
    if (versionId === 'version-2' && direction === 'newer') return 'version-1';
    return null;
  }),
  clearVersionPreview: vi.fn(() => {
    mockState.versionReviewState.previewVersionId = null;
  }),
  restoreVersionData: vi.fn(async () => undefined),
  loadVersions: vi.fn(async () => undefined),
  reloadActiveSession: vi.fn(async () => undefined),
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: mockState.tauriApi,
}));

vi.mock('../state/ui.svelte.js', () => ({
  uiState: mockState.uiState,
}));

vi.mock('../state/documents.svelte.js', () => ({
  getDocById: (docId: string | null) => mockState.docs.find((doc) => doc.id === docId) ?? null,
  loadProjectDocs: mockState.loadProjectDocs,
  hasHydratedProject: (projectId: string) => mockState.hydratedProjects.has(projectId),
}));

vi.mock('../state/projects.svelte.js', () => ({
  getProject: (projectId: string | null) => mockState.projects.find((project) => project.id === projectId) ?? null,
}));

vi.mock('../session/editor-session.svelte.js', () => ({
  openEditorSession: mockState.openEditorSession,
  closeEditorSession: mockState.closeEditorSession,
  reloadActiveSession: mockState.reloadActiveSession,
}));

vi.mock('../state/versions.svelte.js', () => ({
  versionState: mockState.versionState,
  loadVersions: mockState.loadVersions,
}));

vi.mock('../state/version-review.svelte.js', () => ({
  versionReviewState: mockState.versionReviewState,
  previewVersion: mockState.previewVersion,
  getAdjacentSignificantVersionId: mockState.getAdjacentSignificantVersionId,
  clearVersionPreview: mockState.clearVersionPreview,
  restoreVersionData: mockState.restoreVersionData,
}));

async function loadRouter() {
  vi.resetModules();
  return import('./workspace-router.svelte.js');
}

function resetMockState() {
  delete (mockState.uiState as Record<string, unknown>).view;
  delete (mockState.uiState as Record<string, unknown>).activeProjectId;
  delete (mockState.uiState as Record<string, unknown>).historyReviewSessionId;
  mockState.docs = [
    { id: 'doc-a', projectId: 'project-1', title: 'Alpha', activePeers: ['peer-1'] },
    { id: 'doc-b', projectId: 'project-1', title: 'Beta', activePeers: [] },
  ];
  mockState.projects = [
    { id: 'project-1', name: 'Project 1' },
    { id: 'project-2', name: 'Project 2' },
  ];
  mockState.hydratedProjects.clear();
  mockState.versionReviewState.previewVersionId = null;
  mockState.tauriApi.openProject.mockClear();
  mockState.closeEditorSession.mockClear();
  mockState.openEditorSession.mockClear();
  mockState.loadProjectDocs.mockClear();
  mockState.previewVersion.mockClear();
  mockState.getAdjacentSignificantVersionId.mockClear();
  mockState.clearVersionPreview.mockClear();
  mockState.restoreVersionData.mockClear();
  mockState.loadVersions.mockClear();
  mockState.reloadActiveSession.mockClear();
}

beforeEach(() => {
  resetMockState();
});

function setLegacyUiRoute(route: {
  view?: string;
  activeProjectId?: string | null;
  historyReviewSessionId?: string | null;
}) {
  Object.assign(mockState.uiState as Record<string, unknown>, route);
}

describe('workspace router', () => {
  it('starts with no canonical route selected', async () => {
    const router = await loadRouter();

    expect(router.getWorkspaceRoute()).toBeNull();
    expect(router.getWorkspaceContextRoute()).toBeNull();
    expect(router.getSelectedProjectId()).toBeNull();
    expect(router.getSelectedDocId()).toBeNull();
    expect(router.getSelectedDoc()).toBeNull();
    expect(router.getSelectedHistoryVersionId()).toBeNull();
  });

  it('selects project, live doc, history doc, and settings routes canonically', async () => {
    const router = await loadRouter();

    await router.navigateToProject('project-1');
    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-1' });

    await router.navigateToDoc('project-1', 'doc-a');
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    });

    router.navigateToSettings();
    expect(router.getWorkspaceRoute()).toEqual({ kind: 'settings' });
  });

  it('exposes project and doc context through settings', async () => {
    const router = await loadRouter();

    await router.navigateToDoc('project-1', 'doc-a');
    router.navigateToSettings();

    expect(router.getWorkspaceContextRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
    expect(router.getSelectedProjectId()).toBe('project-1');
    expect(router.getSelectedDocId()).toBe('doc-a');
    expect(router.getSelectedDoc()).toEqual(mockState.docs[0]);
  });

  it('preserves history context while settings is open', async () => {
    const router = await loadRouter();

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    router.navigateToSettings();

    expect(router.getWorkspaceContextRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    });
    expect(router.getSelectedHistoryVersionId()).toBe('version-1');
  });

  it('returns null context when settings opens without a saved route', async () => {
    const router = await loadRouter();

    router.navigateToSettings();

    expect(router.getWorkspaceRoute()).toEqual({ kind: 'settings' });
    expect(router.getWorkspaceContextRoute()).toBeNull();
    expect(router.getSelectedProjectId()).toBeNull();
    expect(router.getSelectedDocId()).toBeNull();
    expect(router.getSelectedDoc()).toBeNull();
    expect(router.getSelectedHistoryVersionId()).toBeNull();
  });

  it('resolves selected project/doc for project, live doc, and history routes', async () => {
    const router = await loadRouter();

    await router.navigateToProject('project-1');
    expect(router.getSelectedProjectId()).toBe('project-1');
    expect(router.getSelectedDocId()).toBeNull();

    await router.navigateToDoc('project-1', 'doc-a');
    expect(router.getSelectedProjectId()).toBe('project-1');
    expect(router.getSelectedDocId()).toBe('doc-a');
    expect(router.getSelectedDoc()).toEqual(mockState.docs[0]);

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    expect(router.getSelectedProjectId()).toBe('project-1');
    expect(router.getSelectedDocId()).toBe('doc-a');
    expect(router.getSelectedHistoryVersionId()).toBe('version-1');
  });

  it('returns null selected doc when route points to missing metadata', async () => {
    const router = await loadRouter();

    await router.navigateToDoc('project-1', 'missing-doc');

    expect(router.getSelectedDocId()).toBe('missing-doc');
    expect(router.getSelectedDoc()).toBeNull();
  });

  it('prefers canonical route when stale metadata changes underneath it', async () => {
    const router = await loadRouter();

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.docs = [{ id: 'doc-b', projectId: 'project-2', title: 'Beta', activePeers: [] }];

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    });
    expect(router.getSelectedProjectId()).toBe('project-1');
    expect(router.getSelectedDocId()).toBe('doc-a');
    expect(router.getSelectedHistoryVersionId()).toBe('version-1');
  });

  it('ignores conflicting legacy ui mirror state once a canonical route is claimed', async () => {
    const router = await loadRouter();

    await router.navigateToDoc('project-1', 'doc-a');
    setLegacyUiRoute({
      view: 'project-overview',
      activeProjectId: 'project-9',
      historyReviewSessionId: 'version-9',
    });

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
    expect(router.getSelectedProjectId()).toBe('project-1');
    expect(router.getSelectedDocId()).toBe('doc-a');
  });

  it('does not write legacy ui route mirror fields during navigation', async () => {
    const router = await loadRouter();

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');

    expect('view' in mockState.uiState).toBe(false);
    expect('activeProjectId' in mockState.uiState).toBe(false);
    expect('historyReviewSessionId' in mockState.uiState).toBe(false);
  });

  it('keeps settings return route stable while metadata drifts', async () => {
    const router = await loadRouter();

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    router.navigateToSettings();
    mockState.docs = [{ id: 'doc-b', projectId: 'project-2', title: 'Beta', activePeers: [] }];

    await router.navigateBackFromSettings();

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    });
  });

  it('navigates to a project and hydrates it when needed', async () => {
    const router = await loadRouter();

    await router.navigateToProject('project-1');

    expect(mockState.closeEditorSession).toHaveBeenCalledTimes(1);
    expect(mockState.loadProjectDocs).toHaveBeenCalledWith('project-1', { connectPeers: true });
    expect(mockState.tauriApi.openProject).not.toHaveBeenCalled();
    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-1' });
  });

  it('reuses an already hydrated project without reloading docs', async () => {
    const router = await loadRouter();
    mockState.hydratedProjects.add('project-1');

    await router.navigateToProject('project-1');

    expect(mockState.loadProjectDocs).not.toHaveBeenCalled();
    expect(mockState.tauriApi.openProject).toHaveBeenCalledWith('project-1', true);
  });

  it('navigates to history by opening the doc first when needed', async () => {
    const router = await loadRouter();

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');

    expect(mockState.openEditorSession).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(mockState.previewVersion).toHaveBeenCalledWith('project-1', 'doc-a', 'version-1');
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    });
  });

  it('selects history on the current doc without reopening the editor session', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');
    mockState.openEditorSession.mockClear();

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');

    expect(mockState.openEditorSession).not.toHaveBeenCalled();
    expect(mockState.previewVersion).toHaveBeenCalledWith('project-1', 'doc-a', 'version-1');
  });

  it('claims the live doc route immediately before session open finishes', async () => {
    const router = await loadRouter();
    let resolveOpen!: () => void;
    mockState.openEditorSession.mockImplementationOnce(
      async () => new Promise<undefined>((resolve) => { resolveOpen = () => resolve(undefined); }),
    );

    const navPromise = router.navigateToDoc('project-1', 'doc-a');

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
    expect(router.getSelectedDocId()).toBe('doc-a');

    resolveOpen();
    await navPromise;
  });

  it('keeps the latest selected doc route during overlapping opens', async () => {
    const router = await loadRouter();
    let resolveA!: () => void;
    let resolveB!: () => void;
    mockState.openEditorSession
      .mockImplementationOnce(async () => new Promise<undefined>((resolve) => { resolveA = () => resolve(undefined); }))
      .mockImplementationOnce(async () => new Promise<undefined>((resolve) => { resolveB = () => resolve(undefined); }));

    const navA = router.navigateToDoc('project-1', 'doc-a');
    const navB = router.navigateToDoc('project-1', 'doc-b');

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-b',
      mode: 'live',
    });

    resolveB();
    resolveA();
    await Promise.all([navA, navB]);
  });

  it('does not let a stale navigateToHistory overwrite a newer live doc selection', async () => {
    const router = await loadRouter();
    let resolveOpen!: () => void;
    mockState.openEditorSession.mockImplementationOnce(
      async () => new Promise<undefined>((resolve) => { resolveOpen = () => resolve(undefined); }),
    );

    const historyNav = router.navigateToHistory('project-1', 'doc-a', 'version-1');
    await router.navigateToDoc('project-1', 'doc-b');
    resolveOpen();
    await historyNav;

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-b',
      mode: 'live',
    });
  });

  it('keeps the claimed live doc route when opening that doc fails', async () => {
    const router = await loadRouter();
    mockState.openEditorSession.mockImplementationOnce(async () => {
      throw new Error('open failed');
    });

    await expect(router.navigateToDoc('project-1', 'doc-a')).rejects.toThrow('open failed');

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
  });

  it('navigates back to live without clearing the active doc route', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();

    router.navigateBackToLive();

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
  });

  it('exits history before navigating to a project', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();

    await router.navigateToProject('project-2');

    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-2' });
  });

  it('exits history before opening a different doc session', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();

    await router.navigateToDoc('project-1', 'doc-b');

    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
    expect(mockState.openEditorSession).toHaveBeenCalledWith('project-1', 'doc-b');
  });

  it('restores a version and returns to the live doc route after success', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');

    await router.restoreHistoryVersion('project-1', 'doc-a', 'version-1');

    expect(mockState.restoreVersionData).toHaveBeenCalledWith('project-1', 'doc-a', 'version-1');
    expect(mockState.reloadActiveSession).toHaveBeenCalledTimes(1);
    expect(mockState.loadVersions).toHaveBeenCalledWith('doc-a');
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
  });

  it('stays in history when restore fails', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();
    mockState.restoreVersionData.mockImplementationOnce(async () => {
      throw new Error('restore failed');
    });

    await expect(router.restoreHistoryVersion('project-1', 'doc-a', 'version-1')).rejects.toThrow('restore failed');

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    });
    expect(mockState.clearVersionPreview).not.toHaveBeenCalled();
  });

  it('navigateHistoryNewer exits to live when already at the newest version', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();
    mockState.getAdjacentSignificantVersionId.mockImplementationOnce(() => null);

    await router.navigateHistoryNewer('project-1', 'doc-a');

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
  });

  it('does not exit a newer history route when an older restore resolves late', async () => {
    const router = await loadRouter();
    let resolveRestore!: () => void;
    mockState.restoreVersionData.mockImplementationOnce(
      async () => new Promise<undefined>((resolve) => { resolveRestore = () => resolve(undefined); }),
    );

    await router.navigateToHistory('project-1', 'doc-a', 'version-1');

    const restorePromise = router.restoreHistoryVersion('project-1', 'doc-a', 'version-1');
    await router.navigateToHistory('project-1', 'doc-a', 'version-2');
    resolveRestore();
    await restorePromise;

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-2',
    });
  });

  it('restores the previous history route when leaving settings', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');

    router.navigateToSettings();
    expect(router.getWorkspaceRoute()).toEqual({ kind: 'settings' });

    await router.navigateBackFromSettings();

    expect(mockState.previewVersion).toHaveBeenCalledWith('project-1', 'doc-a', 'version-1');
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    });
  });

  it('restores a previous live doc route when leaving settings', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');

    router.navigateToSettings();
    await router.navigateBackFromSettings();

    expect(mockState.openEditorSession).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
  });

  it('restores a previous project route when leaving settings', async () => {
    const router = await loadRouter();
    await router.navigateToProject('project-1');

    router.navigateToSettings();
    await router.navigateBackFromSettings();

    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-1' });
  });

  it('falls back to null route when leaving settings without a saved route', async () => {
    const router = await loadRouter();
    router.navigateToSettings();

    await router.navigateBackFromSettings();

    expect(router.getWorkspaceRoute()).toBeNull();
  });

  it('clears a deleted active project route', async () => {
    const router = await loadRouter();
    await router.navigateToProject('project-1');

    router.handleDeletedProject('project-1');

    expect(router.getWorkspaceRoute()).toBeNull();
  });

  it('clears settings return context when that project is deleted', async () => {
    const router = await loadRouter();
    await router.navigateToProject('project-1');
    router.navigateToSettings();

    router.handleDeletedProject('project-1');

    expect(router.getWorkspaceContextRoute()).toBeNull();
    expect(router.getSelectedProjectId()).toBeNull();
  });

  it('falls back to the project route when the selected live doc is deleted', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');
    mockState.closeEditorSession.mockClear();

    router.handleDeletedDoc('project-1', 'doc-a');

    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-1' });
    expect(router.getSelectedDocId()).toBeNull();
    expect(mockState.closeEditorSession).not.toHaveBeenCalled();
  });

  it('falls back to the project route and clears preview when the selected history doc is deleted', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();

    router.handleDeletedDoc('project-1', 'doc-a');

    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-1' });
    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
  });

  it('rewrites settings return route to the project when the saved doc is deleted', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');
    router.navigateToSettings();

    router.handleDeletedDoc('project-1', 'doc-a');

    expect(router.getWorkspaceContextRoute()).toEqual({ kind: 'project', projectId: 'project-1' });
  });

  it('rewrites the selected live doc route to the moved destination doc', async () => {
    const router = await loadRouter();
    mockState.docs.push({ id: 'doc-c', projectId: 'project-2', title: 'Gamma', activePeers: [] });
    await router.navigateToDoc('project-1', 'doc-a');

    router.handleMovedDoc({
      fromProjectId: 'project-1',
      fromDocId: 'doc-a',
      toProjectId: 'project-2',
      toDocId: 'doc-c',
    });

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-2',
      docId: 'doc-c',
      mode: 'live',
    });
  });

  it('rewrites history and settings routes to the moved destination doc in live mode', async () => {
    const router = await loadRouter();
    mockState.docs.push({ id: 'doc-c', projectId: 'project-2', title: 'Gamma', activePeers: [] });
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    router.navigateToSettings();
    mockState.clearVersionPreview.mockClear();

    router.handleMovedDoc({
      fromProjectId: 'project-1',
      fromDocId: 'doc-a',
      toProjectId: 'project-2',
      toDocId: 'doc-c',
    });

    expect(router.getWorkspaceContextRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-2',
      docId: 'doc-c',
      mode: 'live',
    });
    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
  });

  it('rewrites the actively viewed history doc to the moved destination live doc', async () => {
    const router = await loadRouter();
    mockState.docs.push({ id: 'doc-c', projectId: 'project-2', title: 'Gamma', activePeers: [] });
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();

    router.handleMovedDoc({
      fromProjectId: 'project-1',
      fromDocId: 'doc-a',
      toProjectId: 'project-2',
      toDocId: 'doc-c',
    });

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-2',
      docId: 'doc-c',
      mode: 'live',
    });
    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
  });

  it('ignores delete and move events for unrelated docs', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');

    router.handleDeletedDoc('project-1', 'doc-b');
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });

    router.handleMovedDoc({
      fromProjectId: 'project-1',
      fromDocId: 'doc-b',
      toProjectId: 'project-2',
      toDocId: 'doc-c',
    });
    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
  });

  it('reconciles a missing selected doc to its project route and closes the stale session', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');
    mockState.closeEditorSession.mockClear();
    mockState.docs = [
      { id: 'doc-b', projectId: 'project-1', title: 'Beta', activePeers: [] },
    ];

    router.reconcileMissingSelectedDoc();

    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-1' });
    expect(mockState.closeEditorSession).toHaveBeenCalledTimes(1);
  });

  it('reconciles a missing selected history doc to its project route and clears preview', async () => {
    const router = await loadRouter();
    await router.navigateToHistory('project-1', 'doc-a', 'version-1');
    mockState.clearVersionPreview.mockClear();
    mockState.closeEditorSession.mockClear();
    mockState.docs = [
      { id: 'doc-b', projectId: 'project-1', title: 'Beta', activePeers: [] },
    ];

    router.reconcileMissingSelectedDoc();

    expect(router.getWorkspaceRoute()).toEqual({ kind: 'project', projectId: 'project-1' });
    expect(mockState.clearVersionPreview).toHaveBeenCalledTimes(1);
    expect(mockState.closeEditorSession).toHaveBeenCalledTimes(1);
  });

  it('reconciles a missing selected doc to null when its project no longer exists', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');
    mockState.docs = [];
    mockState.projects = mockState.projects.filter((project) => project.id !== 'project-1');

    router.reconcileMissingSelectedDoc();

    expect(router.getWorkspaceRoute()).toBeNull();
  });

  it('does not reconcile away a selected doc that is pending a move', async () => {
    const router = await loadRouter();
    await router.navigateToDoc('project-1', 'doc-a');
    mockState.docs = [];

    router.beginMovedDoc({
      fromProjectId: 'project-1',
      fromDocId: 'doc-a',
      toProjectId: 'project-2',
      toDocId: 'doc-c',
    });
    router.reconcileMissingSelectedDoc();

    expect(router.getWorkspaceRoute()).toEqual({
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    });
  });
});
