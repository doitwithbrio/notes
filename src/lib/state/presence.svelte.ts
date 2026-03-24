import type { Peer, CursorPosition } from '../types/index.js';

export const presenceState = $state({
  peers: [] as Peer[],
  cursors: [] as CursorPosition[],
});

export function getOnlinePeers(): Peer[] {
  return presenceState.peers.filter((p) => p.online);
}
