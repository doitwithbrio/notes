import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mockState = vi.hoisted(() => ({
  remoteChangeHandler: null as null | ((payload: { projectId: string; docId: string; mode: string }) => Promise<void> | void),
  tauriApi: {
    getPeerId: vi.fn(async () => 'peer-self'),
    broadcastPresence: vi.fn(async () => undefined),
    onRemoteChange: vi.fn(async (handler: (payload: { projectId: string; docId: string; mode: string }) => Promise<void> | void) => {
      mockState.remoteChangeHandler = handler;
      return () => {};
    }),
    getDocIncremental: vi.fn(async () => new Uint8Array([1, 2, 3])),
    getViewerDocSnapshot: vi.fn(async () => new Uint8Array([4, 5, 6])),
    onSyncStatus: vi.fn(async () => () => {}),
    onPeerStatus: vi.fn(async () => () => {}),
    onPresenceUpdate: vi.fn(async () => () => {}),
    onInviteAcceptStatus: vi.fn(async () => () => {}),
    onProjectEvicted: vi.fn(async () => () => {}),
    listProjectEvictionNotices: vi.fn(async () => []),
  },
  waitForTauriRuntime: vi.fn(async () => true),
  loadAllProjectDocs: vi.fn(async () => undefined),
  getDocById: vi.fn(() => ({ id: 'doc-a', projectId: 'project-1' })),
  markDocUnread: vi.fn(),
  setDocSyncStatus: vi.fn(),
  loadDeviceActorId: vi.fn(async () => undefined),
  scheduleVersionRefresh: vi.fn(),
  getOnlinePeers: vi.fn(() => []),
  hasAnySharedPeers: vi.fn(() => false),
  hydrateProjectPeers: vi.fn(),
  markProjectPeersLoading: vi.fn(),
  setLocalPeerId: vi.fn(),
  updatePeerStatus: vi.fn(),
  updatePresence: vi.fn(),
  expireStalePresence: vi.fn(),
  clearProjectPeersLoading: vi.fn(),
  loadOrder: vi.fn(),
  loadProjects: vi.fn(async () => undefined),
  projectState: { projects: [] as Array<{ id: string }> },
  loadSettings: vi.fn(async () => undefined),
  applySyncState: vi.fn(),
  setPeerCount: vi.fn(),
  setSharedProject: vi.fn(),
  checkForUpdate: vi.fn(async () => undefined),
  getActiveSession: vi.fn(() => null),
  getLocalCursorPresence: vi.fn(() => null),
  isActiveSession: vi.fn(() => false),
  isActiveSessionReadOnly: vi.fn(() => false),
  applyRemoteIncremental: vi.fn(async () => undefined),
  replaceViewerSnapshot: vi.fn(async () => undefined),
  reloadActiveSession: vi.fn(async () => undefined),
  handleInviteAcceptEvent: vi.fn(),
  hydrateInviteStatus: vi.fn(async () => undefined),
  getWorkspaceProjectId: vi.fn(() => null),
  loadProjectTodos: vi.fn(async () => undefined),
  evictProject: vi.fn(async () => undefined),
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: mockState.tauriApi,
}));

vi.mock('../runtime/tauri.js', () => ({
  waitForTauriRuntime: mockState.waitForTauriRuntime,
  TauriRuntimeUnavailableError: class TauriRuntimeUnavailableError extends Error {},
}));

vi.mock('./documents.svelte.js', () => ({
  loadAllProjectDocs: mockState.loadAllProjectDocs,
  getDocById: mockState.getDocById,
  markDocUnread: mockState.markDocUnread,
  setDocSyncStatus: mockState.setDocSyncStatus,
}));

vi.mock('./versions.svelte.js', () => ({
  loadDeviceActorId: mockState.loadDeviceActorId,
  scheduleVersionRefresh: mockState.scheduleVersionRefresh,
}));

vi.mock('./presence.svelte.js', () => ({
  getOnlinePeers: mockState.getOnlinePeers,
  hasAnySharedPeers: mockState.hasAnySharedPeers,
  hydrateProjectPeers: mockState.hydrateProjectPeers,
  markProjectPeersLoading: mockState.markProjectPeersLoading,
  setLocalPeerId: mockState.setLocalPeerId,
  updatePeerStatus: mockState.updatePeerStatus,
  updatePresence: mockState.updatePresence,
  expireStalePresence: mockState.expireStalePresence,
  clearProjectPeersLoading: mockState.clearProjectPeersLoading,
}));

