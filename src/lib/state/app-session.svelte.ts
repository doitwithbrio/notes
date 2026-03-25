import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError, waitForTauriRuntime } from '../runtime/tauri.js';
import { loadAllProjectDocs, markDocUnread, setDocSyncStatus } from './documents.svelte.js';
import { loadDeviceActorId, loadVersions } from './versions.svelte.js';
import { getOnlinePeers, hydrateProjectPeers, updatePeerStatus, updatePresence } from './presence.svelte.js';
import { loadOrder } from './ordering.svelte.js';
import { loadProjects, projectState } from './projects.svelte.js';
import { applySyncState, setPeerCount, setSharedProject } from './sync.svelte.js';
import { getActiveSession, reloadActiveSession } from '../session/editor-session.svelte.js';

export const appSessionState = $state({
  booting: false,
  ready: false,
  error: null as string | null,
  unlistenFns: [] as Array<() => void>,
});

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
          // Refresh versions sidebar after remote changes
          if (active.projectId) {
            await loadVersions(docId);
          }
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

    // Load stable device actor ID for version system
    await loadDeviceActorId();

    // Load saved ordering from localStorage before fetching projects/docs
    loadOrder();

    await loadProjects();
    await loadAllProjectDocs(projectState.projects.map((project) => project.id));

    let totalPeers = 0;
    let anyShared = false;
    for (const project of projectState.projects) {
      const peers = await tauriApi.getPeerStatus(project.id);
      totalPeers += peers.filter((peer) => peer.connected).length;
      if (peers.length > 0) anyShared = true;
      hydrateProjectPeers(project.id, peers);
    }
    setSharedProject(anyShared);
    setPeerCount(totalPeers);

    appSessionState.ready = true;
  } catch (error) {
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      appSessionState.error = error instanceof Error ? error.message : 'Failed to initialize app';
    }
  } finally {
    appSessionState.booting = false;
  }
}

export async function loadDocSidebars(projectId: string, docId: string) {
  await loadVersions(docId);
}

export function teardownAppSession() {
  for (const unlisten of appSessionState.unlistenFns) {
    unlisten();
  }
  appSessionState.unlistenFns = [];
  appSessionState.ready = false;
}
