import * as Automerge from '@automerge/automerge';

import { TauriCommandError, tauriApi } from '../api/tauri.js';
import { buildStoredDocumentUpdate, loadEditorDocument, type StoredNoteDoc } from '../editor/document-adapter.js';
import type { AdapterChange } from '../editor/automerge-prosemirror-adapter.js';
import type { EditorAdapter } from '../editor/setup.js';
import { createDocumentFromPlainText, getVisibleTextFromDocument, type EditorDocument } from '../editor/schema.js';
import { normalizeInlineTodoIds, toggleInlineTodoInDocument } from '../editor/inline-todos.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import { getDocById, markDocUnread, setDocWordCount } from '../state/documents.svelte.js';
import { getProject } from '../state/projects.svelte.js';
import { createVersion, loadVersions, versionState } from '../state/versions.svelte.js';
import { clearVersionPreview } from '../state/version-review.svelte.js';
import { syncInlineTodosForDoc } from '../state/todos.svelte.js';

function concatChunks(chunks: Uint8Array[]) {
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const output = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.length;
  }
  return output;
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

export const editorSessionState = $state({
  projectId: null as string | null,
  docId: null as string | null,
  text: '',
  document: null as EditorDocument | null,
  storageFormat: null as 'legacy-text' | 'graph-v2' | null,
  canEdit: true,
  loading: false,
  flushing: false,
  lastError: null as string | null,
  lastErrorCode: null as string | null,
  lastErrorDetails: null as Record<string, unknown> | null,
  revision: 0,
});

let currentDoc: Automerge.Doc<StoredNoteDoc> | null = null;
let currentStorageFormat: 'legacy-text' | 'graph-v2' | null = null;
let pendingChunks: Uint8Array[] = [];
let applyTimer: ReturnType<typeof setTimeout> | null = null;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let idleVersionTimer: ReturnType<typeof setTimeout> | null = null;
let wordCountTimer: ReturnType<typeof setTimeout> | null = null;
let sessionIntentId = 0;
let transitionQueue: Promise<void> = Promise.resolve();
const supersedeResolvers = new Map<number, () => void>();
let localCursorPresence: { cursorPos: number | null; selection: [number, number] | null } | null = null;
let boundEditorAdapter: EditorAdapter | null = null;

/** 15 minutes idle = auto-create a version. */
const IDLE_VERSION_TIMEOUT_MS = 15 * 60 * 1000;

function clearTimers() {
  if (applyTimer) {
    clearTimeout(applyTimer);
    applyTimer = null;
  }
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  if (idleVersionTimer) {
    clearTimeout(idleVersionTimer);
    idleVersionTimer = null;
  }
  if (wordCountTimer) {
    clearTimeout(wordCountTimer);
    wordCountTimer = null;
  }
}

function scheduleWordCount(docId: string, text: string) {
  clearTimeout(wordCountTimer ?? undefined);
  wordCountTimer = setTimeout(() => {
    setDocWordCount(docId, text);
    wordCountTimer = null;
  }, 200);
}

function flushWordCount() {
  if (!editorSessionState.docId) return;
  clearTimeout(wordCountTimer ?? undefined);
  wordCountTimer = null;
  setDocWordCount(editorSessionState.docId, editorSessionState.text);
}

function editorDocumentsEqual(a: EditorDocument | null, b: EditorDocument) {
  if (!a) return false;
  return JSON.stringify(a.doc) === JSON.stringify(b.doc);
}

function scheduleFlush() {
  clearTimeout(applyTimer ?? undefined);
  applyTimer = setTimeout(() => {
    void flushLocalChanges();
  }, 250);

  clearTimeout(saveTimer ?? undefined);
  saveTimer = setTimeout(() => {
    void saveNow();
  }, 1500);

  // Reset the idle version timer — user is actively editing
  clearTimeout(idleVersionTimer ?? undefined);
  idleVersionTimer = setTimeout(() => {
    if (editorSessionState.projectId && editorSessionState.docId) {
      void createVersion(editorSessionState.projectId, editorSessionState.docId);
    }
  }, IDLE_VERSION_TIMEOUT_MS);
}

export function getActiveSession() {
  if (!editorSessionState.projectId || !editorSessionState.docId) return null;
  return {
    projectId: editorSessionState.projectId,
    docId: editorSessionState.docId,
  };
}

