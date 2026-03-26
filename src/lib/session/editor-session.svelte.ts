import * as Automerge from '@automerge/automerge';

import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import { getDocById, markDocUnread, setActiveDoc, setDocWordCount } from '../state/documents.svelte.js';
import { createVersion, leaveHistoryReview, loadVersions, versionState } from '../state/versions.svelte.js';
import { uiState } from '../state/ui.svelte.js';

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

async function loadBinary(projectId: string, docId: string) {
  const binary = await tauriApi.getDocBinary(projectId, docId);
  const doc = Automerge.load<NotesDoc>(binary);
  // Set stable device actor ID if available
  if (versionState.deviceActorId) {
    currentDoc = Automerge.clone(doc, { actor: versionState.deviceActorId });
  } else {
    currentDoc = doc;
  }
  editorSessionState.text = getDocText(currentDoc);
  editorSessionState.revision += 1;
  setDocWordCount(docId, editorSessionState.text);
}

export async function openEditorSession(projectId: string, docId: string) {
  if (editorSessionState.projectId === projectId && editorSessionState.docId === docId) {
    // Session already loaded — just ensure the UI view is restored
    // (user may have navigated to project-overview or settings and come back)
    leaveHistoryReview();
    setActiveDoc(docId);
    return;
  }

  await closeEditorSession();

  editorSessionState.loading = true;
  editorSessionState.lastError = null;

  try {
    await tauriApi.openProject(projectId, true);
    await tauriApi.openDoc(projectId, docId);
    await tauriApi.markDocSeen(projectId, docId);
    await loadBinary(projectId, docId);
    editorSessionState.projectId = projectId;
    editorSessionState.docId = docId;
    setActiveDoc(docId);
    markDocUnread(docId, false);
    await loadVersions(docId);
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) {
      editorSessionState.lastError = null;
      return;
    }
    editorSessionState.lastError = error instanceof Error ? error.message : 'Failed to open note';
    throw error;
  } finally {
    editorSessionState.loading = false;
  }
}

export async function reloadActiveSession() {
  if (!editorSessionState.projectId || !editorSessionState.docId) return;
  try {
    await loadBinary(editorSessionState.projectId, editorSessionState.docId);
    await tauriApi.markDocSeen(editorSessionState.projectId, editorSessionState.docId);
    markDocUnread(editorSessionState.docId, false);
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
  // Exit any review modes immediately (before async work)
  leaveHistoryReview();

  flushWordCount();
  clearTimers();

  if (!editorSessionState.projectId || !editorSessionState.docId) {
    currentDoc = null;
    pendingChunks = [];
    return;
  }

  const { projectId, docId } = editorSessionState;
  await saveNow().catch(() => undefined);

  // Create an auto-version on document switch/close (if there are changes)
  await createVersion(projectId, docId).catch(() => undefined);

  await tauriApi.closeDoc(projectId, docId).catch(() => undefined);

  // Only clear state if no newer session was opened while we were awaiting
  if (editorSessionState.projectId === projectId && editorSessionState.docId === docId) {
    currentDoc = null;
    pendingChunks = [];
    editorSessionState.projectId = null;
    editorSessionState.docId = null;
    editorSessionState.text = '';
    editorSessionState.revision += 1;
  }
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
