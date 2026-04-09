import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mockState = vi.hoisted(() => ({
  editorCommands: {
    setContent: vi.fn(),
  },
  editorInstance: {
    commands: {
      setContent: vi.fn(),
    },
    getJSON: vi.fn(() => ({ type: 'doc', content: [] })),
    setEditable: vi.fn(),
    destroy: vi.fn(),
    state: { selection: { from: 1, to: 1 } },
    view: { dispatch: vi.fn() },
  },
  editorSessionState: {
    projectId: 'project-1' as string | null,
    docId: 'doc-a' as string | null,
    text: 'Current live text',
    document: { schemaVersion: 2, doc: { type: 'doc', content: [] } },
    canEdit: true,
    loading: false,
    flushing: false,
    lastError: null as string | null,
    revision: 1,
  },
  getProjectPeerById: vi.fn(() => null),
  getRemoteCursorsForDoc: vi.fn(() => []),
  updateRemotePresence: vi.fn(),
  setLocalCursorPresence: vi.fn(),
  syncState: { connection: 'connected' as 'connected' | 'slow' | 'offline' | 'local' },
  versionState: { supported: true },
  versionReviewState: {
    previewLoading: false,
    previewError: null as string | null,
    previewText: null as string | null,
    status: 'ready' as 'idle' | 'loading' | 'ready' | 'error',
    viewMode: 'snapshot',
  },
  activeDoc: {
    id: 'doc-a',
    projectId: 'project-1',
    path: 'ideas.md',
    title: 'ideas',
    syncStatus: 'synced',
    wordCount: 2,
    activePeers: [],
    hasUnread: false,
  },
  route: {
    kind: 'doc',
    projectId: 'project-1',
    docId: 'doc-a',
    mode: 'history',
    versionId: 'version-2',
  } as {
    kind: 'doc';
    projectId: string;
    docId: string;
    mode: 'history' | 'live';
    versionId?: string;
  },
  diffBlocks: [{ type: 'changed', content: 'Old text', lineStart: 1, lineEnd: 1 }],
  navigateBackToLive: vi.fn(),
}));

vi.mock('../../editor/setup.js', () => ({
  createEditor: vi.fn(() => mockState.editorInstance),
  createEditorAdapter: vi.fn(() => ({
    getEditor: () => mockState.editorInstance,
    updateRemotePresence: mockState.updateRemotePresence,
    detach: vi.fn(),
  })),
  editorToDocument: vi.fn(() => mockState.editorSessionState.document),
  textToEditorHtml: vi.fn((text: string) => `<p>${text}</p>`),
  updateRemotePresence: mockState.updateRemotePresence,
}));

vi.mock('../../editor/schema.js', () => ({
  createDocumentFromPlainText: vi.fn((text: string) => ({
    schemaVersion: 2,
    doc: { type: 'doc', content: [{ type: 'paragraph', content: text ? [{ type: 'text', text }] : [] }] },
  })),
}));

vi.mock('../../state/presence.svelte.js', () => ({
  getProjectPeerById: mockState.getProjectPeerById,
  getRemoteCursorsForDoc: mockState.getRemoteCursorsForDoc,
}));

vi.mock('../../state/sync.svelte.js', () => ({
  syncState: mockState.syncState,
}));

vi.mock('../../session/editor-session.svelte.js', () => ({
  editorSessionState: mockState.editorSessionState,
  bindEditorAdapter: vi.fn(),
  handleBoundEditorChange: vi.fn(),
  setLocalCursorPresence: mockState.setLocalCursorPresence,
  updateEditorDocument: vi.fn(),
  reloadActiveSession: vi.fn(async () => undefined),
}));

vi.mock('../../state/versions.svelte.js', () => ({
  versionState: mockState.versionState,
}));

vi.mock('../../state/version-review.svelte.js', () => ({
  versionReviewState: mockState.versionReviewState,
  showSavePrompt: vi.fn(),
}));

