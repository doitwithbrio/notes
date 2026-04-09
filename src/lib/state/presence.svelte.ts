import { CURSOR_COLORS, type BackendPeerStatusSummary, type CursorPosition, type Peer } from '../types/index.js';
import { getDocById, setProjectActivePeers } from './documents.svelte.js';

export const ACTIVE_FILE_PRESENCE_TTL_MS = 6_000;

export const presenceState = $state({
  localPeerId: null as string | null,
  projectPeers: {} as Record<string, Peer[]>,
  cursors: [] as CursorPosition[],
  loadingProjectIds: [] as string[],
  hydratedProjectIds: [] as string[],
});

const pendingPeerStatus = new Map<string, { alias?: string; online?: boolean; activeDoc?: string | null }>();
const peerPresenceSeenAt = new Map<string, number>();
const peerCursorClock = new Map<string, { sessionId: string; sessionStartedAt: number; seq: number }>();

function presenceKey(projectId: string, peerId: string) {
  return `${projectId}:${peerId}`;
}

function isNewerCursorState(
  current: { sessionId: string; sessionStartedAt: number; seq: number } | undefined,
  next: { sessionId: string; sessionStartedAt: number; seq: number },
) {
  if (!current) return true;
  if (current.sessionId === next.sessionId) {
    return next.seq >= current.seq;
  }
  return next.sessionStartedAt >= current.sessionStartedAt;
}

function colorForPeer(peerId: string) {
  const hash = Array.from(peerId).reduce((acc, char) => acc + char.charCodeAt(0), 0);
  return CURSOR_COLORS[hash % CURSOR_COLORS.length] ?? CURSOR_COLORS[0];
}

function normalizeAlias(alias: string | null | undefined) {
  return alias?.trim() ? alias : 'peer';
}

function syncProjectDocPresence(projectId: string) {
  const peerToDocMap = new Map<string, string | null>();
  for (const peer of getVisibleProjectPeers(projectId)) {
    if (!peer.online || !peer.activeDoc) continue;
    const doc = getDocById(peer.activeDoc);
    if (!doc || doc.projectId !== projectId) continue;
    peerToDocMap.set(peer.id, peer.activeDoc);
  }
  setProjectActivePeers(projectId, peerToDocMap);
}

function setProjectPeers(projectId: string, peers: Peer[]) {
  presenceState.projectPeers[projectId] = peers;
  if (!presenceState.hydratedProjectIds.includes(projectId)) {
    presenceState.hydratedProjectIds = [...presenceState.hydratedProjectIds, projectId];
  }
  presenceState.loadingProjectIds = presenceState.loadingProjectIds.filter((id) => id !== projectId);
  syncProjectDocPresence(projectId);
}

function updateProjectPeer(projectId: string, peerId: string, updater: (peer: Peer) => void) {
  const peers = presenceState.projectPeers[projectId];
  if (!peers) return false;
  const peer = peers.find((entry) => entry.id === peerId);
  if (!peer) return false;
  updater(peer);
  syncProjectDocPresence(projectId);
  return true;
}

function applyPendingState(peer: Peer) {
  const key = presenceKey(peer.projectId, peer.id);
  const pending = pendingPeerStatus.get(key);
  if (!pending) return peer;

  peer.alias = normalizeAlias(pending.alias ?? peer.alias);
  if (pending.online !== undefined) {
    peer.online = pending.online;
  }
  if (!peer.online) {
    peer.activeDoc = null;
  } else if (pending.activeDoc !== undefined) {
    const doc = pending.activeDoc ? getDocById(pending.activeDoc) : null;
    peer.activeDoc = doc?.projectId === peer.projectId ? pending.activeDoc : null;
  }
  pendingPeerStatus.delete(key);
  return peer;
}

export function setLocalPeerId(peerId: string) {
  presenceState.localPeerId = peerId;
}

export function markProjectPeersLoading(projectId: string) {
  if (!presenceState.loadingProjectIds.includes(projectId)) {
    presenceState.loadingProjectIds = [...presenceState.loadingProjectIds, projectId];
  }
}

export function clearProjectPeersLoading(projectId: string) {
  presenceState.loadingProjectIds = presenceState.loadingProjectIds.filter((id) => id !== projectId);
}

export function isProjectPeersLoading(projectId: string | null) {
  return projectId ? presenceState.loadingProjectIds.includes(projectId) : false;
}

export function getProjectPeers(projectId: string | null): Peer[] {
  if (!projectId) return [];
  return presenceState.projectPeers[projectId] ?? [];
}

export function getVisibleProjectPeers(projectId: string | null): Peer[] {
  return getProjectPeers(projectId).filter((peer) => !peer.isSelf);
}

export function getProjectPeerById(projectId: string | null, peerId: string): Peer | null {
  return getProjectPeers(projectId).find((peer) => peer.id === peerId) ?? null;
}

export function getRemoteCursorsForDoc(docId: string | null): CursorPosition[] {
  if (!docId) return [];
  return presenceState.cursors.filter((cursor) => cursor.docId === docId);
}

export function getOnlinePeers(): Peer[] {
  const seen = new Map<string, Peer>();
  for (const peers of Object.values(presenceState.projectPeers)) {
    for (const peer of peers) {
      if (!peer.online || peer.isSelf || seen.has(peer.id)) continue;
      seen.set(peer.id, peer);
    }
  }
  return [...seen.values()];
}

export function hasAnySharedPeers() {
  return Object.values(presenceState.projectPeers).some((peers) => peers.some((peer) => !peer.isSelf));
}

