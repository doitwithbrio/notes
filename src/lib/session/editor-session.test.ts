import * as Automerge from '@automerge/automerge';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { loadEditorDocument, type StoredNoteDoc } from '../editor/document-adapter.js';
import type { EditorDocument } from '../editor/schema.js';

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
  getDocIncremental: vi.fn(async () => new Uint8Array()),
  getViewerDocSnapshot: vi.fn(async () => new Uint8Array()),
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
  const todos = await import('../state/todos.svelte.js');
  const versions = await import('../state/versions.svelte.js');
  const review = await import('../state/version-review.svelte.js');
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
  versions.versionState.supported = true;
  versions.versionState.deviceActorId = '0123456789abcdef0123456789abcdef';
  versions.versionState.versions = [];
  versions.versionState.activeDocId = null;
  todos.todoState.todos = [];
  review.clearVersionPreview();

  return { documents, todos, session };
}

function makeBinary(text: string) {
  return Automerge.save(Automerge.from({ schemaVersion: 1, text })) as Uint8Array;
}

function makeGraphBinary() {
  return Automerge.save(Automerge.from({
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [
        {
          type: 'heading',
          attrs: { level: 1 },
          content: [{ type: 'text', text: 'Title' }],
        },
        {
          type: 'paragraph',
          content: [{ type: 'text', text: 'Body copy' }],
        },
      ],
    },
  })) as Uint8Array;
}

function makeGraphTaskBinaryWithoutIds() {
  return Automerge.save(Automerge.from({
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [{
        type: 'task_list',
        content: [{
          type: 'task_item',
          attrs: { checked: false },
          content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Task body' }] }],
        }],
      }],
    },
  })) as Uint8Array;
}

function applyIncremental(binary: Uint8Array, incremental: Uint8Array) {
  const doc = Automerge.load(binary);
  Automerge.loadIncremental(doc, incremental);
  return Automerge.save(doc) as Uint8Array;
}

function updateGraphBodyText(text: string): EditorDocument {
  return {
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [
        {
          type: 'heading',
          attrs: { level: 1 },
          content: [{ type: 'text', text: 'Title' }],
        },
        {
          type: 'paragraph',
          content: [{ type: 'text', text }],
        },
      ],
    },
  };
}

function updateLegacyBodyText(text: string): EditorDocument {
  return {
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          content: [{ type: 'text', text }],
        },
      ],
    },
  };
}

function updateHeadingLevel(level: number): EditorDocument {
  return {
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [
        {
          type: 'heading',
          attrs: { level },
          content: [{ type: 'text', text: 'Title' }],
        },
        {
          type: 'paragraph',
          content: [{ type: 'text', text: 'Body copy' }],
        },
      ],
    },
  };
}

function updateParagraphWithBoldInlineMarkdown(): EditorDocument {
  return {
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          content: [
            { type: 'text', text: 'before ' },
            { type: 'text', text: 'hi', marks: [{ type: 'bold' }] },
            { type: 'text', text: ' after' },
          ],
        },
      ],
    },
  };
}

function updateTaskText(todoId: string, text: string): EditorDocument {
  return {
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [{
        type: 'task_list',
        content: [{
          type: 'task_item',
          attrs: { checked: false, todoId },
          content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
        }],
      }],
    },
  };
}

function getAppliedIncremental() {
  const call = tauriApiMock.applyChanges.mock.calls.at(-1) as unknown as [string, string, Uint8Array] | undefined;
  expect(call).toBeTruthy();
  return call![2];
}

