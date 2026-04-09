import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import TimelineScrubber from './TimelineScrubber.svelte';

const mockState = vi.hoisted(() => ({
  editorSessionState: {
    projectId: 'project-1' as string | null,
    docId: 'doc-a' as string | null,
    canEdit: true,
  },
  versions: [
    {
      id: 'version-2',
      docId: 'doc-a',
      project: 'project-1',
      type: 'auto',
      name: 'Walrus',
      label: null,
      heads: ['head-2'],
      actor: 'actor-2',
      createdAt: 2,
      changeCount: 2,
      charsAdded: 5,
      charsRemoved: 1,
      blocksChanged: 1,
      significance: 'significant',
      seq: 2,
    },
    {
      id: 'version-1',
      docId: 'doc-a',
      project: 'project-1',
      type: 'named',
      name: 'Seal',
      label: 'checkpoint',
      heads: ['head-1'],
      actor: 'actor-1',
      createdAt: 1,
      changeCount: 1,
      charsAdded: 1,
      charsRemoved: 0,
      blocksChanged: 1,
      significance: 'significant',
      seq: 1,
    },
  ],
  versionReviewState: {
    previewVersionIndex: 0,
    status: 'ready',
    viewMode: 'snapshot',
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
  navigateBackToLive: vi.fn(),
  navigateHistoryNewer: vi.fn(async () => undefined),
  navigateHistoryOlder: vi.fn(async () => undefined),
  navigateToHistory: vi.fn(async () => undefined),
  restoreHistoryVersion: vi.fn(async () => true),
  setVersionViewMode: vi.fn((mode: 'snapshot' | 'diff') => {
    mockState.versionReviewState.viewMode = mode;
  }),
  formatShortTime: vi.fn((value: number) => `t-${value}`),
}));

vi.mock('../../session/editor-session.svelte.js', () => ({
  editorSessionState: mockState.editorSessionState,
}));

vi.mock('../../state/versions.svelte.js', () => ({
  getSignificantVersions: () => mockState.versions,
}));

vi.mock('../../state/version-review.svelte.js', () => ({
  versionReviewState: mockState.versionReviewState,
  setVersionViewMode: mockState.setVersionViewMode,
}));

vi.mock('../../utils/time.js', () => ({
  formatShortTime: mockState.formatShortTime,
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  getHistoryVersionId: () => (mockState.route.mode === 'history' ? mockState.route.versionId ?? null : null),
  getWorkspaceRoute: () => mockState.route,
  isHistoryRoute: (route: typeof mockState.route | null) => route?.kind === 'doc' && route.mode === 'history',
  navigateBackToLive: mockState.navigateBackToLive,
  navigateHistoryNewer: mockState.navigateHistoryNewer,
  navigateHistoryOlder: mockState.navigateHistoryOlder,
  navigateToHistory: mockState.navigateToHistory,
  restoreHistoryVersion: mockState.restoreHistoryVersion,
}));

describe('TimelineScrubber history bar', () => {
  beforeEach(() => {
    mockState.versionReviewState.previewVersionIndex = 0;
    mockState.versionReviewState.status = 'ready';
    mockState.versionReviewState.viewMode = 'snapshot';
    mockState.editorSessionState.canEdit = true;
    mockState.route = {
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-2',
    };
    mockState.navigateBackToLive.mockReset();
    mockState.navigateHistoryNewer.mockReset();
    mockState.navigateHistoryOlder.mockReset();
    mockState.navigateToHistory.mockReset();
    mockState.restoreHistoryVersion.mockReset();
    mockState.restoreHistoryVersion.mockImplementation(async () => true);
    mockState.setVersionViewMode.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it('renders history controls without a live timeline stop', () => {
    render(TimelineScrubber);

    expect(screen.queryByText(/^live$/i)).toBeNull();
  });

  it('shows snapshot and diff view toggles in the history bar', () => {
    render(TimelineScrubber);

    expect(screen.getByRole('button', { name: /snapshot/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /diff/i })).toBeTruthy();
    expect(screen.getByText(/Walrus/i)).toBeTruthy();
    expect(screen.getByText(/Reviewing saved version/i)).toBeTruthy();
  });

  it('keeps selected version identity visible while loading', () => {
    mockState.versionReviewState.status = 'loading';
    render(TimelineScrubber);

    expect(screen.getByText(/Walrus/i)).toBeTruthy();
    expect(screen.getByText(/Loading version/i)).toBeTruthy();
  });

  it('updates the review presentation mode from the history bar toggle', async () => {
    render(TimelineScrubber);

    await fireEvent.click(screen.getByRole('button', { name: /diff/i }));

    expect(mockState.setVersionViewMode).toHaveBeenCalledWith('diff');
  });

  it('opens a selected version when clicking a timeline tick', async () => {
    render(TimelineScrubber);

    await fireEvent.click(screen.getByRole('button', { name: /Seal/i }));

    expect(mockState.navigateToHistory).toHaveBeenCalledWith('project-1', 'doc-a', 'version-1');
  });

  it('navigates to an older version from the history bar arrow', async () => {
    render(TimelineScrubber);

    await fireEvent.click(screen.getByRole('button', { name: /older version/i }));

    expect(mockState.navigateHistoryOlder).toHaveBeenCalledWith('project-1', 'doc-a');
  });

  it('navigates to a newer version when one exists', async () => {
    mockState.route = {
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    };

    render(TimelineScrubber);

    await fireEvent.click(screen.getByRole('button', { name: /newer version/i }));

    expect(mockState.navigateHistoryNewer).toHaveBeenCalledWith('project-1', 'doc-a');
  });

  it('provides a dedicated back to live action outside timeline navigation', async () => {
    render(TimelineScrubber);

    const backToLive = screen.getByRole('button', { name: /back to live/i });
    await fireEvent.click(backToLive);

    expect(mockState.navigateBackToLive).toHaveBeenCalledTimes(1);
  });

  it('shows a restore action when the selected version is ready', () => {
    render(TimelineScrubber);

    expect(screen.getByRole('button', { name: /restore/i })).toBeTruthy();
  });

  it('disables restore while the preview is still loading', () => {
    mockState.versionReviewState.status = 'loading';
    render(TimelineScrubber);

    expect(screen.getByRole('button', { name: /restore/i }).hasAttribute('disabled')).toBe(true);
  });

  it('disables restore for read-only sessions', () => {
    mockState.editorSessionState.canEdit = false;
    render(TimelineScrubber);

    expect(screen.getByRole('button', { name: /restore/i }).hasAttribute('disabled')).toBe(true);
  });

  it('confirms before restoring the selected version', async () => {
    render(TimelineScrubber);

    await fireEvent.click(screen.getByRole('button', { name: /^restore$/i }));
    expect(screen.getByText(/to the live note/i)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: /confirm restore/i }));

    expect(mockState.restoreHistoryVersion).toHaveBeenCalledWith('project-1', 'doc-a', 'version-2');
  });

  it('shows restore failures inline in the history area', async () => {
    mockState.restoreHistoryVersion.mockImplementationOnce(async () => {
      throw new Error('restore failed');
    });

    render(TimelineScrubber);
    await fireEvent.click(screen.getByRole('button', { name: /^restore$/i }));
    await fireEvent.click(screen.getByRole('button', { name: /confirm restore/i }));

    expect(screen.getByText(/restore failed/i)).toBeTruthy();
  });

  it('shows an inline unavailable message when restore cannot run', async () => {
    mockState.restoreHistoryVersion.mockImplementationOnce(async () => false);

    render(TimelineScrubber);
    await fireEvent.click(screen.getByRole('button', { name: /^restore$/i }));
    await fireEvent.click(screen.getByRole('button', { name: /confirm restore/i }));

    expect(screen.getByText(/restore is unavailable right now/i)).toBeTruthy();
  });

  it('shows pending restore state while confirm is in flight', async () => {
    let resolveRestore!: () => void;
    mockState.restoreHistoryVersion.mockImplementationOnce(
      async () => new Promise<boolean>((resolve) => { resolveRestore = () => resolve(true); }),
    );

    render(TimelineScrubber);
    await fireEvent.click(screen.getByRole('button', { name: /^restore$/i }));
    await fireEvent.click(screen.getByRole('button', { name: /confirm restore/i }));

    expect(screen.getByRole('button', { name: /restoring/i }).hasAttribute('disabled')).toBe(true);

    resolveRestore();
  });

  it('clears restore confirmation when switching to another version', async () => {
    render(TimelineScrubber);

    await fireEvent.click(screen.getByRole('button', { name: /^restore$/i }));
    expect(screen.getByText(/to the live note/i)).toBeTruthy();

    cleanup();
    mockState.route = {
      kind: 'doc',
      projectId: 'project-1',
      docId: 'doc-a',
      mode: 'history',
      versionId: 'version-1',
    };
    render(TimelineScrubber);

    expect(screen.queryByText(/restore this version to live/i)).toBeNull();
    expect(screen.getByRole('button', { name: /^restore$/i })).toBeTruthy();
  });
});
