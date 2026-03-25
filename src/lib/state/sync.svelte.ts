import type { ConnectionStatus, SyncStatus } from '../types/index.js';

export const syncState = $state({
  connection: 'local' as ConnectionStatus,
  peerCount: 0,
  unsentChanges: 0,
  isSharedProject: false,
});

export function setSharedProject(shared: boolean) {
  syncState.isSharedProject = shared;
  // Re-evaluate connection status
  if (!shared && syncState.peerCount === 0) {
    syncState.connection = 'local';
  }
}

export function setPeerCount(count: number) {
  syncState.peerCount = count;
  if (count === 0) {
    // No peers: show 'offline' only if this is a shared project, otherwise 'local'
    syncState.connection = syncState.isSharedProject ? 'offline' : 'local';
  } else if (count > 0 && (syncState.connection === 'offline' || syncState.connection === 'local')) {
    syncState.connection = syncState.unsentChanges > 0 ? 'slow' : 'connected';
  }
}

export function applySyncState(syncStatus: SyncStatus, unsentChanges: number) {
  syncState.unsentChanges = unsentChanges;

  // 'local-only' from backend means no peers configured — show as 'local', not 'offline'
  if (syncStatus === 'local-only') {
    if (syncState.peerCount === 0 && !syncState.isSharedProject) {
      syncState.connection = 'local';
    } else {
      syncState.connection = 'offline';
    }
    return;
  }

  if (syncState.peerCount === 0) {
    syncState.connection = syncState.isSharedProject ? 'offline' : 'local';
    return;
  }

  if (syncStatus === 'syncing' || unsentChanges > 0) {
    syncState.connection = 'slow';
    return;
  }

  syncState.connection = 'connected';
}