vi.mock('../../utils/diff.js', () => ({
  computeBlockDiff: vi.fn(() => mockState.diffBlocks),
  getDiffBlockTargetId: vi.fn((block: { type: string; lineStart: number }, index: number) => `${block.type}-${block.lineStart}-${index}`),
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  getSelectedDoc: () => mockState.activeDoc,
  getWorkspaceRoute: () => mockState.route,
  isHistoryRoute: (route: typeof mockState.route | null) => route?.kind === 'doc' && route.mode === 'history',
  navigateBackToLive: mockState.navigateBackToLive,
}));

vi.mock('./TimelineScrubber.svelte', () => import('../__test_mocks__/StubSidebarChild.svelte'));

vi.mock('./SaveVersionBar.svelte', () => import('../__test_mocks__/StubSidebarChild.svelte'));

async function loadEditorPane() {
  return (await import('./EditorPane.svelte')).default;
}

describe('EditorPane history view', () => {
  beforeEach(() => {
    mockState.editorInstance.commands.setContent.mockReset();
    mockState.editorInstance.getJSON.mockReset();
    mockState.editorInstance.getJSON.mockReturnValue({ type: 'doc', content: [] });
    mockState.editorInstance.setEditable.mockReset();
    mockState.editorInstance.destroy.mockReset();
    mockState.editorSessionState.text = 'Current live text';
    mockState.editorSessionState.document = { schemaVersion: 2, doc: { type: 'doc', content: [] } };
    mockState.editorSessionState.canEdit = true;
    mockState.versionReviewState.previewLoading = false;
    mockState.versionReviewState.previewError = null;
    mockState.versionReviewState.previewText = 'Older saved text';
    mockState.versionReviewState.status = 'ready';
    mockState.getProjectPeerById.mockReset();
    mockState.getProjectPeerById.mockReturnValue(null);
    mockState.getRemoteCursorsForDoc.mockReset();
    mockState.getRemoteCursorsForDoc.mockReturnValue([]);
    mockState.updateRemotePresence.mockReset();
    mockState.setLocalCursorPresence.mockReset();
    mockState.versionReviewState.viewMode = 'snapshot';
    mockState.route = {
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-2',
    };
    mockState.diffBlocks = [{ type: 'changed', content: 'Old text', lineStart: 1, lineEnd: 1 }];
  });

  afterEach(() => {
    cleanup();
  });

  it('keeps the editor content visible while a history preview is loading', async () => {
    mockState.versionReviewState.previewLoading = true;
    mockState.versionReviewState.previewText = null;
    mockState.versionReviewState.status = 'loading';

    const EditorPane = await loadEditorPane();
    const { container } = render(EditorPane);

    expect(container.querySelector('.editor-content-wrap.editor-hidden')).toBeNull();
  });

  it('defaults to snapshot view instead of forcing diff rendering', async () => {
    const EditorPane = await loadEditorPane();
    const { getByTestId, queryByTestId } = render(EditorPane);

    expect(queryByTestId('history-diff-view')).toBeNull();
    expect(getByTestId('history-snapshot-view')).toBeTruthy();
    expect(screen.queryByText(/viewing history · read only/i)).toBeNull();
  });

  it('shows a history error overlay without dropping back to diff mode', async () => {
    mockState.versionReviewState.previewError = 'version preview unavailable';
    mockState.versionReviewState.previewText = null;
    mockState.versionReviewState.status = 'error';

    const EditorPane = await loadEditorPane();
    const { container, getByTestId } = render(EditorPane);

    expect(container.textContent).toContain('version preview unavailable');
    expect(getByTestId('history-error-view')).toBeTruthy();
  });

  it('prefers the history error overlay over stale preview text', async () => {
    mockState.versionReviewState.previewError = 'version preview unavailable';
    mockState.versionReviewState.previewText = 'Older saved text';
    mockState.versionReviewState.status = 'error';

    const EditorPane = await loadEditorPane();
    const { queryByTestId, getByTestId } = render(EditorPane);

    expect(getByTestId('history-error-view')).toBeTruthy();
    expect(queryByTestId('history-snapshot-view')).toBeNull();
  });

  it('renders diff blocks when diff mode is selected', async () => {
    mockState.versionReviewState.viewMode = 'diff';

    const EditorPane = await loadEditorPane();
    const { container, getByTestId } = render(EditorPane);

    expect(getByTestId('history-diff-view')).toBeTruthy();
    expect(container.textContent).toContain('Old text');
  });

  it('renders the identical diff state when no diff blocks exist', async () => {
    mockState.versionReviewState.viewMode = 'diff';
    mockState.diffBlocks = [{ type: 'unchanged', content: 'Same text', lineStart: 1, lineEnd: 1 }];

    const EditorPane = await loadEditorPane();
    const { container, getByTestId } = render(EditorPane);

    expect(getByTestId('history-diff-identical')).toBeTruthy();
    expect(container.textContent).toContain('this version matches the current live note');
  });

  it('renders the selected snapshot body in snapshot mode', async () => {
    mockState.versionReviewState.previewText = 'Snapshot body';

    const EditorPane = await loadEditorPane();
    const { container, getByTestId } = render(EditorPane);

    expect(getByTestId('history-snapshot-view')).toBeTruthy();
    expect(container.textContent).toContain('Snapshot body');
  });

  it('sets the editor read-only in history mode and editable in live mode', async () => {
    const EditorPane = await loadEditorPane();

    render(EditorPane);
    expect(mockState.editorInstance.setEditable).toHaveBeenCalledWith(false);

    cleanup();
    mockState.editorInstance.setEditable.mockReset();
    mockState.route = {
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    };

    render(EditorPane);
    expect(mockState.editorInstance.setEditable).toHaveBeenCalledWith(true);
  });

  it('pushes remote cursor overlays into the editor in live mode', async () => {
    mockState.route = {
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    };
    mockState.getRemoteCursorsForDoc.mockReturnValue([
      {
        projectId: 'project-1',
        peerId: 'peer-2',
        alias: 'alice',
        cursorColor: '#00aa00',
        sessionId: 'session-b',
        seq: 2,
        docId: 'doc-a',
        from: 4,
        to: 8,
        lastActive: Date.now(),
      },
    ] as any);

    const EditorPane = await loadEditorPane();
    render(EditorPane);

    expect(mockState.updateRemotePresence).toHaveBeenCalledWith(
      expect.arrayContaining([expect.objectContaining({ peerId: 'peer-2', docId: 'doc-a' })]),
    );
  });

  it('does not replace live editor content via setContent on the steady-state path', async () => {
    mockState.route = {
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'live',
    };

    const EditorPane = await loadEditorPane();
    render(EditorPane);

    expect(mockState.editorInstance.commands.setContent).not.toHaveBeenCalled();
  });

  it('keeps showing the previous snapshot overlay while loading a newer version', async () => {
    mockState.versionReviewState.previewLoading = true;
    mockState.versionReviewState.status = 'loading';
    mockState.versionReviewState.previewText = 'Older saved text';

    const EditorPane = await loadEditorPane();
    const { getByTestId } = render(EditorPane);

    expect(getByTestId('history-snapshot-view')).toBeTruthy();
    expect(screen.getByText(/older saved text/i)).toBeTruthy();
  });

  it('scrolls to the selected diff region from the minibar', async () => {
    mockState.versionReviewState.viewMode = 'diff';

    const scrollTo = vi.fn();
    const originalScrollTo = HTMLElement.prototype.scrollTo;
    HTMLElement.prototype.scrollTo = scrollTo;

    try {
      const EditorPane = await loadEditorPane();
      const { container } = render(EditorPane);

      const scrollRoot = container.querySelector('[data-testid="history-diff-view"]') as HTMLDivElement;
      const target = container.querySelector('[data-diff-target="changed-1-0"]') as HTMLDivElement;
      scrollRoot.getBoundingClientRect = () => ({
        x: 0, y: 0, top: 100, left: 0, right: 500, bottom: 700, width: 500, height: 600,
        toJSON: () => ({}),
      });
      target.getBoundingClientRect = () => ({
        x: 0, y: 0, top: 240, left: 0, right: 500, bottom: 320, width: 500, height: 80,
        toJSON: () => ({}),
      });

      await fireEvent.click(screen.getByRole('button', { name: /changed change/i }));

      expect(scrollTo).toHaveBeenCalledWith({ top: 92, behavior: 'smooth' });
    } finally {
      HTMLElement.prototype.scrollTo = originalScrollTo;
    }
  });
});
