import { CURSOR_COLORS, type BackendPeerStatusSummary, type Peer, type CursorPosition, type PeerRole } from '../types/index.js';
import { setProjectActivePeers } from './documents.svelte.js';

export const presenceState = $state({
  peers: [] as Peer[],
  cursors: [] as CursorPosition[],
});

function colorForPeer(peerId: string) {
  const hash = Array.from(peerId).reduce((acc, char) => acc + char.charCodeAt(0), 0);
  return CURSOR_COLORS[hash % CURSOR_COLORS.length] ?? CURSOR_COLORS[0];
}

function upsertPeerBase(peerId: string, alias: string | null, online: boolean, role?: PeerRole | null) {
  const existing = presenceState.peers.find((peer) => peer.id === peerId);
  if (existing) {
    existing.alias = alias ?? existing.alias ?? 'peer';
    existing.online = online;
    if (role !== undefined) existing.role = role;
    return existing;
  }

  const peer: Peer = {
    id: peerId,
    alias: alias ?? 'peer',
    online,
    cursorColor: colorForPeer(peerId),
    role: role ?? null,
    activeDoc: null,
  };
  presenceState.peers.push(peer);
  return peer;
}

export function getOnlinePeers(): Peer[] {
  return presenceState.peers.filter((peer) => peer.online);
}

export function hydrateProjectPeers(projectId: string, peers: BackendPeerStatusSummary[]) {
  const peerToDocMap = new Map<string, string | null>();
  for (const peer of peers) {
    const state = upsertPeerBase(peer.peerId, peer.alias, peer.connected, peer.role);
    state.activeDoc = peer.activeDoc;
    peerToDocMap.set(peer.peerId, peer.activeDoc);
  }
  setProjectActivePeers(projectId, peerToDocMap);
}

export function updatePeerStatus(peerId: string, connected: boolean, alias: string | null) {
  upsertPeerBase(peerId, alias, connected);
}

export function updatePresence(
  peerId: string,
  alias: string,
  activeDoc: string | null,
  cursorPos: number | null,
  selection: [number, number] | null,
) {
  const peer = upsertPeerBase(peerId, alias, true);
  peer.activeDoc = activeDoc;

  const cursorIndex = presenceState.cursors.findIndex((cursor) => cursor.peerId === peerId);
  if (activeDoc && cursorPos !== null) {
    const nextCursor: CursorPosition = {
      peerId,
      docId: activeDoc,
      from: selection?.[0] ?? cursorPos,
      to: selection?.[1] ?? cursorPos,
      lastActive: Date.now(),
    };
    if (cursorIndex >= 0) {
      presenceState.cursors[cursorIndex] = nextCursor;
    } else {
      presenceState.cursors.push(nextCursor);
    }
  } else if (cursorIndex >= 0) {
    presenceState.cursors.splice(cursorIndex, 1);
  }
}
