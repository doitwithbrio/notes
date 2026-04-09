import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError, waitForTauriRuntime } from '../runtime/tauri.js';
import { getDocById, loadAllProjectDocs, markDocUnread, setDocSyncStatus } from './documents.svelte.js';
import { loadDeviceActorId, scheduleVersionRefresh } from './versions.svelte.js';
import {
  clearProjectPeersLoading,
  expireStalePresence,
  getOnlinePeers,
  hasAnySharedPeers,
  hydrateProjectPeers,
  markProjectPeersLoading,
  setLocalPeerId,
  updatePeerStatus,
  updatePresence,
} from './presence.svelte.js';
import { loadOrder } from './ordering.svelte.js';
import { loadProjects, projectState } from './projects.svelte.js';
import { loadSettings } from './settings.svelte.js';
import { applySyncState, setPeerCount, setSharedProject } from './sync.svelte.js';
import { checkForUpdate } from './updates.svelte.js';
import {
  applyRemoteIncremental,
  getActiveSession,
  getLocalCursorPresence,
  isActiveSession,
  isActiveSessionReadOnly,
  reloadActiveSession,
  replaceViewerSnapshot,
} from '../session/editor-session.svelte.js';
import { handleInviteAcceptEvent, hydrateInviteStatus } from './invite.svelte.js';
import { getWorkspaceProjectId } from '../navigation/workspace-router.svelte.js';
import { loadProjectTodos } from './todos.svelte.js';
import { evictProject } from './project-eviction.svelte.js';

export const appSessionState = $state({
  booting: false,
  ready: false,
  error: null as string | null,
  unlistenFns: [] as Array<() => void>,
});

const projectPeerRequestIds = new Map<string, number>();
const PRESENCE_CHECK_INTERVAL_MS = 100;
const PRESENCE_HEARTBEAT_MS = 1_000;
const CURSOR_PUBLISH_INTERVAL_MS = 50;

let presenceTimer: ReturnType<typeof setInterval> | null = null;
let lastPublishedPresence: { projectId: string | null; docId: string | null } = {
  projectId: null,
  docId: null,
};
let lastPresenceSentAt = 0;
let lastPublishedCursor: { projectId: string | null; docId: string | null; cursorPos: number | null; selection: [number, number] | null } = {
  projectId: null,
  docId: null,
  cursorPos: null,
  selection: null,
};
let lastCursorSentAt = 0;

async function publishPresenceSnapshot(force = false) {
  const active = getActiveSession();
  const current = {
    projectId: active?.projectId ?? getWorkspaceProjectId(),
    docId: active?.docId ?? null,
  };

  const changed = current.projectId !== lastPublishedPresence.projectId
    || current.docId !== lastPublishedPresence.docId;
  const shouldHeartbeat = current.projectId !== null && Date.now() - lastPresenceSentAt >= PRESENCE_HEARTBEAT_MS;
  const cursorState = getLocalCursorPresence();
  const nextCursorPos = current.docId ? cursorState?.cursorPos ?? null : null;
  const nextSelection = current.docId ? cursorState?.selection ?? null : null;
  const cursorChanged = current.projectId !== lastPublishedCursor.projectId
    || current.docId !== lastPublishedCursor.docId
    || nextCursorPos !== lastPublishedCursor.cursorPos
    || JSON.stringify(nextSelection) !== JSON.stringify(lastPublishedCursor.selection);
  const shouldPublishCursor = current.docId !== null && cursorChanged && Date.now() - lastCursorSentAt >= CURSOR_PUBLISH_INTERVAL_MS;

  if (!force && !changed && !shouldHeartbeat && !shouldPublishCursor) {
    return;
  }

  if (
    lastPublishedPresence.projectId
    && lastPublishedPresence.projectId !== current.projectId
  ) {
    try {
      await tauriApi.broadcastPresence(lastPublishedPresence.projectId, null, null, null);
    } catch (error) {
      if (!(error instanceof TauriRuntimeUnavailableError)) {
        console.debug('Presence project-switch clear skipped', error);
      }
    }
  }

  const targetProjectId = current.projectId ?? lastPublishedPresence.projectId;
  if (!targetProjectId) {
    return;
  }

  try {
    await tauriApi.broadcastPresence(targetProjectId, current.projectId ? current.docId : null, nextCursorPos, nextSelection);
    lastPublishedPresence = current;
    lastPresenceSentAt = Date.now();
    lastPublishedCursor = {
      projectId: current.projectId,
      docId: current.docId,
      cursorPos: nextCursorPos,
      selection: nextSelection,
    };
    lastCursorSentAt = Date.now();
  } catch (error) {
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      console.debug('Presence publish skipped', error);
    }
  }
}