beforeEach(() => {
  tauriApiMock.openProject.mockClear();
  tauriApiMock.openDoc.mockClear();
  tauriApiMock.closeDoc.mockClear();
  tauriApiMock.markDocSeen.mockClear();
  tauriApiMock.getDocBinary.mockClear();
  tauriApiMock.getDocIncremental.mockClear();
  tauriApiMock.getViewerDocSnapshot.mockClear();
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
  tauriApiMock.getDocIncremental.mockImplementation(async () => new Uint8Array());
  tauriApiMock.getViewerDocSnapshot.mockImplementation(async () => new Uint8Array());
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
  it('does not mutate route state when opening from project overview', async () => {
    const { documents, session } = await loadFreshModules();
    const binaryGate = deferred<Uint8Array>();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => binaryGate.promise);

    const openPromise = session.openEditorSession('project-1', 'doc-b');

    expect(session.editorSessionState.loading).toBe(true);

    await vi.waitFor(() => {
      expect(tauriApiMock.getDocBinary).toHaveBeenCalledWith('project-1', 'doc-b');
    });

    binaryGate.resolve(makeBinary('doc-b body'));
    await openPromise;

    expect(session.editorSessionState.docId).toBe('doc-b');
    expect(session.editorSessionState.text).toBe('doc-b body');
  });

  it('keeps document selection untouched while a previous doc is still loading', async () => {
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

    gateA.resolve(makeBinary('doc-a body'));
    gateB.resolve(makeBinary('doc-b body'));

    await Promise.all([openA, openB]);

    expect(tauriApiMock.closeDoc).toHaveBeenCalledWith('project-1', 'doc-a');
    expect(session.editorSessionState.docId).toBe('doc-b');
    expect(session.editorSessionState.text).toBe('doc-b body');
  });

  it('lets a newer open proceed even if an older load is still hung', async () => {
    const { session } = await loadFreshModules();
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

  it('opens canonical graph docs as editable and persists graph updates', async () => {
    const { session } = await loadFreshModules();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => makeGraphBinary());

    await session.openEditorSession('project-1', 'doc-a');

    expect(session.editorSessionState.storageFormat).toBe('graph-v2');
    expect(session.editorSessionState.canEdit).toBe(true);
    expect(session.editorSessionState.text).toBe('Title\n\nBody copy');

    session.updateEditorDocument(updateGraphBodyText('Changed graph body'), 'Title\n\nChanged graph body');
    await session.flushLocalChanges();

    expect(tauriApiMock.applyChanges).toHaveBeenCalledTimes(1);
    expect(session.editorSessionState.text).toBe('Title\n\nChanged graph body');

    const incremental = getAppliedIncremental();
    const updatedBinary = applyIncremental(makeGraphBinary(), incremental);
    const loaded = loadEditorDocument(updatedBinary);

    expect(loaded.sourceSchema).toBe('graph-v2');
    expect(loaded.visibleText).toBe('Title\n\nChanged graph body');
    expect(loaded.editorDocument.doc.content?.[0]?.type).toBe('heading');
    expect(loaded.editorDocument.doc.content?.[1]?.type).toBe('paragraph');
  });

  it('migrates edited legacy docs onto the graph-v2 write path', async () => {
    const { session } = await loadFreshModules();
    const original = makeBinary('Legacy body');
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');

    expect(session.editorSessionState.storageFormat).toBe('legacy-text');
    expect(session.editorSessionState.canEdit).toBe(true);

    session.updateEditorDocument(updateLegacyBodyText('Migrated body'), 'Migrated body');
    await session.flushLocalChanges();

    expect(tauriApiMock.applyChanges).toHaveBeenCalledTimes(1);

    const incremental = getAppliedIncremental();
    const updatedBinary = applyIncremental(original, incremental);
    const loaded = loadEditorDocument(updatedBinary);

    expect(loaded.sourceSchema).toBe('graph-v2');
    expect(loaded.visibleText).toBe('Migrated body');
    expect(loaded.editorDocument.doc.content?.[0]?.type).toBe('paragraph');
  });

  it('persists structural edits even when visible text is unchanged', async () => {
    const { session } = await loadFreshModules();
    const original = makeGraphBinary();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');

    session.updateEditorDocument(updateHeadingLevel(2), 'Title\n\nBody copy');
    await session.flushLocalChanges();

    expect(tauriApiMock.applyChanges).toHaveBeenCalledTimes(1);

    const incremental = getAppliedIncremental();
    const updatedBinary = applyIncremental(original, incremental);
    const loaded = loadEditorDocument(updatedBinary);

    expect(loaded.sourceSchema).toBe('graph-v2');
    expect(loaded.visibleText).toBe('Title\n\nBody copy');
    expect(loaded.editorDocument.doc.content?.[0]?.attrs?.level).toBe(2);
  });

  it('derives persisted text from the editor document instead of trusting stale plain text input', async () => {
    const { session } = await loadFreshModules();
    const original = makeGraphBinary();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');

    session.updateEditorDocument(updateGraphBodyText('Changed graph body'), 'stale plain text');
    await session.flushLocalChanges();

    expect(session.editorSessionState.text).toBe('Title\n\nChanged graph body');

    const incremental = getAppliedIncremental();
    const updatedBinary = applyIncremental(original, incremental);
    const loaded = loadEditorDocument(updatedBinary);
    expect(loaded.visibleText).toBe('Title\n\nChanged graph body');
  });

  it('preserves inline marks through save and reload', async () => {
    const { session } = await loadFreshModules();
    const original = makeBinary('legacy');
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');

    session.updateEditorDocument(updateParagraphWithBoldInlineMarkdown(), 'ignored');
    await session.flushLocalChanges();

    const updatedBinary = applyIncremental(original, getAppliedIncremental());
    const loaded = loadEditorDocument(updatedBinary);
    const paragraph = loaded.editorDocument.doc.content?.[0];

    expect(loaded.visibleText).toBe('before hi after');
    expect(paragraph?.content?.[1]?.marks?.[0]?.type).toBe('bold');
  });

  it('keeps normalized inline todo ids stable across save and reload', async () => {
    const { session, todos } = await loadFreshModules();
    const original = makeGraphTaskBinaryWithoutIds();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');

    const normalizedTodoId = session.editorSessionState.document?.doc.content?.[0]?.content?.[0]?.attrs?.todoId;
    expect(typeof normalizedTodoId).toBe('string');
    expect(todos.todoState.todos.some((todo) => todo.id === `inline:doc-a:${normalizedTodoId}`)).toBe(true);

    session.updateEditorDocument(updateTaskText(String(normalizedTodoId), 'Task body updated'), 'Task body updated');
    await session.flushLocalChanges();

    const firstIncremental = getAppliedIncremental();
    const savedBinary = applyIncremental(original, firstIncremental);
    const reloaded = loadEditorDocument(savedBinary);
    const reloadedTodoId = reloaded.editorDocument.doc.content?.[0]?.content?.[0]?.attrs?.todoId;

    expect(reloadedTodoId).toBe(normalizedTodoId);

    session.updateEditorDocument(updateTaskText(String(reloadedTodoId), 'Task body updated again'), 'Task body updated again');
    await session.flushLocalChanges();

    const secondIncremental = getAppliedIncremental();
    const secondBinary = applyIncremental(savedBinary, secondIncremental);
    const secondReload = loadEditorDocument(secondBinary);

    expect(secondReload.editorDocument.doc.content?.[0]?.content?.[0]?.attrs?.todoId).toBe(normalizedTodoId);
    expect(todos.todoState.todos.find((todo) => todo.id === `inline:doc-a:${normalizedTodoId}`)?.text).toBe('Task body updated again');
  });

  it('toggles an inline todo in the active session without changing its text', async () => {
    const { session, todos } = await loadFreshModules();
    const original = makeGraphTaskBinaryWithoutIds();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');

    const normalizedTodoId = String(session.editorSessionState.document?.doc.content?.[0]?.content?.[0]?.attrs?.todoId);
    expect(session.toggleInlineTodoInActiveSession('project-1', 'doc-a', normalizedTodoId)).toBe(true);
    expect(session.editorSessionState.text).toBe('- [x] Task body');
    expect(todos.todoState.todos.find((todo) => todo.id === `inline:doc-a:${normalizedTodoId}`)?.done).toBe(true);

    await session.flushLocalChanges();

    const updatedBinary = applyIncremental(original, getAppliedIncremental());
    const loaded = loadEditorDocument(updatedBinary);
    expect(loaded.editorDocument.doc.content?.[0]?.content?.[0]?.attrs?.checked).toBe(true);
    expect(loaded.visibleText).toBe('- [x] Task body');
  });

  it('does not toggle inline todos through the active session when doc ids do not match', async () => {
    const { session } = await loadFreshModules();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => makeGraphTaskBinaryWithoutIds());

    await session.openEditorSession('project-1', 'doc-a');

    expect(session.toggleInlineTodoInActiveSession('project-1', 'doc-b', 'missing')).toBe(false);
    expect(tauriApiMock.applyChanges).not.toHaveBeenCalled();
  });

  it('applies remote incrementals into the active local Automerge session', async () => {
    const { session } = await loadFreshModules();
    const original = makeGraphBinary();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');

    const remoteDoc = Automerge.load<StoredNoteDoc>(original);
    const changedRemote = Automerge.change(remoteDoc, (doc) => {
      doc.doc = {
        type: 'doc',
        content: [
          {
            type: 'heading',
            attrs: { level: 1 },
            content: [{ type: 'text', text: 'Title' }],
          },
          {
            type: 'paragraph',
            content: [{ type: 'text', text: 'Remote body' }],
          },
        ],
      } as StoredNoteDoc['doc'];
    });
    const incremental = Automerge.saveIncremental(changedRemote) as Uint8Array;

    await session.applyRemoteIncremental('project-1', 'doc-a', incremental);

    expect(session.editorSessionState.text).toBe('Title\n\nRemote body');
    expect(session.editorSessionState.storageFormat).toBe('graph-v2');
    expect(tauriApiMock.markDocSeen).toHaveBeenCalledWith('project-1', 'doc-a');
  });

  it('routes remote incrementals through the bound adapter without clearing local selection', async () => {
    const { session } = await loadFreshModules();
    const original = makeGraphBinary();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => original);

    await session.openEditorSession('project-1', 'doc-a');
    session.setLocalCursorPresence(8, [8, 12]);

    const remoteDoc = Automerge.load<StoredNoteDoc>(original);
    const changedRemote = Automerge.change(remoteDoc, (doc) => {
      doc.doc = updateGraphBodyText('Remote body').doc as StoredNoteDoc['doc'];
    });
    const incremental = Automerge.saveIncremental(changedRemote) as Uint8Array;

    const adapter = {
      attach: vi.fn((doc: Automerge.Doc<StoredNoteDoc>) => doc),
      detach: vi.fn(),
      applyIncremental: vi.fn((bytes: Uint8Array) => {
        const hydrated = Automerge.load<StoredNoteDoc>(original);
        Automerge.loadIncremental(hydrated, bytes);
        const loaded = loadEditorDocument(Automerge.save(hydrated) as Uint8Array);
        session.handleBoundEditorChange({
          source: 'remote',
          doc: hydrated,
          document: loaded.editorDocument,
          text: loaded.visibleText,
        });
        return hydrated;
      }),
      replaceSnapshot: vi.fn(),
      getCurrentDoc: vi.fn(() => null),
      getEditor: vi.fn(() => null),
      updateRemotePresence: vi.fn(),
    };

    session.bindEditorAdapter(adapter as any);
    const binaryCallsBeforeRemote = tauriApiMock.getDocBinary.mock.calls.length;

    await session.applyRemoteIncremental('project-1', 'doc-a', incremental);

    expect(adapter.applyIncremental).toHaveBeenCalledWith(incremental);
    expect(session.editorSessionState.text).toBe('Title\n\nRemote body');
    expect(session.getLocalCursorPresence()).toEqual({ cursorPos: 8, selection: [8, 12] });
    expect(tauriApiMock.getDocBinary).toHaveBeenCalledTimes(binaryCallsBeforeRemote);
  });

  it('replaces viewer snapshots without creating pending local changes', async () => {
    const { session } = await loadFreshModules();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => makeGraphBinary());

    await session.openEditorSession('project-1', 'doc-a');

    const viewerSnapshot = Automerge.save(Automerge.from({
      schemaVersion: 2,
      doc: {
        type: 'doc',
        content: [
          {
            type: 'heading',
            attrs: { level: 1 },
            content: [{ type: 'text', text: 'Viewer' }],
          },
          {
            type: 'paragraph',
            content: [{ type: 'text', text: 'Snapshot copy' }],
          },
        ],
      },
    })) as Uint8Array;

    await session.replaceViewerSnapshot('project-1', 'doc-a', viewerSnapshot);

    expect(session.editorSessionState.canEdit).toBe(false);
    expect(session.editorSessionState.text).toBe('Viewer\n\nSnapshot copy');
    await session.flushLocalChanges();
    expect(tauriApiMock.applyChanges).not.toHaveBeenCalled();
  });

  it('replaces viewer snapshots through the bound adapter without creating pending local changes', async () => {
    const { session } = await loadFreshModules();
    tauriApiMock.getDocBinary.mockImplementationOnce(async () => makeGraphBinary());

    await session.openEditorSession('project-1', 'doc-a');

    const viewerSnapshot = Automerge.save(Automerge.from({
      schemaVersion: 2,
      doc: updateGraphBodyText('Adapter snapshot').doc,
      text: 'Title\n\nAdapter snapshot',
    })) as Uint8Array;

    const adapter = {
      attach: vi.fn((doc: Automerge.Doc<StoredNoteDoc>) => doc),
      detach: vi.fn(),
      applyIncremental: vi.fn(),
      replaceSnapshot: vi.fn((bytes: Uint8Array, editable: boolean) => {
        const loaded = loadEditorDocument(bytes);
        session.handleBoundEditorChange({
          source: 'remote',
          doc: loaded.storageDoc,
          document: loaded.editorDocument,
          text: loaded.visibleText,
        });
        expect(editable).toBe(false);
        return loaded.storageDoc;
      }),
      getCurrentDoc: vi.fn(() => null),
      getEditor: vi.fn(() => null),
      updateRemotePresence: vi.fn(),
    };

    session.bindEditorAdapter(adapter as any);
    await session.replaceViewerSnapshot('project-1', 'doc-a', viewerSnapshot);

    expect(adapter.replaceSnapshot).toHaveBeenCalledWith(viewerSnapshot, false);
    expect(session.editorSessionState.canEdit).toBe(false);
    expect(session.editorSessionState.text).toBe('Title\n\nAdapter snapshot');
    await session.flushLocalChanges();
    expect(tauriApiMock.applyChanges).not.toHaveBeenCalled();
  });
});
