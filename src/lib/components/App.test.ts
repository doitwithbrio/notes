import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import App from './App.svelte';

const mockState = vi.hoisted(() => ({
  appSessionState: { ready: true, error: null as string | null },
  editorSessionState: {
    loading: false,
    lastError: null as string | null,
    lastErrorCode: null as string | null,
    lastErrorDetails: null as Record<string, unknown> | null,
    projectId: null as string | null,
    docId: null as string | null,
  },
  uiState: { sidebarOpen: true, rightSidebarOpen: false, quickOpenVisible: false, revokedProjectNotices: [] as Array<{ projectId: string; backendProjectId: string; projectName: string; reason: string }> },
  projectState: { projects: [{ id: 'project-1', name: 'Project 1', path: 'project-1', shared: true, role: 'owner', accessState: 'owner', canEdit: true, canManagePeers: true, peerCount: 1 }] },
  inviteState: {
    shareDialogOpen: false,
    joinDialogOpen: false,
    pendingJoinResumes: [] as unknown[],
    latestInviteEvent: null as null | Record<string, unknown>,
  },
  route: null as
    | null
    | { kind: 'project'; projectId: string }
    | { kind: 'doc'; projectId: string; docId: string; mode: 'live' },
  selectedDoc: null as null | { id: string; projectId: string; title: string },
  reconcileMissingSelectedDoc: vi.fn(),
  navigateToDoc: vi.fn(async () => undefined),
  navigateToProject: vi.fn(async () => undefined),
  recoverDocFromMarkdown: vi.fn(async () => ({ id: 'doc-a', path: 'ideas.md', fileType: 'note', created: new Date().toISOString() })),
  resumePendingJoins: vi.fn(async () => undefined),
  clearInviteBanner: vi.fn(),
  clearRevokedProjectNotice: vi.fn(),
  dismissProjectEvictionNotice: vi.fn(async () => undefined),
}));

vi.mock('../state/app-session.svelte.js', () => ({
  initializeApp: vi.fn(async () => undefined),
  teardownAppSession: vi.fn(),
  appSessionState: mockState.appSessionState,
}));

vi.mock('../session/editor-session.svelte.js', () => ({
  closeEditorSession: vi.fn(async () => undefined),
  editorSessionState: mockState.editorSessionState,
}));

vi.mock('../state/appearance.svelte.js', () => ({
  teardownAppearance: vi.fn(),
}));

vi.mock('../state/ui.svelte.js', () => ({
  closeQuickOpen: vi.fn(),
  toggleQuickOpen: vi.fn(),
  uiState: mockState.uiState,
  clearRevokedProjectNotice: mockState.clearRevokedProjectNotice,
}));

vi.mock('../state/projects.svelte.js', () => ({
  projectState: mockState.projectState,
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: {
    recoverDocFromMarkdown: mockState.recoverDocFromMarkdown,
    dismissProjectEvictionNotice: mockState.dismissProjectEvictionNotice,
    e2eIsEnabled: vi.fn(async () => false),
    e2eSetNetworkBlocked: vi.fn(async () => undefined),
  },
}));

vi.mock('../state/invite.svelte.js', () => ({
  inviteState: mockState.inviteState,
  resumePendingJoins: mockState.resumePendingJoins,
  clearInviteBanner: mockState.clearInviteBanner,
}));

vi.mock('../utils/platform.js', () => ({
  isMac: false,
}));

vi.mock('../navigation/workspace-router.svelte.js', () => ({
  getWorkspaceRoute: () => mockState.route,
  getSelectedDoc: () => mockState.selectedDoc,
  getSelectedProjectId: () => (mockState.route && mockState.route.kind === 'doc' ? mockState.route.projectId : mockState.route?.kind === 'project' ? mockState.route.projectId : null),
  isProjectRoute: (route: unknown) => !!route && (route as { kind?: string }).kind === 'project',
  navigateToDoc: mockState.navigateToDoc,
  navigateToProject: mockState.navigateToProject,
  reconcileMissingSelectedDoc: mockState.reconcileMissingSelectedDoc,
}));

vi.mock('./sidebar/Sidebar.svelte', () => import('./__test_mocks__/StubSidebarChild.svelte'));
vi.mock('./rightsidebar/RightSidebar.svelte', () => import('./__test_mocks__/StubSidebarChild.svelte'));
vi.mock('./UpdateBanner.svelte', () => import('./__test_mocks__/StubSidebarChild.svelte'));
vi.mock('./editor/ProjectOverview.svelte', () => import('./__test_mocks__/StubProjectOverview.svelte'));