export function isActiveSession(projectId: string, docId: string) {
  return editorSessionState.projectId === projectId && editorSessionState.docId === docId;
}

export function isActiveSessionReadOnly() {
  return Boolean(editorSessionState.projectId && editorSessionState.docId && !editorSessionState.canEdit);
}

export function getLocalCursorPresence() {
  return localCursorPresence;
}

export function bindEditorAdapter(adapter: EditorAdapter | null) {
  boundEditorAdapter = adapter;
  if (!boundEditorAdapter) return;
  if (!currentDoc) return;
  currentDoc = boundEditorAdapter.attach(currentDoc, editorSessionState.canEdit);
}

export function handleBoundEditorChange(change: AdapterChange) {
  if (!editorSessionState.projectId || !editorSessionState.docId) return;

  currentDoc = change.doc;
  currentStorageFormat = 'graph-v2';
  editorSessionState.document = change.document;
  editorSessionState.storageFormat = 'graph-v2';
  editorSessionState.text = change.text;
  editorSessionState.revision += 1;
  scheduleWordCount(editorSessionState.docId, change.text);
  syncInlineTodosForDoc(editorSessionState.projectId, editorSessionState.docId, change.document);

  if (change.source !== 'local') {
    return;
  }

  const incremental = Automerge.saveIncremental(change.doc);
  if (incremental.length === 0) return;
  pendingChunks.push(incremental);
  scheduleFlush();
}

async function handleProjectAccessRevoked(projectId: string) {
  await tauriApi.purgeProjectLocalData(projectId, 'access-revoked').catch(() => undefined);
  const eviction = await import('../state/project-eviction.svelte.js');
  await eviction.evictProject(projectId, 'access-revoked');
}

export function setLocalCursorPresence(cursorPos: number | null, selection: [number, number] | null) {
  localCursorPresence = {
    cursorPos,
    selection,
  };
}

function queueSessionTransition<T>(work: () => Promise<T>): Promise<T> {
  const run = transitionQueue.then(work, work);
  transitionQueue = run.then(() => undefined, () => undefined);
  return run;
}

function bumpSessionIntent() {
  sessionIntentId += 1;
  for (const [intentId, resolve] of supersedeResolvers.entries()) {
    if (intentId < sessionIntentId) {
      supersedeResolvers.delete(intentId);
      resolve();
    }
  }
  return sessionIntentId;
}

async function raceWithSuperseded<T>(intentId: number, promise: Promise<T>) {
  if (intentId !== sessionIntentId) {
    return { superseded: true } as const;
  }

  const signal = deferred<void>();
  supersedeResolvers.set(intentId, signal.resolve);

  try {
    return await Promise.race([
      promise.then((value) => ({ superseded: false as const, value })),
      signal.promise.then(() => ({ superseded: true as const })),
    ]);
  } finally {
    if (supersedeResolvers.get(intentId) === signal.resolve) {
      supersedeResolvers.delete(intentId);
    }
  }
}

async function closeSessionInternal(intentId: number, options?: { preserveLoading?: boolean }) {
  clearVersionPreview();
  localCursorPresence = null;
  boundEditorAdapter?.detach();

  flushWordCount();
  clearTimers();

  if (!editorSessionState.projectId || !editorSessionState.docId) {
        currentDoc = null;
        currentStorageFormat = null;
        pendingChunks = [];
        editorSessionState.document = null;
        editorSessionState.storageFormat = null;
        editorSessionState.canEdit = true;
        if (intentId === sessionIntentId && !options?.preserveLoading) {
          editorSessionState.loading = false;
        }
    return;
  }

  const { projectId, docId } = editorSessionState;
  await saveNow().catch(() => undefined);

  await createVersion(projectId, docId).catch(() => undefined);
  await tauriApi.closeDoc(projectId, docId).catch(() => undefined);

  if (editorSessionState.projectId === projectId && editorSessionState.docId === docId) {
    currentDoc = null;
    currentStorageFormat = null;
    pendingChunks = [];
    editorSessionState.document = null;
    editorSessionState.storageFormat = null;
    editorSessionState.canEdit = true;
    editorSessionState.projectId = null;
    editorSessionState.docId = null;
    editorSessionState.text = '';
    editorSessionState.revision += 1;
  }

  if (intentId === sessionIntentId && !options?.preserveLoading) {
    editorSessionState.loading = false;
  }
}

