import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import App from './App.svelte';

const mockState = vi.hoisted(() => ({
  appSessionState: { ready: true, error: null as string | null },
  editorSessionState: { loading: false },
  uiState: { sidebarOpen: true, rightSidebarOpen: false, quickOpenVisible: false },
  projectState: { projects: [{ id: 'project-1', name: 'Project 1' }] },
  inviteState: { shareDialogOpen: false, joinDialogOpen: false },
  route: null as
    | null
    | { kind: 'project'; projectId: string }
    | { kind: 'doc'; projectId: string; docId: string; mode: 'live' },
  selectedDoc: null as null | { id: string; projectId: string; title: string },
  reconcileMissingSelectedDoc: vi.fn(),
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
}));

vi.mock('../state/projects.svelte.js', () => ({
  projectState: mockState.projectState,
}));

vi.mock('../state/invite.svelte.js', () => ({
  inviteState: mockState.inviteState,
}));

vi.mock('../utils/platform.js', () => ({
  isMac: false,
}));

vi.mock('../navigation/workspace-router.svelte.js', () => ({
  getWorkspaceRoute: () => mockState.route,
  getSelectedDoc: () => mockState.selectedDoc,
  isProjectRoute: (route: unknown) => !!route && (route as { kind?: string }).kind === 'project',
  reconcileMissingSelectedDoc: mockState.reconcileMissingSelectedDoc,
}));

vi.mock('./sidebar/Sidebar.svelte', () => import('./__test_mocks__/StubSidebarChild.svelte'));
vi.mock('./rightsidebar/RightSidebar.svelte', () => import('./__test_mocks__/StubSidebarChild.svelte'));
vi.mock('./UpdateBanner.svelte', () => import('./__test_mocks__/StubSidebarChild.svelte'));
vi.mock('./editor/ProjectOverview.svelte', () => import('./__test_mocks__/StubProjectOverview.svelte'));

describe('App fallback behavior', () => {
  beforeEach(() => {
    mockState.route = null;
    mockState.selectedDoc = null;
    mockState.editorSessionState.loading = false;
    mockState.reconcileMissingSelectedDoc.mockReset();
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
});
