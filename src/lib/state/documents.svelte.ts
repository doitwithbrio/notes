import { tauriApi } from '../api/tauri.js';
import type { BackendDocInfo, BackendUnseenDocInfo, Document, SyncStatus } from '../types/index.js';
import { uiState } from './ui.svelte.js';

export const documentState = $state({
  activeDocId: null as string | null,
  docs: [] as Document[],
  loading: false,
});

function titleFromPath(path: string) {
  const leaf = path.split('/').pop() ?? path;
  return leaf.replace(/\.md$/i, '');
}

function countWords(text: string) {
  const trimmed = text.trim();
  return trimmed ? trimmed.split(/\s+/).length : 0;
}

function mapDoc(projectId: string, doc: BackendDocInfo, unseenMap: Map<string, BackendUnseenDocInfo>): Document {
  const unseen = unseenMap.get(doc.id);
  return {
    id: doc.id,
    projectId,
    path: doc.path,
    title: titleFromPath(doc.path),
    syncStatus: 'local-only',
    wordCount: 0,
    activePeers: [],
    hasUnread: unseen?.hasUnseenChanges ?? false,
    createdAt: doc.created,
    lastSeenAt: unseen?.lastSeenAt ?? null,
  };
}

export function getActiveDoc(): Document | null {
  return documentState.docs.find((doc) => doc.id === documentState.activeDocId) ?? null;
}

export function getDocById(docId: string | null): Document | null {
  if (!docId) return null;
  return documentState.docs.find((doc) => doc.id === docId) ?? null;
}

export function setActiveDoc(docId: string | null) {
  documentState.activeDocId = docId;
  if (!docId) return;
  const doc = getDocById(docId);
  if (!doc) return;
  uiState.view = 'editor';
  uiState.activeProjectId = doc.projectId;
}

export async function loadProjectDocs(projectId: string) {
  const [docs, unseenDocs] = await Promise.all([
    tauriApi.listFiles(projectId),
    tauriApi.getUnseenDocs(projectId),
  ]);
  const unseenMap = new Map(unseenDocs.map((entry) => [entry.docId, entry]));
  const nextDocs = docs.map((doc) => mapDoc(projectId, doc, unseenMap));
  documentState.docs = [
    ...documentState.docs.filter((doc) => doc.projectId !== projectId),
    ...nextDocs,
  ].sort((a, b) => a.path.localeCompare(b.path));
}

export async function loadAllProjectDocs(projectIds: string[]) {
  documentState.loading = true;
  try {
    for (const projectId of projectIds) {
      await tauriApi.openProject(projectId);
      await loadProjectDocs(projectId);
    }
  } finally {
    documentState.loading = false;
  }
}

export function addDoc(doc: Document) {
  documentState.docs.push(doc);
}

export function removeDoc(docId: string) {
  const index = documentState.docs.findIndex((doc) => doc.id === docId);
  if (index >= 0) documentState.docs.splice(index, 1);
  if (documentState.activeDocId === docId) {
    documentState.activeDocId = null;
  }
}

export async function deleteDoc(projectId: string, docId: string) {
  await tauriApi.deleteNote(projectId, docId);
  removeDoc(docId);
}

export function markDocUnread(docId: string, hasUnread = true) {
  const doc = getDocById(docId);
  if (doc) doc.hasUnread = hasUnread;
}

export function setDocSyncStatus(docId: string, syncStatus: SyncStatus) {
  const doc = getDocById(docId);
  if (doc) doc.syncStatus = syncStatus;
}

export function setDocWordCount(docId: string, text: string) {
  const doc = getDocById(docId);
  if (doc) doc.wordCount = countWords(text);
}

export function setDocPath(docId: string, path: string) {
  const doc = getDocById(docId);
  if (!doc) return;
  doc.path = path;
  doc.title = titleFromPath(path);
}

export function setDocActivePeers(docId: string, peerIds: string[]) {
  const doc = getDocById(docId);
  if (doc) doc.activePeers = peerIds;
}

export function setProjectActivePeers(projectId: string, peerToDocMap: Map<string, string | null>) {
  for (const doc of documentState.docs) {
    if (doc.projectId !== projectId) continue;
    doc.activePeers = [];
  }
  for (const [peerId, docId] of peerToDocMap.entries()) {
    if (!docId) continue;
    const doc = getDocById(docId);
    if (doc && !doc.activePeers.includes(peerId)) {
      doc.activePeers = [...doc.activePeers, peerId];
    }
  }
}

export function reorderDocs(projectId: string, fromIndex: number, toIndex: number) {
  if (fromIndex === toIndex) return;
  const scoped = documentState.docs.filter((doc) => doc.projectId === projectId);
  const [item] = scoped.splice(fromIndex, 1);
  if (!item) return;
  scoped.splice(toIndex, 0, item);

  let scopedIndex = 0;
  documentState.docs = documentState.docs.map((doc) =>
    doc.projectId === projectId ? scoped[scopedIndex++]! : doc,
  );
}
