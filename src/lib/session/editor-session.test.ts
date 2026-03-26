import * as Automerge from '@automerge/automerge';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

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
  openProject: vi.fn(async () => undefined),
  openDoc: vi.fn<(project: string, docId: string) => Promise<void>>(async () => undefined),
  closeDoc: vi.fn(async () => undefined),
  markDocSeen: vi.fn(async () => undefined),
  getDocBinary: vi.fn<(project: string, docId: string) => Promise<Uint8Array>>(async () => new Uint8Array()),
  applyChanges: vi.fn(async () => undefined),
  saveDoc: vi.fn(async () => undefined),
  createVersion: vi.fn(async () => null),
  getDocVersions: vi.fn(async () => []),
  getVersionText: vi.fn(async () => ''),
  getDeviceActorId: vi.fn(async () => '0123456789abcdef0123456789abcdef'),
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: tauriApiMock,
}));

async function loadFreshModules() {
  vi.resetModules();
  const documents = await import('../state/documents.svelte.js');
  const ui = await import('../state/ui.svelte.js');
  const versions = await import('../state/versions.svelte.js');
  const session = await import('./editor-session.svelte.js');

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
    {
      id: 'doc-b',
      projectId: 'project-1',
      path: 'b.md',
      title: 'b',
      syncStatus: 'local-only',
      wordCount: 0,
      activePeers: [],
      hasUnread: false,
    },
  ];
  documents.documentState.activeDocId = null;
  ui.uiState.view = 'project-overview';
  ui.uiState.activeProjectId = 'project-1';
  ui.uiState.historyReviewSessionId = null;
  versions.versionState.supported = true;
  versions.versionState.deviceActorId = '0123456789abcdef0123456789abcdef';
  versions.versionState.versions = [];
  versions.versionState.activeDocId = null;
  versions.exitVersionReview();

  return { documents, ui, session };
}

function makeBinary(text: string) {
  return Automerge.save(Automerge.from({ schemaVersion: 1, text })) as Uint8Array;
}

beforeEach(() => {
  tauriApiMock.openProject.mockClear();
  tauriApiMock.openDoc.mockClear();
  tauriApiMock.closeDoc.mockClear();
  tauriApiMock.markDocSeen.mockClear();
  tauriApiMock.getDocBinary.mockClear();
  tauriApiMock.applyChanges.mockClear();
  tauriApiMock.saveDoc.mockClear();
  tauriApiMock.createVersion.mockClear();
  tauriApiMock.getDocVersions.mockClear();
  tauriApiMock.getVersionText.mockClear();
  tauriApiMock.getDeviceActorId.mockClear();
  tauriApiMock.openProject.mockImplementation(async () => undefined);
  tauriApiMock.openDoc.mockImplementation(async () => undefined);
  tauriApiMock.closeDoc.mockImplementation(async () => undefined);
  tauriApiMock.markDocSeen.mockImplementation(async () => undefined);
  tauriApiMock.getDocBinary.mockImplementation(async (_project: string, docId: string) => makeBinary(docId));
  tauriApiMock.applyChanges.mockImplementation(async () => undefined);
  tauriApiMock.saveDoc.mockImplementation(async () => undefined);
  tauriApiMock.createVersion.mockImplementation(async () => null);
  tauriApiMock.getDocVersions.mockImplementation(async () => []);
  tauriApiMock.getVersionText.mockImplementation(async () => '');
  tauriApiMock.getDeviceActorId.mockImplementation(async () => '0123456789abcdef0123456789abcdef');
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('editor session navigation behavior', () => {
  it('selects the target doc immediately when opening from project overview', async () => {
    const { documents, ui, session } = await loadFreshModules();
    const binaryGate = deferred<Uint8Array>();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => binaryGate.promise);

    const openPromise = session.openEditorSession('project-1', 'doc-b');

    expect(session.editorSessionState.loading).toBe(true);
    expect(ui.uiState.view).toBe('editor');
    expect(ui.uiState.activeProjectId).toBe('project-1');
    expect(documents.documentState.activeDocId).toBe('doc-b');

    binaryGate.resolve(makeBinary('doc-b body'));
    await openPromise;

    expect(session.editorSessionState.docId).toBe('doc-b');
    expect(session.editorSessionState.text).toBe('doc-b body');
  });

  it('moves sidebar selection to the latest clicked doc while a previous doc is still loading', async () => {
    const { documents, session } = await loadFreshModules();
    const gateA = deferred<Uint8Array>();
    const gateB = deferred<Uint8Array>();

    tauriApiMock.getDocBinary.mockImplementation(async (_project: string, docId: string) => {
      if (docId === 'doc-a') return gateA.promise;
      if (docId === 'doc-b') return gateB.promise;
      return makeBinary(docId);
    });

    const openA = session.openEditorSession('project-1', 'doc-a');
    expect(documents.documentState.activeDocId).toBe('doc-a');
    await vi.waitFor(() => {
      expect(tauriApiMock.getDocBinary).toHaveBeenCalledWith('project-1', 'doc-a');
    });

    const openB = session.openEditorSession('project-1', 'doc-b');
    expect(documents.documentState.activeDocId).toBe('doc-b');

    gateA.resolve(makeBinary('doc-a body'));
    gateB.resolve(makeBinary('doc-b body'));

    await Promise.all([openA, openB]);

    expect(tauriApiMock.closeDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(session.editorSessionState.docId).toBe('doc-b');
    expect(session.editorSessionState.text).toBe('doc-b body');
    expect(documents.documentState.activeDocId).toBe('doc-b');
  });

  it('lets a newer open proceed even if an older load is still hung', async () => {
    const { documents, session } = await loadFreshModules();
    const gateA = deferred<Uint8Array>();
    const gateB = deferred<Uint8Array>();

    tauriApiMock.getDocBinary.mockImplementation(async (_project: string, docId: string) => {
      if (docId === 'doc-a') return gateA.promise;
      if (docId === 'doc-b') return gateB.promise;
      return makeBinary(docId);
    });

    const openA = session.openEditorSession('project-1', 'doc-a');
    await vi.waitFor(() => {
      expect(tauriApiMock.getDocBinary).toHaveBeenCalledWith('project-1', 'doc-a');
    });
    const openB = session.openEditorSession('project-1', 'doc-b');

    gateB.resolve(makeBinary('doc-b body'));
    await openB;

    expect(session.editorSessionState.docId).toBe('doc-b');
    expect(session.editorSessionState.text).toBe('doc-b body');
    expect(documents.documentState.activeDocId).toBe('doc-b');
    expect(tauriApiMock.closeDoc).toHaveBeenCalledWith('project-1', 'doc-a');

    gateA.resolve(makeBinary('doc-a body'));
    await openA;
  });

  it('closes a superseded doc that finishes opening after a newer click wins', async () => {
    const { session } = await loadFreshModules();
    const openGateA = deferred<void>();

    tauriApiMock.openDoc.mockImplementation(async (_project: string, docId: string) => {
      if (docId === 'doc-a') {
        await openGateA.promise;
      }
    });

    const openA = session.openEditorSession('project-1', 'doc-a');
    await vi.waitFor(() => {
      expect(tauriApiMock.openDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    });

    const openB = session.openEditorSession('project-1', 'doc-b');
    await openB;

    openGateA.resolve();
    await openA;
    await vi.waitFor(() => {
      expect(tauriApiMock.closeDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    });
  });
});
