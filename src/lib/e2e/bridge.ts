import { tauriApi } from '../api/tauri.js';
import { appSessionState } from '../state/app-session.svelte.js';
import { syncState } from '../state/sync.svelte.js';

type E2EBridge = {
  setNetworkBlocked(blocked: boolean): Promise<void>;
  getPeerId(): Promise<string>;
  getProjectPeerIds(project: string): Promise<string[]>;
  getSyncState(): {
    connection: typeof syncState.connection;
    peerCount: number;
    unsentChanges: number;
    isSharedProject: boolean;
  };
  isReady(): boolean;
};

declare global {
  interface Window {
    __P2P_E2E__?: E2EBridge;
  }
}

export async function installE2EBridge() {
  const enabled = await tauriApi.e2eIsEnabled().catch(() => false);
  if (!enabled) {
    return;
  }

  window.__P2P_E2E__ = {
    async setNetworkBlocked(blocked: boolean) {
      await tauriApi.e2eSetNetworkBlocked(blocked);
    },
    async getPeerId() {
      return tauriApi.getPeerId();
    },
    async getProjectPeerIds(project: string) {
      const peers = await tauriApi.getPeerStatus(project);
      return peers.map((peer) => peer.peerId);
    },
    getSyncState() {
      return {
        connection: syncState.connection,
        peerCount: syncState.peerCount,
        unsentChanges: syncState.unsentChanges,
        isSharedProject: syncState.isSharedProject,
      };
    },
    isReady() {
      return appSessionState.ready;
    },
  };
}

export function teardownE2EBridge() {
  delete window.__P2P_E2E__;
}
