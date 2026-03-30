import { beforeEach, describe, expect, it, vi } from 'vitest';

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const tauriApiMock = vi.hoisted(() => ({
  listFiles: vi.fn(async () => []),
  getUnseenDocs: vi.fn(async () => []),
  openProject: vi.fn(async () => undefined),
  deleteNote: vi.fn(async () => undefined),
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: tauriApiMock,
}));

async function loadFreshModules() {
  vi.resetModules();
  const documents = await import('./documents.svelte.js');

  documents.documentState.docs = [
    {
      id: 'doc-a',
      projectId: 'project-1',
      path: 'a.md',
      title: 'a',
      syncStatus: 'local-only',
      wordCount: 0,
      activePeers: [],
      hasUnread: false,
    },
  ];

  return { documents };
}

describe('documents state', () => {
  beforeEach(() => {
    tauriApiMock.listFiles.mockClear();
    tauriApiMock.getUnseenDocs.mockClear();
    tauriApiMock.openProject.mockClear();
    tauriApiMock.deleteNote.mockClear();
  });

  it('getDocById returns document metadata by id', async () => {
    const { documents } = await loadFreshModules();

    expect(documents.getDocById('doc-a')?.projectId).toBe('project-1');
  });

  it('removeDoc removes the document from the catalog', async () => {
    const { documents } = await loadFreshModules();

    documents.removeDoc('doc-a');

    expect(documents.documentState.docs).toHaveLength(0);
  });

  it('connects peers for a hydrated project without reloading docs', async () => {
    const { documents } = await loadFreshModules();
    documents.documentState.hydratedProjectIds = ['project-1'];

    await documents.loadProjectDocs('project-1', { connectPeers: true });

    expect(tauriApiMock.openProject).toHaveBeenCalledWith('project-1', true);
    expect(tauriApiMock.listFiles).not.toHaveBeenCalled();
  });

  it('queues a force reload requested while an earlier force reload is in flight', async () => {
    const gate = deferred<Array<{ id: string; path: string; created: string }>>();
    (tauriApiMock.listFiles as any)
      .mockImplementationOnce(() => gate.promise as any)
      .mockImplementationOnce(async () => [{ id: 'doc-b', path: 'b.md', created: 'later' }]);
    (tauriApiMock.getUnseenDocs as any)
      .mockImplementationOnce(async () => [])
      .mockImplementationOnce(async () => []);

    const { documents } = await loadFreshModules();
    const firstLoad = documents.loadProjectDocs('project-1', { force: true });
    const secondLoad = documents.loadProjectDocs('project-1', { force: true });

    gate.resolve([{ id: 'doc-a', path: 'a.md', created: 'now' }]);
    await Promise.all([firstLoad, secondLoad]);

    expect(tauriApiMock.openProject).toHaveBeenCalledTimes(2);
    expect(tauriApiMock.listFiles).toHaveBeenCalledTimes(2);
    expect(documents.documentState.docs.some((doc) => doc.id === 'doc-b')).toBe(true);
  });
});
