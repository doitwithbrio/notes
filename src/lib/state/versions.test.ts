import { beforeEach, describe, expect, it, vi } from 'vitest';

const tauriApiMock = vi.hoisted(() => ({
  getDocVersions: vi.fn(async () => []),
  createVersion: vi.fn(async () => null),
  getVersionText: vi.fn(async () => 'preview'),
  getDeviceActorId: vi.fn(async () => 'actor-id'),
  restoreToVersion: vi.fn(async () => undefined),
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: tauriApiMock,
}));

async function loadFreshModules() {
  vi.resetModules();
  const versions = await import('./versions.svelte.js');
  const review = await import('./version-review.svelte.js');

  versions.versionState.supported = true;
  versions.versionState.versions = [
    {
      id: 'version-2',
      docId: 'doc-a',
      project: 'project-1',
      name: 'otter',
      label: null,
      type: 'auto',
      heads: ['head-2'],
      actor: 'actor-2',
      significance: 'significant',
      createdAt: 2,
      changeCount: 2,
      charsAdded: 5,
      charsRemoved: 1,
      blocksChanged: 1,
      seq: 2,
    },
    {
      id: 'version-1',
      docId: 'doc-a',
      project: 'project-1',
      name: 'seal',
      label: 'checkpoint',
      type: 'named',
      heads: ['head-1'],
      actor: 'actor-1',
      significance: 'significant',
      createdAt: 1,
      changeCount: 1,
      charsAdded: 1,
      charsRemoved: 0,
      blocksChanged: 1,
      seq: 1,
    },
  ];
  review.versionReviewState.previewVersionId = null;
  review.versionReviewState.previewVersionIndex = -1;
  review.versionReviewState.previewText = null;
  review.versionReviewState.previewLoading = false;
  review.versionReviewState.previewError = null;

  return { versions, review };
}

describe('version review state', () => {
  beforeEach(() => {
    tauriApiMock.getDocVersions.mockClear();
    tauriApiMock.createVersion.mockClear();
    tauriApiMock.getVersionText.mockClear();
    tauriApiMock.getDeviceActorId.mockClear();
    tauriApiMock.restoreToVersion.mockClear();
    tauriApiMock.getVersionText.mockImplementation(async () => 'preview');
    tauriApiMock.restoreToVersion.mockImplementation(async () => undefined);
  });

  it('selectVersion previews a version without mutating route state', async () => {
    const { review } = await loadFreshModules();

    await review.previewVersion('project-1', 'doc-a', 'version-2');

    expect(review.versionReviewState.previewVersionId).toBe('version-2');
    expect(review.versionReviewState.previewText).toBe('preview');
  });

  it('clearVersionPreview resets preview state without changing route state', async () => {
    const { review } = await loadFreshModules();
    review.versionReviewState.previewVersionId = 'version-2';
    review.versionReviewState.previewVersionIndex = 0;
    review.versionReviewState.previewText = 'preview';
    review.versionReviewState.previewLoading = true;
    review.versionReviewState.previewError = 'oops';

    review.clearVersionPreview();

    expect(review.versionReviewState.previewVersionId).toBeNull();
    expect(review.versionReviewState.previewVersionIndex).toBe(-1);
    expect(review.versionReviewState.previewText).toBeNull();
    expect(review.versionReviewState.previewLoading).toBe(false);
    expect(review.versionReviewState.previewError).toBeNull();
  });

  it('restoreVersionData does not exit history review', async () => {
    const { review } = await loadFreshModules();

    await review.restoreVersionData('project-1', 'doc-a', 'version-2');

    expect(tauriApiMock.restoreToVersion).toHaveBeenCalledWith('project-1', 'doc-a', 'version-2');
  });

  it('getAdjacentSignificantVersionId returns older, newer, or null', async () => {
    const { review } = await loadFreshModules();

    expect(review.getAdjacentSignificantVersionId('version-2', 'older')).toBe('version-1');
    expect(review.getAdjacentSignificantVersionId('version-1', 'newer')).toBe('version-2');
    expect(review.getAdjacentSignificantVersionId('version-2', 'newer')).toBeNull();
  });

  it('clearVersionPreview prevents stale preview writes from a prior request', async () => {
    const { review } = await loadFreshModules();
    let resolveText!: (value: string) => void;
    tauriApiMock.getVersionText.mockImplementationOnce(
      () => new Promise<string>((resolve) => { resolveText = resolve; }),
    );

    const previewPromise = review.previewVersion('project-1', 'doc-a', 'version-2');
    review.clearVersionPreview();
    resolveText('late preview');
    await previewPromise;

    expect(review.versionReviewState.previewText).toBeNull();
    expect(review.versionReviewState.previewVersionId).toBeNull();
  });

  it('ignores a slower preview response after a newer version is selected', async () => {
    const { review } = await loadFreshModules();
    let resolveFirst!: (value: string) => void;

    tauriApiMock.getVersionText
      .mockImplementationOnce(
        () => new Promise<string>((resolve) => { resolveFirst = resolve; }),
      )
      .mockImplementationOnce(async () => 'new preview');

    const firstPreview = review.previewVersion('project-1', 'doc-a', 'version-2');
    const secondPreview = review.previewVersion('project-1', 'doc-a', 'version-1');
    await secondPreview;

    resolveFirst('old preview');
    await firstPreview;

    expect(review.versionReviewState.previewVersionId).toBe('version-1');
    expect(review.versionReviewState.previewText).toBe('new preview');
  });

  it('versions store keeps only list state and device metadata', async () => {
    const { versions, review } = await loadFreshModules();

    expect('previewVersionId' in versions.versionState).toBe(false);
    expect('savePromptVisible' in versions.versionState).toBe(false);
    expect('diffBlocks' in review.versionReviewState).toBe(false);
  });
});