async function loadBinary(projectId: string, docId: string) {
  const binary = await tauriApi.getDocBinary(projectId, docId);
  const loaded = loadEditorDocument(binary);
  const doc = loaded.storageDoc;
  const loadedDoc = versionState.deviceActorId
    ? Automerge.clone(doc, { actor: versionState.deviceActorId })
    : doc;

  return {
    doc: loadedDoc,
    editorDocument: loaded.editorDocument,
    text: loaded.visibleText,
    storageFormat: loaded.sourceSchema,
  };
}

function applyLoadedSessionState(
  projectId: string,
  docId: string,
  loaded: {
    doc: Automerge.Doc<StoredNoteDoc>;
    editorDocument: EditorDocument;
    text: string;
    storageFormat: 'legacy-text' | 'graph-v2';
  },
  options?: { canEdit?: boolean; markSeen?: boolean },
) {
  currentDoc = loaded.doc;
  currentStorageFormat = loaded.storageFormat;
  editorSessionState.document = loaded.editorDocument;
  editorSessionState.storageFormat = loaded.storageFormat;
  editorSessionState.canEdit = options?.canEdit ?? getProject(projectId)?.canEdit ?? true;
  editorSessionState.text = loaded.text;
  editorSessionState.revision += 1;
  setDocWordCount(docId, loaded.text);
  syncInlineTodosForDoc(projectId, docId, loaded.editorDocument);
  if (options?.markSeen !== false) {
    markDocUnread(docId, false);
  }
}

export async function openEditorSession(projectId: string, docId: string) {
  const intentId = bumpSessionIntent();
  editorSessionState.loading = true;
  editorSessionState.lastError = null;
  editorSessionState.lastErrorCode = null;
  editorSessionState.lastErrorDetails = null;
  clearVersionPreview();

  return queueSessionTransition(async () => {
    if (intentId !== sessionIntentId) return;

    const sameDocAlreadyOpen =
      editorSessionState.projectId === projectId
      && editorSessionState.docId === docId
      && currentDoc !== null;

    if (sameDocAlreadyOpen) {
      clearVersionPreview();
      editorSessionState.loading = false;
      return;
    }

    await closeSessionInternal(intentId, { preserveLoading: true });
    if (intentId !== sessionIntentId) return;

    let backendDocOpened = false;

    try {
      const openProjectResult = await raceWithSuperseded(intentId, tauriApi.openProject(projectId, true));
      if (openProjectResult.superseded) return;

      const openDocPromise = tauriApi.openDoc(projectId, docId);
      const openDocResult = await raceWithSuperseded(intentId, openDocPromise);
      if (openDocResult.superseded) {
        void openDocPromise
          .then(() => tauriApi.closeDoc(projectId, docId))
          .catch(() => undefined);
        return;
      }
      backendDocOpened = true;
      if (intentId !== sessionIntentId) {
        await tauriApi.closeDoc(projectId, docId).catch(() => undefined);
        return;
      }

      const markSeenResult = await raceWithSuperseded(intentId, tauriApi.markDocSeen(projectId, docId));
      if (markSeenResult.superseded) {
        await tauriApi.closeDoc(projectId, docId).catch(() => undefined);
        return;
      }

      const loadBinaryResult = await raceWithSuperseded(intentId, loadBinary(projectId, docId));
      if (loadBinaryResult.superseded) {
        await tauriApi.closeDoc(projectId, docId).catch(() => undefined);
        return;
      }
      const loaded = loadBinaryResult.value;

      localCursorPresence = null;
      editorSessionState.projectId = projectId;
      editorSessionState.docId = docId;
      applyLoadedSessionState(projectId, docId, loaded);
      if (currentDoc && boundEditorAdapter) {
        currentDoc = boundEditorAdapter.attach(currentDoc, editorSessionState.canEdit);
      }
      await loadVersions(docId);
    } catch (error) {
      if (backendDocOpened) {
        await tauriApi.closeDoc(projectId, docId).catch(() => undefined);
      }
      if (error instanceof TauriCommandError
        && (error.code === 'PROJECT_IDENTITY_MISMATCH' || error.code === 'PROJECT_NOT_FOUND')) {
        await handleProjectAccessRevoked(projectId);
        return;
      }
      if (error instanceof TauriRuntimeUnavailableError) {
        editorSessionState.lastError = null;
        editorSessionState.lastErrorCode = null;
        editorSessionState.lastErrorDetails = null;
        return;
      }
      if (error instanceof TauriCommandError) {
        editorSessionState.lastError = error.message;
        editorSessionState.lastErrorCode = error.code;
        editorSessionState.lastErrorDetails = error.details ?? null;
      } else {
        editorSessionState.lastError = error instanceof Error ? error.message : 'Failed to open note';
        editorSessionState.lastErrorCode = null;
        editorSessionState.lastErrorDetails = null;
      }
      throw error;
    } finally {
      if (intentId === sessionIntentId) {
        editorSessionState.loading = false;
      }
    }
  });
}