export function hydrateProjectPeers(projectId: string, peers: BackendPeerStatusSummary[]) {
  const now = Date.now();
  const nextPeers = peers.map((peer) => ({
    id: peer.peerId,
    projectId,
    alias: normalizeAlias(peer.alias),
    online: peer.connected,
    cursorColor: colorForPeer(peer.peerId),
    role: peer.role,
    activeDoc: peer.connected ? peer.activeDoc : null,
    isSelf: peer.isSelf || peer.peerId === presenceState.localPeerId,
  })).map(applyPendingState).map((peer) => {
    if (peer.activeDoc) {
      peerPresenceSeenAt.set(presenceKey(projectId, peer.id), now);
    }
    return peer;
  });
  setProjectPeers(projectId, nextPeers);
}

export function clearProjectPeers(projectId: string) {
  for (const peer of presenceState.projectPeers[projectId] ?? []) {
    peerPresenceSeenAt.delete(presenceKey(projectId, peer.id));
    peerCursorClock.delete(presenceKey(projectId, peer.id));
  }
  delete presenceState.projectPeers[projectId];
  presenceState.loadingProjectIds = presenceState.loadingProjectIds.filter((id) => id !== projectId);
  presenceState.hydratedProjectIds = presenceState.hydratedProjectIds.filter((id) => id !== projectId);
  setProjectActivePeers(projectId, new Map());
}

export function updatePeerStatus(peerId: string, connected: boolean, alias: string | null) {
  let updated = false;
  for (const projectId of Object.keys(presenceState.projectPeers)) {
    updated = updateProjectPeer(projectId, peerId, (peer) => {
      peer.alias = normalizeAlias(alias ?? peer.alias);
      peer.online = connected;
      if (!connected) {
        peer.activeDoc = null;
        peerPresenceSeenAt.delete(presenceKey(projectId, peer.id));
        peerCursorClock.delete(presenceKey(projectId, peer.id));
      }
    }) || updated;
  }

  if (!connected) {
    presenceState.cursors = presenceState.cursors.filter((cursor) => cursor.peerId !== peerId);
  }

  if (!updated) {
    for (const projectId of Object.keys(presenceState.projectPeers)) {
      const key = presenceKey(projectId, peerId);
      pendingPeerStatus.set(key, {
        ...pendingPeerStatus.get(key),
        alias: normalizeAlias(alias),
        online: connected,
        activeDoc: connected ? pendingPeerStatus.get(key)?.activeDoc : null,
      });
    }
  }
}

export function updatePresence(
  projectId: string,
  peerId: string,
  sessionId: string,
  sessionStartedAt: number,
  seq: number,
  alias: string,
  activeDoc: string | null,
  cursorPos: number | null,
  selection: [number, number] | null,
) {
  if (peerId === presenceState.localPeerId) {
    return;
  }

  const clockKey = presenceKey(projectId, peerId);
  const nextClock = { sessionId, sessionStartedAt, seq };
  if (!isNewerCursorState(peerCursorClock.get(clockKey), nextClock)) {
    return;
  }
  peerCursorClock.set(clockKey, nextClock);

  const activeProjectId = activeDoc ? getDocById(activeDoc)?.projectId ?? projectId : projectId;
  const now = Date.now();
  let updated = false;
  for (const nextProjectId of Object.keys(presenceState.projectPeers)) {
    updated = updateProjectPeer(nextProjectId, peerId, (peer) => {
      peer.alias = normalizeAlias(alias);
      peer.online = true;
      peer.activeDoc = activeProjectId === nextProjectId ? activeDoc : null;
      const key = presenceKey(nextProjectId, peer.id);
      if (peer.activeDoc) {
        peerPresenceSeenAt.set(key, now);
      } else {
        peerPresenceSeenAt.delete(key);
      }
    }) || updated;
  }

  if (!updated) {
    const key = presenceKey(projectId, peerId);
    pendingPeerStatus.set(key, {
      ...pendingPeerStatus.get(key),
      alias: normalizeAlias(alias),
      online: true,
      activeDoc,
    });
  }

  const cursorIndex = presenceState.cursors.findIndex(
    (cursor) => cursor.peerId === peerId && cursor.projectId === projectId,
  );
  if (activeDoc && cursorPos !== null) {
    const nextCursor: CursorPosition = {
      projectId,
      peerId,
      alias: normalizeAlias(alias),
      cursorColor: colorForPeer(peerId),
      sessionId,
      sessionStartedAt,
      seq,
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

export function expireStalePresence(now = Date.now()) {
  const stalePeerIds = new Set<string>();
  for (const [peerKey, seenAt] of peerPresenceSeenAt.entries()) {
    if (now - seenAt >= ACTIVE_FILE_PRESENCE_TTL_MS) {
      stalePeerIds.add(peerKey);
      peerPresenceSeenAt.delete(peerKey);
      peerCursorClock.delete(peerKey);
    }
  }

  if (stalePeerIds.size === 0) {
    return;
  }

  for (const projectId of Object.keys(presenceState.projectPeers)) {
    let changed = false;
    for (const peer of presenceState.projectPeers[projectId] ?? []) {
      if (!stalePeerIds.has(presenceKey(projectId, peer.id)) || !peer.activeDoc) continue;
      peer.activeDoc = null;
      changed = true;
    }
    if (changed) {
      syncProjectDocPresence(projectId);
    }
  }

  presenceState.cursors = presenceState.cursors.filter(
    (cursor) => !stalePeerIds.has(presenceKey(cursor.projectId, cursor.peerId))
      && now - cursor.lastActive < ACTIVE_FILE_PRESENCE_TTL_MS,
  );
}