vi.mock('./ordering.svelte.js', () => ({
  loadOrder: mockState.loadOrder,
}));

vi.mock('./projects.svelte.js', () => ({
  loadProjects: mockState.loadProjects,
  projectState: mockState.projectState,
}));

vi.mock('./settings.svelte.js', () => ({
  loadSettings: mockState.loadSettings,
}));

vi.mock('./sync.svelte.js', () => ({
  applySyncState: mockState.applySyncState,
  setPeerCount: mockState.setPeerCount,
  setSharedProject: mockState.setSharedProject,
}));

vi.mock('./updates.svelte.js', () => ({
  checkForUpdate: mockState.checkForUpdate,
}));

vi.mock('../session/editor-session.svelte.js', () => ({
  applyRemoteIncremental: mockState.applyRemoteIncremental,
  getActiveSession: mockState.getActiveSession,
  getLocalCursorPresence: mockState.getLocalCursorPresence,
  isActiveSession: mockState.isActiveSession,
  isActiveSessionReadOnly: mockState.isActiveSessionReadOnly,
  reloadActiveSession: mockState.reloadActiveSession,
  replaceViewerSnapshot: mockState.replaceViewerSnapshot,
}));

vi.mock('./invite.svelte.js', () => ({
  handleInviteAcceptEvent: mockState.handleInviteAcceptEvent,
  hydrateInviteStatus: mockState.hydrateInviteStatus,
}));

vi.mock('../navigation/workspace-router.svelte.js', () => ({
  getWorkspaceProjectId: mockState.getWorkspaceProjectId,
}));

vi.mock('./todos.svelte.js', () => ({
  loadProjectTodos: mockState.loadProjectTodos,
}));
vi.mock('./project-eviction.svelte.js', () => ({
  evictProject: mockState.evictProject,
}));