export async function reloadActiveSession() {
  if (!editorSessionState.projectId || !editorSessionState.docId) return;
  const projectId = editorSessionState.projectId;
  const docId = editorSessionState.docId;
  const intentId = sessionIntentId;

  try {
    const loaded = await loadBinary(projectId, docId);
    if (
      intentId !== sessionIntentId
      || editorSessionState.projectId !== projectId
      || editorSessionState.docId !== docId
    ) {
      return;
    }

    applyLoadedSessionState(projectId, docId, loaded, {
      canEdit: getProject(projectId)?.canEdit ?? true,
      markSeen: false,
    });
    if (currentDoc && boundEditorAdapter) {
      currentDoc = boundEditorAdapter.attach(currentDoc, editorSessionState.canEdit);
    }
    await tauriApi.markDocSeen(projectId, docId);
    if (
      intentId !== sessionIntentId
      || editorSessionState.projectId !== projectId
      || editorSessionState.docId !== docId
    ) {
      return;
    }

    markDocUnread(docId, false);
  } catch (error) {
    if (error instanceof TauriCommandError
      && (error.code === 'PROJECT_IDENTITY_MISMATCH' || error.code === 'PROJECT_NOT_FOUND')) {
      await handleProjectAccessRevoked(projectId);
      return;
    }
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      throw error;
    }
  }
}

export function updateEditorDocument(nextDocument: EditorDocument, nextText: string) {
  if (!editorSessionState.canEdit) return;
  if (!currentDoc || !editorSessionState.docId) return;
  const normalizedDocument = normalizeInlineTodoIds(nextDocument);
  const derivedText = getVisibleTextFromDocument(normalizedDocument);
  if (derivedText === editorSessionState.text && editorDocumentsEqual(editorSessionState.document, normalizedDocument)) {
    return;
  }

  const update = buildStoredDocumentUpdate(currentDoc, normalizedDocument, derivedText);
  currentDoc = update.storageDoc;
  currentStorageFormat = 'graph-v2';
  editorSessionState.document = normalizedDocument;
  editorSessionState.storageFormat = 'graph-v2';
  editorSessionState.canEdit = true;
  editorSessionState.text = derivedText;
  editorSessionState.revision += 1;
  scheduleWordCount(editorSessionState.docId, derivedText);
  syncInlineTodosForDoc(editorSessionState.projectId!, editorSessionState.docId, normalizedDocument);

  if (update.incremental.length > 0) {
    pendingChunks.push(update.incremental);
    scheduleFlush();
  }

  if (boundEditorAdapter) {
    currentDoc = boundEditorAdapter.replaceSnapshot(Automerge.save(currentDoc) as Uint8Array, true);
  }
}

export function updateEditorText(nextText: string) {
  updateEditorDocument(createDocumentFromPlainText(nextText), nextText);
}

export function toggleInlineTodoInActiveSession(projectId: string, docId: string, todoId: string) {
  if (!editorSessionState.canEdit) return false;
  if (editorSessionState.projectId !== projectId || editorSessionState.docId !== docId) return false;
  if (!editorSessionState.document) return false;

  const toggled = toggleInlineTodoInDocument(editorSessionState.document, todoId);
  if (!toggled) return false;

  updateEditorDocument(toggled.document, getVisibleTextFromDocument(toggled.document));
  return true;
}