async function clearPublishedPresence() {
  if (!lastPublishedPresence.projectId) {
    return;
  }

  try {
    await tauriApi.broadcastPresence(lastPublishedPresence.projectId, null, null, null);
  } catch (error) {
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      console.debug('Presence clear skipped', error);
    }
  } finally {
    lastPublishedPresence = { projectId: null, docId: null };
    lastPresenceSentAt = 0;
    lastPublishedCursor = { projectId: null, docId: null, cursorPos: null, selection: null };
    lastCursorSentAt = 0;
  }
}

function startPresenceCoordinator() {
  if (presenceTimer) return;
  presenceTimer = setInterval(() => {
    void publishPresenceSnapshot();
    expireStalePresence();
  }, PRESENCE_CHECK_INTERVAL_MS);
}

function stopPresenceCoordinator() {
  if (presenceTimer) {
    clearInterval(presenceTimer);
    presenceTimer = null;
  }
  lastPublishedPresence = { projectId: null, docId: null };
  lastPresenceSentAt = 0;
  lastPublishedCursor = { projectId: null, docId: null, cursorPos: null, selection: null };
  lastCursorSentAt = 0;
}

async function warmProjectDocs(projectIds: string[]) {
  if (projectIds.length === 0) return;
  await loadAllProjectDocs(projectIds, 2);
}

async function warmPeerStatus(projectIds: string[]) {
  if (projectIds.length === 0) return;

  await Promise.all(projectIds.map((projectId) => hydrateProjectRoster(projectId)));
}

export async function hydrateProjectRoster(projectId: string) {
  markProjectPeersLoading(projectId);
  const requestId = (projectPeerRequestIds.get(projectId) ?? 0) + 1;
  projectPeerRequestIds.set(projectId, requestId);

  try {
    const peers = await tauriApi.getPeerStatus(projectId);
    if (projectPeerRequestIds.get(projectId) !== requestId) {
      return;
    }

    hydrateProjectPeers(projectId, peers);
    setSharedProject(hasAnySharedPeers());
    setPeerCount(getOnlinePeers().length);
  } catch (error) {
    if (projectPeerRequestIds.get(projectId) === requestId) {
      clearProjectPeersLoading(projectId);
    }
    throw error;
  }
}

