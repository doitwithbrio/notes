import type { ConnectionStatus } from '../types/index.js';

export const syncState = $state({
  connection: 'offline' as ConnectionStatus,
  peerCount: 0,
  unsentChanges: 0,
});
