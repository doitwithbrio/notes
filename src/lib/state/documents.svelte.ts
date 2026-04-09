import { TauriCommandError, tauriApi } from '../api/tauri.js';
import type { BackendDocInfo, BackendUnseenDocInfo, Document, SyncStatus } from '../types/index.js';
import { applyDocOrder, saveDocOrder } from './ordering.svelte.js';
import { loadProjectTodos } from './todos.svelte.js';

export const documentState = $state({
  docs: [] as Document[],
  loading: false,
  loadingProjectIds: [] as string[],
  hydratedProjectIds: [] as string[],
});

type ProjectLoadState = {
  promise: Promise<void> | null;
  reloadRequested: boolean;
  connectPeersRequested: boolean;
};

const projectLoadStates = new Map<string, ProjectLoadState>();

function getProjectLoadState(projectId: string): ProjectLoadState {
  let state = projectLoadStates.get(projectId);
  if (!state) {
    state = {
      promise: null,
      reloadRequested: false,
      connectPeersRequested: false,
    };
    projectLoadStates.set(projectId, state);
  }
  return state;
}

function setProjectLoading(projectId: string, loading: boolean) {
  if (loading) {
    if (!documentState.loadingProjectIds.includes(projectId)) {
      documentState.loadingProjectIds = [...documentState.loadingProjectIds, projectId];
    }
    return;
  }

  if (documentState.loadingProjectIds.includes(projectId)) {
    documentState.loadingProjectIds = documentState.loadingProjectIds.filter((id) => id !== projectId);
  }
}

function markProjectHydrated(projectId: string) {
  if (!documentState.hydratedProjectIds.includes(projectId)) {
    documentState.hydratedProjectIds = [...documentState.hydratedProjectIds, projectId];
  }
}

export function isProjectLoading(projectId: string) {
  return documentState.loadingProjectIds.includes(projectId);
}

export function hasHydratedProject(projectId: string) {
  return documentState.hydratedProjectIds.includes(projectId);
}

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

async function fetchProjectDocs(projectId: string): Promise<Document[]> {
  const [docs, unseenDocs] = await Promise.all([
    tauriApi.listFiles(projectId),
    tauriApi.getUnseenDocs(projectId),
  ]);
  const unseenMap = new Map(unseenDocs.map((entry) => [entry.docId, entry]));
  return applyDocOrder(
    projectId,
    docs.map((doc) => mapDoc(projectId, doc, unseenMap)),
  );
}

function isRevokedProjectError(error: unknown) {
  return error instanceof TauriCommandError
    && (error.code === 'PROJECT_IDENTITY_MISMATCH' || error.code === 'PROJECT_NOT_FOUND');
}

async function purgeInaccessibleProject(projectId: string, reason: string) {
  await tauriApi.purgeProjectLocalData(projectId, reason).catch(() => undefined);
  const eviction = await import('./project-eviction.svelte.js');
  await eviction.evictProject(projectId, reason);
}

export function getDocById(docId: string | null): Document | null {
  if (!docId) return null;
  return documentState.docs.find((doc) => doc.id === docId) ?? null;
}

export async function loadProjectDocs(
  projectId: string,
  options?: { force?: boolean; connectPeers?: boolean },
) {
  if (!options?.force && !options?.connectPeers && hasHydratedProject(projectId)) {
    return;
  }

  const loadState = getProjectLoadState(projectId);
  loadState.reloadRequested = loadState.reloadRequested || !!options?.force || !hasHydratedProject(projectId);
  loadState.connectPeersRequested = loadState.connectPeersRequested || !!options?.connectPeers;

  if (loadState.promise) {
    return loadState.promise;
  }

  loadState.promise = (async () => {
    setProjectLoading(projectId, true);
    try {
      do {
        const shouldReload = loadState.reloadRequested;
        const shouldConnectPeers = loadState.connectPeersRequested;
        loadState.reloadRequested = false;
        loadState.connectPeersRequested = false;

        if (shouldReload) {
          await tauriApi.openProject(projectId, shouldConnectPeers);
          const orderedDocs = await fetchProjectDocs(projectId);
          documentState.docs = [
            ...documentState.docs.filter((doc) => doc.projectId !== projectId),
            ...orderedDocs,
          ];
          markProjectHydrated(projectId);
          void loadProjectTodos(projectId, { force: true }).catch((error) => {
            console.error(`Failed to hydrate todos for ${projectId}`, error);
          });
        } else if (shouldConnectPeers) {
          await tauriApi.openProject(projectId, true);
        }
      } while (loadState.reloadRequested || loadState.connectPeersRequested);
    } catch (error) {
      if (isRevokedProjectError(error)) {
        await purgeInaccessibleProject(projectId, 'access-revoked');
        return;
      }
      throw error;
    } finally {
      setProjectLoading(projectId, false);
      loadState.promise = null;
    }
  })();

  return loadState.promise;
}

export async function loadAllProjectDocs(projectIds: string[], concurrency = 2) {
  documentState.loading = true;
  try {
    const queue = [...projectIds];
    const workers = Array.from({ length: Math.max(1, concurrency) }, async () => {
      while (queue.length > 0) {
        const projectId = queue.shift();
        if (!projectId) continue;
        await loadProjectDocs(projectId);
      }
    });
    await Promise.all(workers);
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
}

export function clearProjectDocs(projectId: string) {
  documentState.docs = documentState.docs.filter((doc) => doc.projectId !== projectId);
  documentState.loadingProjectIds = documentState.loadingProjectIds.filter((id) => id !== projectId);
  documentState.hydratedProjectIds = documentState.hydratedProjectIds.filter((id) => id !== projectId);
  projectLoadStates.delete(projectId);
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
  saveDocOrder(projectId, scoped);
}
