import type { ConnectionStatus, SyncStatus } from '../types/index.js';

export const syncState = $state({
  connection: 'offline' as ConnectionStatus,
  peerCount: 0,
  unsentChanges: 0,
});

export function setPeerCount(count: number) {
  syncState.peerCount = count;
  if (count === 0 && syncState.unsentChanges === 0) {
    syncState.connection = 'offline';
  }
}

export function applySyncState(syncStatus: SyncStatus, unsentChanges: number) {
  syncState.unsentChanges = unsentChanges;
  if (syncState.peerCount === 0) {
    syncState.connection = 'offline';
    return;
  }

  if (syncStatus === 'syncing' || unsentChanges > 0) {
    syncState.connection = 'slow';
    return;
  }

  syncState.connection = 'connected';
}
