import * as Automerge from '@automerge/automerge';

import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import { getDocById, markDocUnread, setDocWordCount } from '../state/documents.svelte.js';
import { createVersion, loadVersions, versionState } from '../state/versions.svelte.js';
import { clearVersionPreview } from '../state/version-review.svelte.js';

type NotesDoc = {
  schemaVersion?: number;
  text?: string;
};

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

function getDocText(doc: Automerge.Doc<NotesDoc>) {
  return typeof doc.text === 'string' ? doc.text : String(doc.text ?? '');
}

export const editorSessionState = $state({
  projectId: null as string | null,
  docId: null as string | null,
  text: '',
  loading: false,
  flushing: false,
  lastError: null as string | null,
  revision: 0,
});

let currentDoc: Automerge.Doc<NotesDoc> | null = null;
let pendingChunks: Uint8Array[] = [];
let applyTimer: ReturnType<typeof setTimeout> | null = null;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let idleVersionTimer: ReturnType<typeof setTimeout> | null = null;
let wordCountTimer: ReturnType<typeof setTimeout> | null = null;
let sessionIntentId = 0;
let transitionQueue: Promise<void> = Promise.resolve();
const supersedeResolvers = new Map<number, () => void>();

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

  flushWordCount();
  clearTimers();

  if (!editorSessionState.projectId || !editorSessionState.docId) {
    currentDoc = null;
    pendingChunks = [];
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
    pendingChunks = [];
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
  const doc = Automerge.load<NotesDoc>(binary);
  const loadedDoc = versionState.deviceActorId
    ? Automerge.clone(doc, { actor: versionState.deviceActorId })
    : doc;

  return {
    doc: loadedDoc,
    text: getDocText(loadedDoc),
  };
}

export async function openEditorSession(projectId: string, docId: string) {
  const intentId = bumpSessionIntent();
  editorSessionState.loading = true;
  editorSessionState.lastError = null;
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

      currentDoc = loaded.doc;
      editorSessionState.text = loaded.text;
      editorSessionState.revision += 1;
      setDocWordCount(docId, loaded.text);
      editorSessionState.projectId = projectId;
      editorSessionState.docId = docId;
      markDocUnread(docId, false);
      await loadVersions(docId);
    } catch (error) {
      if (backendDocOpened) {
        await tauriApi.closeDoc(projectId, docId).catch(() => undefined);
      }
      if (error instanceof TauriRuntimeUnavailableError) {
        editorSessionState.lastError = null;
        return;
      }
      editorSessionState.lastError = error instanceof Error ? error.message : 'Failed to open note';
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

    currentDoc = loaded.doc;
    editorSessionState.text = loaded.text;
    editorSessionState.revision += 1;
    setDocWordCount(docId, loaded.text);
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
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      throw error;
    }
  }
}

export function updateEditorText(nextText: string) {
  if (!currentDoc || !editorSessionState.docId) return;
  if (nextText === editorSessionState.text) return;

  const nextDoc = Automerge.change(currentDoc, (doc) => {
    doc.text = nextText;
    if (doc.schemaVersion === undefined) {
      doc.schemaVersion = 1;
    }
  });

  const incremental = Automerge.saveIncremental(nextDoc);
  currentDoc = nextDoc;
  editorSessionState.text = nextText;
  editorSessionState.revision += 1;
  scheduleWordCount(editorSessionState.docId, nextText);

  if (incremental.length > 0) {
    pendingChunks.push(incremental);
    scheduleFlush();
  }
}

export async function flushLocalChanges() {
  if (!editorSessionState.projectId || !editorSessionState.docId || pendingChunks.length === 0) {
    return;
  }

  editorSessionState.flushing = true;
  const data = concatChunks(pendingChunks);
  pendingChunks = [];

  try {
    await tauriApi.applyChanges(editorSessionState.projectId, editorSessionState.docId, data);
  } catch (error) {
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
  try {
    await flushLocalChanges();
    await tauriApi.saveDoc(editorSessionState.projectId, editorSessionState.docId);
  } catch (error) {
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
