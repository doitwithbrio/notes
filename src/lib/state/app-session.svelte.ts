import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError, waitForTauriRuntime } from '../runtime/tauri.js';
import { loadAllProjectDocs, markDocUnread, setDocSyncStatus } from './documents.svelte.js';
import { loadDeviceActorId, scheduleVersionRefresh } from './versions.svelte.js';
import { getOnlinePeers, hydrateProjectPeers, updatePeerStatus, updatePresence } from './presence.svelte.js';
import { loadOrder } from './ordering.svelte.js';
import { loadProjects, projectState } from './projects.svelte.js';
import { loadSettings } from './settings.svelte.js';
import { applySyncState, setPeerCount, setSharedProject } from './sync.svelte.js';
import { checkForUpdate } from './updates.svelte.js';
import { getActiveSession, reloadActiveSession } from '../session/editor-session.svelte.js';

export const appSessionState = $state({
  booting: false,
  ready: false,
  error: null as string | null,
  unlistenFns: [] as Array<() => void>,
});

async function warmProjectDocs(projectIds: string[]) {
  if (projectIds.length === 0) return;
  await loadAllProjectDocs(projectIds, 2);
}

async function warmPeerStatus(projectIds: string[]) {
  if (projectIds.length === 0) return;

  const peerResults = await Promise.all(
    projectIds.map(async (projectId) => ({
      projectId,
      peers: await tauriApi.getPeerStatus(projectId),
    })),
  );

  let totalPeers = 0;
  let anyShared = false;
  for (const { projectId, peers } of peerResults) {
    totalPeers += peers.filter((peer) => peer.connected).length;
    if (peers.length > 0) anyShared = true;
    hydrateProjectPeers(projectId, peers);
  }

  setSharedProject(anyShared);
  setPeerCount(totalPeers);
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

    const [remoteUnlisten, syncUnlisten, peerUnlisten, presenceUnlisten] = await Promise.all([
      tauriApi.onRemoteChange(async ({ docId }) => {
        const active = getActiveSession();
        if (active?.docId === docId) {
          await reloadActiveSession();
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
      }),
      tauriApi.onPresenceUpdate(({ peerId, alias, activeDoc, cursorPos, selection }) => {
        updatePresence(peerId, alias, activeDoc, cursorPos, selection);
      }),
    ]);

    appSessionState.unlistenFns = [remoteUnlisten, syncUnlisten, peerUnlisten, presenceUnlisten];

    // Load saved ordering from localStorage before fetching projects/docs
    loadOrder();

    await loadProjects();

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
  scheduleVersionRefresh(docId, 0);
}

export function teardownAppSession() {
  for (const unlisten of appSessionState.unlistenFns) {
    unlisten();
  }
  appSessionState.unlistenFns = [];
  appSessionState.ready = false;
}