describe('App fallback behavior', () => {
  beforeEach(() => {
    mockState.projectState.projects = [{ id: 'project-1', name: 'Project 1', path: 'project-1', shared: true, role: 'owner', accessState: 'owner', canEdit: true, canManagePeers: true, peerCount: 1 }];
    mockState.route = null;
    mockState.selectedDoc = null;
    mockState.editorSessionState.loading = false;
    mockState.editorSessionState.lastError = null;
    mockState.editorSessionState.lastErrorCode = null;
    mockState.editorSessionState.lastErrorDetails = null;
    mockState.editorSessionState.projectId = null;
    mockState.editorSessionState.docId = null;
    mockState.inviteState.pendingJoinResumes = [];
    mockState.inviteState.latestInviteEvent = null;
    mockState.uiState.revokedProjectNotices = [];
    mockState.reconcileMissingSelectedDoc.mockReset();
    mockState.navigateToDoc.mockReset();
    mockState.navigateToProject.mockReset();
    mockState.recoverDocFromMarkdown.mockReset();
    mockState.resumePendingJoins.mockReset();
    mockState.clearInviteBanner.mockReset();
    mockState.clearRevokedProjectNotice.mockReset();
    mockState.dismissProjectEvictionNotice.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it('requests router reconciliation when a selected doc route has missing metadata', async () => {
    mockState.route = { kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' };

    render(App);

    await waitFor(() => {
      expect(mockState.reconcileMissingSelectedDoc).toHaveBeenCalledTimes(1);
    });
    expect(screen.getByTestId('empty-editor-state')).toBeTruthy();
  });

  it('renders project overview once the route falls back to a project', async () => {
    mockState.route = { kind: 'project', projectId: 'project-1' };

    render(App);

    expect(screen.getByTestId('project-overview').textContent).toContain('project-1');
  });

  it('does not reconcile a missing doc route while the editor session is still loading', async () => {
    mockState.route = { kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' };
    mockState.editorSessionState.loading = true;

    render(App);

    await waitFor(() => {
      expect(screen.getByTestId('editor-loading')).toBeTruthy();
    });
    expect(mockState.reconcileMissingSelectedDoc).not.toHaveBeenCalled();
  });

  it('shows a dedicated failed-open state for a selected doc without a loaded session', async () => {
    mockState.route = { kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' };
    mockState.selectedDoc = { id: 'doc-a', projectId: 'project-1', title: 'ideas' };
    mockState.editorSessionState.lastError = 'Failed to open note';

    render(App);

    expect(screen.getByTestId('editor-open-failed').textContent).toContain('could not open ideas');
    expect(screen.queryByTestId('editor-loading')).toBeNull();
  });

  it('retries opening a selected doc from the failed-open state', async () => {
    mockState.route = { kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' };
    mockState.selectedDoc = { id: 'doc-a', projectId: 'project-1', title: 'ideas' };
    mockState.editorSessionState.lastError = 'Failed to open note';

    render(App);
    await fireEvent.click(screen.getByTestId('editor-open-retry'));

    expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(mockState.navigateToProject).not.toHaveBeenCalled();
  });

  it('shows markdown recovery action for recoverable corruption and opens the recovered copy', async () => {
    mockState.route = { kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' };
    mockState.selectedDoc = { id: 'doc-a', projectId: 'project-1', title: 'ideas' };
    mockState.editorSessionState.lastError = 'Document data is unreadable, but a markdown export is available for recovery';
    mockState.editorSessionState.lastErrorCode = 'DOC_CORRUPTED_RECOVERABLE';
    mockState.editorSessionState.lastErrorDetails = {
      docId: 'doc-a',
      notePath: 'ideas.md',
      suggestedPath: 'ideas (recovered).md',
    };

    render(App);

    expect(screen.getByTestId('editor-open-recover').textContent).toContain('recover note from markdown');
    await fireEvent.click(screen.getByTestId('editor-open-recover'));

    await waitFor(() => {
      expect(mockState.recoverDocFromMarkdown).toHaveBeenCalledWith('project-1', 'doc-a');
      expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    });
  });

  it('keeps recovery available for editors with write access', async () => {
    mockState.route = { kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' };
    mockState.selectedDoc = { id: 'doc-a', projectId: 'project-1', title: 'ideas' };
    mockState.projectState.projects = [{ id: 'project-1', name: 'Project 1', path: 'project-1', shared: true, role: 'editor', accessState: 'editor', canEdit: true, canManagePeers: false, peerCount: 1 }];
    mockState.editorSessionState.lastError = 'Document data is unreadable, but a markdown export is available for recovery';
    mockState.editorSessionState.lastErrorCode = 'DOC_CORRUPTED_RECOVERABLE';
    mockState.editorSessionState.lastErrorDetails = {
      docId: 'doc-a',
      notePath: 'ideas.md',
      suggestedPath: 'ideas.md',
    };

    render(App);

    expect(screen.getByTestId('editor-open-recover').textContent).toContain('recover note from markdown');
  });

  it('explains identity mismatch and hides recovery action when write access is unavailable', async () => {
    mockState.route = { kind: 'doc', projectId: 'project-1', docId: 'doc-a', mode: 'live' };
    mockState.selectedDoc = { id: 'doc-a', projectId: 'project-1', title: 'ideas' };
    mockState.projectState.projects = [{ id: 'project-1', name: 'Project 1', path: 'project-1', shared: true, role: null as any, accessState: 'identity-mismatch', canEdit: false, canManagePeers: false, peerCount: 1 }];
    mockState.editorSessionState.lastError = 'Document data is unreadable, but a markdown export is available for recovery';
    mockState.editorSessionState.lastErrorCode = 'DOC_CORRUPTED_RECOVERABLE';
    mockState.editorSessionState.lastErrorDetails = {
      docId: 'doc-a',
      notePath: 'ideas.md',
      suggestedPath: 'ideas.md',
    };

    render(App);

    expect(screen.queryByTestId('editor-open-recover')).toBeNull();
    expect(screen.getByTestId('editor-open-failed').textContent).toContain('different device identity');
  });

  it('opens the invite banner project using the local project name key', async () => {
    mockState.inviteState.latestInviteEvent = {
      stage: 'completed',
      localProjectName: 'Project 1',
      projectName: 'Remote Project',
      role: 'editor',
    };

    render(App);
    await fireEvent.click(screen.getByText('open project'));

    expect(mockState.navigateToProject).toHaveBeenCalledWith('Project 1');
    expect(mockState.clearInviteBanner).toHaveBeenCalledTimes(1);
  });

  it('retries pending joins for failed invite events even without pending resume entries', async () => {
    mockState.inviteState.latestInviteEvent = {
      stage: 'failed',
      projectName: 'Remote Project',
      error: 'timed out',
    };

    render(App);
    await fireEvent.click(screen.getByText('retry'));

    expect(mockState.resumePendingJoins).toHaveBeenCalledTimes(1);
    expect(mockState.clearInviteBanner).toHaveBeenCalledTimes(1);
  });

  it('shows the resume banner when a pending join session exists', () => {
    mockState.inviteState.pendingJoinResumes = [
      {
        sessionId: 'session-1',
        ownerPeerId: 'owner-peer',
        projectId: 'project-1',
        projectName: 'Remote Project',
        localProjectName: 'Project 1',
        role: 'editor',
        stage: 'payload-staged',
        updatedAt: new Date().toISOString(),
      },
    ];

    render(App);

    expect(screen.getByTestId('invite-banner').textContent).toContain('finishing join for Project 1');
    expect(screen.getByText('retry')).toBeTruthy();
  });

  it('shows and dismisses the revoked project notice banner', async () => {
    mockState.uiState.revokedProjectNotices = [{
      projectId: 'project-1',
      backendProjectId: 'manifest-uuid-1',
      projectName: 'Project 1',
      reason: 'access-revoked',
    }];

    render(App);

    expect(screen.getByTestId('revoked-project-banner').textContent).toContain('Project 1 was removed from this device');
    await fireEvent.click(screen.getByText('dismiss'));

    expect(mockState.clearRevokedProjectNotice).toHaveBeenCalledWith('project-1');
    expect(mockState.dismissProjectEvictionNotice).toHaveBeenCalledWith('manifest-uuid-1');
  });

  it('does not trigger quick open while typing in an input', async () => {
    const ui = await import('../state/ui.svelte.js');
    render(App);

    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();

    await fireEvent.keyDown(input, { key: 'f', ctrlKey: true });

    expect(ui.toggleQuickOpen).not.toHaveBeenCalled();
    input.remove();
  });
});