export async function initializeApp() {
  if (appSessionState.booting || appSessionState.ready) return;

  appSessionState.booting = true;
  appSessionState.error = null;

  try {
    const runtimeReady = await waitForTauriRuntime(2000);
    if (!runtimeReady) {
      return;
    }

    const localPeerId = await tauriApi.getPeerId();
    setLocalPeerId(localPeerId);

    const [remoteUnlisten, syncUnlisten, peerUnlisten, presenceUnlisten, inviteUnlisten, projectEvictedUnlisten] = await Promise.all([
      tauriApi.onRemoteChange(async ({ projectId, docId, mode }) => {
        const docProjectId = getDocById(docId)?.projectId ?? projectId;
        if (docProjectId) {
          void loadProjectTodos(docProjectId, { force: true }).catch((error) => {
            console.error('Failed to refresh todos after remote change', error);
          });
        }

        if (docProjectId && isActiveSession(docProjectId, docId)) {
          if (mode === 'incremental-available' && !isActiveSessionReadOnly()) {
            const incremental = await tauriApi.getDocIncremental(docProjectId, docId);
            await applyRemoteIncremental(docProjectId, docId, incremental);
          } else if (mode === 'viewer-snapshot-available' || isActiveSessionReadOnly()) {
            const snapshot = await tauriApi.getViewerDocSnapshot(docProjectId, docId);
            await replaceViewerSnapshot(docProjectId, docId, snapshot);
          } else if (mode === 'metadata-only') {
            markDocUnread(docId, true);
          } else {
            await reloadActiveSession();
          }
          scheduleVersionRefresh(docId);
        } else {
          markDocUnread(docId, true);
        }
      }),
      tauriApi.onSyncStatus(({ docId, state, unsentChanges }) => {
        setDocSyncStatus(docId, state);
        applySyncState(state, unsentChanges);
      }),
      tauriApi.onPeerStatus(({ peerId, state, alias }) => {
        updatePeerStatus(peerId, state === 'connected', alias);
        setPeerCount(getOnlinePeers().length);
        const activeProjectId = getWorkspaceProjectId();
        if (activeProjectId) {
          void hydrateProjectRoster(activeProjectId).catch((error) => {
            console.error('Failed to rehydrate peer roster', error);
          });
        }
      }),
      tauriApi.onPresenceUpdate(({ projectId, peerId, sessionId, sessionStartedAt, seq, alias, activeDoc, cursorPos, selection }) => {
        updatePresence(projectId, peerId, sessionId, sessionStartedAt, seq, alias, activeDoc, cursorPos, selection);
      }),
      tauriApi.onInviteAcceptStatus((event) => {
        handleInviteAcceptEvent(event);
      }),
      tauriApi.onProjectEvicted((event) => {
        void evictProject(event.projectName, event.reason, event.projectName, event.projectId);
      }),
    ]);

    appSessionState.unlistenFns = [remoteUnlisten, syncUnlisten, peerUnlisten, presenceUnlisten, inviteUnlisten, projectEvictedUnlisten];
    startPresenceCoordinator();

    // Load saved ordering from localStorage before fetching projects/docs
    loadOrder();

    await loadProjects();
    await hydrateInviteStatus();
    const evictionNotices = await tauriApi.listProjectEvictionNotices().catch(() => []);
    for (const notice of evictionNotices) {
      void evictProject(notice.projectName, notice.reason, notice.projectName, notice.projectId);
    }

    appSessionState.ready = true;

    const projectIds = projectState.projects.map((project) => project.id);
    void warmProjectDocs(projectIds).catch((error) => {
      console.error('Failed to hydrate project docs', error);
    });
    void warmPeerStatus(projectIds).catch((error) => {
      console.error('Failed to hydrate peer status', error);
    });
    void loadDeviceActorId().catch((error) => {
      console.error('Failed to load device actor ID', error);
    });
    void loadSettings().catch((error) => {
      console.error('Failed to load settings', error);
    });

    // Silent background update check — runs after everything is loaded,
    // doesn't block the UI, swallows errors (silent=true).
    // If an update is found, the UpdateBanner will appear automatically.
    checkForUpdate(true).catch(() => {});
  } catch (error) {
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      appSessionState.error = error instanceof Error ? error.message : 'Failed to initialize app';
    }
  } finally {
    appSessionState.booting = false;
  }
}

export async function loadDocSidebars(projectId: string, docId: string) {
  await hydrateProjectRoster(projectId);
  scheduleVersionRefresh(docId, 0);
}

export function teardownAppSession() {
  void clearPublishedPresence().catch(() => undefined);
  stopPresenceCoordinator();
  for (const unlisten of appSessionState.unlistenFns) {
    unlisten();
  }
  appSessionState.unlistenFns = [];
  appSessionState.ready = false;
}