export async function flushLocalChanges() {
  if (!editorSessionState.canEdit) {
    pendingChunks = [];
    return;
  }
  if (!editorSessionState.projectId || !editorSessionState.docId || pendingChunks.length === 0) {
    return;
  }

  editorSessionState.flushing = true;
  const data = concatChunks(pendingChunks);
  pendingChunks = [];

  try {
    await tauriApi.applyChanges(editorSessionState.projectId, editorSessionState.docId, data);
  } catch (error) {
    if (error instanceof TauriCommandError
      && (error.code === 'PROJECT_IDENTITY_MISMATCH' || error.code === 'PROJECT_NOT_FOUND')) {
      await handleProjectAccessRevoked(editorSessionState.projectId);
      return;
    }
    if (error instanceof TauriRuntimeUnavailableError) {
      pendingChunks.unshift(data);
      return;
    }
    editorSessionState.lastError =
      error instanceof Error ? error.message : 'Failed to apply changes';
    pendingChunks.unshift(data);
    throw error;
  } finally {
    editorSessionState.flushing = false;
  }
}

export async function saveNow() {
  if (!editorSessionState.projectId || !editorSessionState.docId) return;
  const projectId = editorSessionState.projectId;
  const docId = editorSessionState.docId;
  try {
    await flushLocalChanges();
    if (editorSessionState.projectId !== projectId || editorSessionState.docId !== docId) {
      return;
    }
    await tauriApi.saveDoc(projectId, docId);
  } catch (error) {
    if (error instanceof TauriCommandError
      && (error.code === 'PROJECT_IDENTITY_MISMATCH' || error.code === 'PROJECT_NOT_FOUND')) {
      await handleProjectAccessRevoked(projectId);
      return;
    }
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      throw error;
    }
  }
}

export async function closeEditorSession() {
  const intentId = bumpSessionIntent();
  return queueSessionTransition(async () => {
    if (intentId !== sessionIntentId) return;
    await closeSessionInternal(intentId);
  });
}

export async function applyRemoteIncremental(projectId: string, docId: string, incremental: Uint8Array) {
  if (!currentDoc || incremental.length === 0) return;
  if (!isActiveSession(projectId, docId)) return;

  if (boundEditorAdapter) {
    currentDoc = boundEditorAdapter.applyIncremental(incremental) ?? currentDoc;
  } else {
    const nextDoc = Automerge.loadIncremental(currentDoc, incremental);
    currentDoc = nextDoc;
    const loaded = loadEditorDocument(Automerge.save(nextDoc) as Uint8Array);
    currentStorageFormat = loaded.sourceSchema;
    editorSessionState.document = loaded.editorDocument;
    editorSessionState.storageFormat = loaded.sourceSchema;
    editorSessionState.text = loaded.visibleText;
    editorSessionState.revision += 1;
    setDocWordCount(docId, loaded.visibleText);
    syncInlineTodosForDoc(projectId, docId, loaded.editorDocument);
  }
  markDocUnread(docId, false);
  await tauriApi.markDocSeen(projectId, docId);
}

export async function replaceViewerSnapshot(projectId: string, docId: string, snapshot: Uint8Array) {
  if (!isActiveSession(projectId, docId)) return;

  const loaded = loadEditorDocument(snapshot);
  const nextDoc = versionState.deviceActorId
    ? Automerge.clone(loaded.storageDoc, { actor: versionState.deviceActorId })
    : loaded.storageDoc;
  pendingChunks = [];
  applyLoadedSessionState(projectId, docId, {
    doc: nextDoc,
    editorDocument: loaded.editorDocument,
    text: loaded.visibleText,
    storageFormat: loaded.sourceSchema,
  }, {
    canEdit: false,
  });
  if (currentDoc && boundEditorAdapter) {
    currentDoc = boundEditorAdapter.replaceSnapshot(snapshot, false);
  }
  await tauriApi.markDocSeen(projectId, docId);
}

export async function renameActiveDoc(newPath: string) {
  if (!editorSessionState.projectId || !editorSessionState.docId) return;
  try {
    await tauriApi.renameNote(editorSessionState.projectId, editorSessionState.docId, newPath);
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return;
    throw error;
  }
  const doc = getDocById(editorSessionState.docId);
  if (doc) {
    doc.path = newPath;
    doc.title = newPath.split('/').pop()?.replace(/\.md$/i, '') ?? newPath;
  }
}