describe('app session', () => {
  beforeEach(() => {
    mockState.remoteChangeHandler = null;
    mockState.tauriApi.getPeerId.mockClear();
    mockState.tauriApi.broadcastPresence.mockClear();
    mockState.tauriApi.onRemoteChange.mockClear();
    mockState.tauriApi.getDocIncremental.mockClear();
    mockState.tauriApi.getViewerDocSnapshot.mockClear();
    mockState.tauriApi.onProjectEvicted.mockClear();
    mockState.tauriApi.listProjectEvictionNotices.mockClear();
    mockState.loadProjectTodos.mockClear();
    mockState.getDocById.mockClear();
    mockState.getDocById.mockReturnValue({ id: 'doc-a', projectId: 'project-1' });
    mockState.getActiveSession.mockReturnValue(null as any);
    mockState.getLocalCursorPresence.mockReturnValue(null as any);
    mockState.isActiveSession.mockReturnValue(false as any);
    mockState.isActiveSessionReadOnly.mockReturnValue(false as any);
    mockState.applyRemoteIncremental.mockClear();
    mockState.replaceViewerSnapshot.mockClear();
    mockState.reloadActiveSession.mockClear();
    mockState.getWorkspaceProjectId.mockReturnValue(null as any);
    mockState.clearProjectPeersLoading.mockClear();
    mockState.evictProject.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('refreshes project todos when a remote doc change arrives', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const appSession = await import('./app-session.svelte.js');

    await appSession.initializeApp();
    await mockState.remoteChangeHandler?.({ projectId: 'project-1', docId: 'doc-a', mode: 'metadata-only' });

    expect(mockState.loadProjectTodos).toHaveBeenCalledWith('project-1', { force: true });

    appSession.teardownAppSession();
  });

  it('applies incrementals for an active editable session without reloading', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const appSession = await import('./app-session.svelte.js');

    mockState.isActiveSession.mockReturnValue(true as any);
    mockState.isActiveSessionReadOnly.mockReturnValue(false as any);
    await appSession.initializeApp();

    await mockState.remoteChangeHandler?.({
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'incremental-available',
    });

    expect(mockState.tauriApi.getDocIncremental).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(mockState.applyRemoteIncremental).toHaveBeenCalledWith('project-1', 'doc-a', new Uint8Array([1, 2, 3]));
    expect(mockState.reloadActiveSession).not.toHaveBeenCalled();

    appSession.teardownAppSession();
  });

  it('replaces viewer snapshots for an active read-only session', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const appSession = await import('./app-session.svelte.js');

    mockState.isActiveSession.mockReturnValue(true as any);
    mockState.isActiveSessionReadOnly.mockReturnValue(true as any);
    await appSession.initializeApp();

    await mockState.remoteChangeHandler?.({
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'viewer-snapshot-available',
    });

    expect(mockState.tauriApi.getViewerDocSnapshot).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(mockState.replaceViewerSnapshot).toHaveBeenCalledWith('project-1', 'doc-a', new Uint8Array([4, 5, 6]));
    expect(mockState.reloadActiveSession).not.toHaveBeenCalled();

    appSession.teardownAppSession();
  });

  it('publishes presence immediately on active doc changes and clears on route exit', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const appSession = await import('./app-session.svelte.js');

    mockState.getWorkspaceProjectId.mockReturnValue('project-1' as any);
    await appSession.initializeApp();

    mockState.getActiveSession.mockReturnValue({ projectId: 'project-1', docId: 'doc-a' } as any);
    await vi.advanceTimersByTimeAsync(150);

    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledWith('project-1', 'doc-a', null, null);

    mockState.getActiveSession.mockReturnValue({ projectId: 'project-1', docId: 'doc-b' } as any);
    await vi.advanceTimersByTimeAsync(150);

    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledWith('project-1', 'doc-b', null, null);

    mockState.getWorkspaceProjectId.mockReturnValue('project-2' as any);
    mockState.getActiveSession.mockReturnValue({ projectId: 'project-2', docId: 'doc-c' } as any);
    await vi.advanceTimersByTimeAsync(150);

    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledWith('project-1', null, null, null);
    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledWith('project-2', 'doc-c', null, null);

    mockState.getActiveSession.mockReturnValue(null as any);
    mockState.getWorkspaceProjectId.mockReturnValue(null as any);
    await vi.advanceTimersByTimeAsync(150);

    expect(mockState.tauriApi.broadcastPresence).toHaveBeenLastCalledWith('project-2', null, null, null);

    appSession.teardownAppSession();
  });

  it('heartbeats stable presence without flickering', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const appSession = await import('./app-session.svelte.js');

    mockState.getWorkspaceProjectId.mockReturnValue('project-1' as any);
    mockState.getActiveSession.mockReturnValue({ projectId: 'project-1', docId: 'doc-a' } as any);
    await appSession.initializeApp();

    await vi.advanceTimersByTimeAsync(150);
    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(900);
    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(150);
    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledTimes(2);
    expect(mockState.tauriApi.broadcastPresence).toHaveBeenLastCalledWith('project-1', 'doc-a', null, null);

    appSession.teardownAppSession();
  });

  it('publishes cursor and selection updates on the fast path', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    const appSession = await import('./app-session.svelte.js');

    mockState.getWorkspaceProjectId.mockReturnValue('project-1' as any);
    mockState.getActiveSession.mockReturnValue({ projectId: 'project-1', docId: 'doc-a' } as any);
    mockState.getLocalCursorPresence.mockReturnValue({ cursorPos: 8, selection: [8, 12] } as any);
    await appSession.initializeApp();

    await vi.advanceTimersByTimeAsync(200);

    expect(mockState.tauriApi.broadcastPresence).toHaveBeenCalledWith('project-1', 'doc-a', 8, [8, 12]);

    appSession.teardownAppSession();
  });

  it('evicts projects that were persisted as revoked notices on startup', async () => {
    vi.useFakeTimers();
    mockState.tauriApi.listProjectEvictionNotices.mockResolvedValueOnce([
      { projectId: 'manifest-uuid-9', projectName: 'Secret Project', reason: 'access-revoked' },
    ] as any);
    vi.resetModules();
    const appSession = await import('./app-session.svelte.js');

    await appSession.initializeApp();

    expect(mockState.evictProject).toHaveBeenCalledWith('Secret Project', 'access-revoked', 'Secret Project', 'manifest-uuid-9');

    appSession.teardownAppSession();
  });
});
