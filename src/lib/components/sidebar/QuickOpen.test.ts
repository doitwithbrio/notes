import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import QuickOpen from './QuickOpen.svelte';

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const mockState = vi.hoisted(() => ({
  documentState: {
    docs: [] as Array<{
      id: string;
      projectId: string;
      path: string;
      title: string;
    }>,
    loading: false,
    loadingProjectIds: [] as string[],
  },
  closeQuickOpen: vi.fn(),
  navigateToDoc: vi.fn(),
  consoleError: vi.fn(),
}));

vi.mock('../../state/documents.svelte.js', () => ({
  documentState: mockState.documentState,
}));

vi.mock('../../state/ui.svelte.js', () => ({
  closeQuickOpen: mockState.closeQuickOpen,
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  navigateToDoc: mockState.navigateToDoc,
}));

describe('QuickOpen', () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    mockState.documentState.docs = [
      { id: 'doc-a', projectId: 'project-1', path: 'alpha.md', title: 'Alpha' },
    ];
    mockState.documentState.loading = false;
    mockState.documentState.loadingProjectIds = [];
    mockState.closeQuickOpen.mockReset();
    mockState.navigateToDoc.mockReset();
    mockState.consoleError.mockReset();
    vi.spyOn(console, 'error').mockImplementation(mockState.consoleError);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('routes to the selected note and closes after navigation succeeds', async () => {
    const gate = deferred<void>();
    mockState.navigateToDoc.mockImplementation(() => gate.promise);

    render(QuickOpen);

    const result = screen.getByRole('button', { name: /alpha alpha\.md/i });
    const clickPromise = fireEvent.click(result);

    expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(mockState.closeQuickOpen).not.toHaveBeenCalled();

    gate.resolve();
    await clickPromise;

    await waitFor(() => {
      expect(mockState.closeQuickOpen).toHaveBeenCalledTimes(1);
    });
  });

  it('filters results and opens the highlighted match with Enter', async () => {
    mockState.documentState.docs = [
      { id: 'doc-a', projectId: 'project-1', path: 'alpha.md', title: 'Alpha' },
      { id: 'doc-b', projectId: 'project-1', path: 'beta.md', title: 'Beta' },
    ];

    render(QuickOpen);

    const input = screen.getByPlaceholderText('search notes...');
    await fireEvent.input(input, { target: { value: 'bet' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-1', 'doc-b');
    });
  });

  it('moves selection one item per arrow key and ignores repeated Enter while opening', async () => {
    mockState.documentState.docs = [
      { id: 'doc-a', projectId: 'project-1', path: 'alpha.md', title: 'Alpha' },
      { id: 'doc-b', projectId: 'project-1', path: 'beta.md', title: 'Beta' },
      { id: 'doc-c', projectId: 'project-1', path: 'charlie.md', title: 'Charlie' },
    ];
    const gate = deferred<void>();
    mockState.navigateToDoc.mockImplementation(() => gate.promise);

    render(QuickOpen);

    const input = screen.getByPlaceholderText('search notes...');
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'Enter' });
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockState.navigateToDoc).toHaveBeenCalledTimes(1);
      expect(mockState.navigateToDoc).toHaveBeenCalledWith('project-1', 'doc-b');
    });

    gate.resolve();
  });

  it('closes when the backdrop is clicked', async () => {
    const { container } = render(QuickOpen);

    const backdrop = container.querySelector('.quick-open-backdrop');
    expect(backdrop).toBeTruthy();

    await fireEvent.click(backdrop!);

    expect(mockState.closeQuickOpen).toHaveBeenCalledTimes(1);
  });

  it('stays open when router navigation fails', async () => {
    mockState.navigateToDoc.mockImplementation(async () => {
      throw new Error('open failed');
    });

    render(QuickOpen);

    await fireEvent.click(screen.getByRole('button', { name: /alpha alpha\.md/i }));

    await waitFor(() => {
      expect(mockState.consoleError).toHaveBeenCalled();
    });
    expect(mockState.closeQuickOpen).not.toHaveBeenCalled();
  });
});
