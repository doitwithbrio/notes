import { beforeEach, describe, expect, it, vi } from 'vitest';

const documentsMock = vi.hoisted(() => ({
  setProjectActivePeers: vi.fn(),
  getDocById: vi.fn((docId: string | null) => {
    if (docId === 'doc-a') return { id: 'doc-a', projectId: 'project-1' };
    if (docId === 'doc-b') return { id: 'doc-b', projectId: 'project-1' };
    return null;
  }),
}));

vi.mock('./documents.svelte.js', () => ({
  setProjectActivePeers: documentsMock.setProjectActivePeers,
  getDocById: documentsMock.getDocById,
}));

async function loadPresenceModule() {
  vi.resetModules();
  return import('./presence.svelte.js');
}

describe('presence state', () => {
  beforeEach(() => {
    documentsMock.setProjectActivePeers.mockReset();
    documentsMock.getDocById.mockClear();
  });

  it('keeps a flat project roster, excludes self from visible peers, and preserves offline members', async () => {
    const presence = await loadPresenceModule();
    presence.setLocalPeerId('self-peer');

    presence.hydrateProjectPeers('project-1', [
      { peerId: 'owner-peer', alias: 'owner', role: 'owner', connected: true, activeDoc: null, isSelf: false },
      { peerId: 'self-peer', alias: 'me', role: 'editor', connected: true, activeDoc: 'doc-a', isSelf: true },
      { peerId: 'viewer-peer', alias: 'viewer', role: 'viewer', connected: false, activeDoc: null, isSelf: false },
    ]);

    expect(presence.getProjectPeers('project-1').map((peer) => peer.id)).toEqual([
      'owner-peer',
      'self-peer',
      'viewer-peer',
    ]);
    expect(presence.getVisibleProjectPeers('project-1').map((peer) => peer.id)).toEqual([
      'owner-peer',
      'viewer-peer',
    ]);
    expect(presence.getVisibleProjectPeers('project-1').map((peer) => peer.online)).toEqual([true, false]);
  });

  it('derives file activity from roster presence without leaking self or offline peers', async () => {
    const presence = await loadPresenceModule();
    presence.setLocalPeerId('self-peer');

    presence.hydrateProjectPeers('project-1', [
      { peerId: 'owner-peer', alias: 'owner', role: 'owner', connected: true, activeDoc: 'doc-a', isSelf: false },
      { peerId: 'self-peer', alias: 'me', role: 'editor', connected: true, activeDoc: 'doc-a', isSelf: true },
      { peerId: 'viewer-peer', alias: 'viewer', role: 'viewer', connected: false, activeDoc: 'doc-a', isSelf: false },
    ]);

    expect(documentsMock.setProjectActivePeers).toHaveBeenLastCalledWith(
      'project-1',
      new Map([['owner-peer', 'doc-a']]),
    );

    presence.updatePresence('project-1', 'owner-peer', 'session-a', 100, 1, 'owner', 'doc-b', 7, [7, 9]);
    presence.updatePeerStatus('viewer-peer', true, 'viewer');
    presence.updatePresence('project-1', 'viewer-peer', 'session-a', 100, 1, 'viewer', 'doc-a', 3, null);

    expect(documentsMock.setProjectActivePeers).toHaveBeenLastCalledWith(
      'project-1',
      new Map([
        ['owner-peer', 'doc-b'],
        ['viewer-peer', 'doc-a'],
      ]),
    );
  });

  it('replays early status and presence updates after roster hydration', async () => {
    const presence = await loadPresenceModule();
    presence.setLocalPeerId('self-peer');

    presence.updatePeerStatus('owner-peer', true, 'owner');
    presence.updatePresence('project-1', 'owner-peer', 'session-a', 100, 1, 'owner', 'doc-a', 1, null);

    presence.hydrateProjectPeers('project-1', [
      { peerId: 'owner-peer', alias: null, role: 'owner', connected: false, activeDoc: null, isSelf: false },
      { peerId: 'self-peer', alias: 'me', role: 'editor', connected: true, activeDoc: null, isSelf: true },
    ]);

    expect(presence.getVisibleProjectPeers('project-1').map((peer) => ({
      id: peer.id,
      online: peer.online,
      activeDoc: peer.activeDoc,
    }))).toEqual([
      { id: 'owner-peer', online: true, activeDoc: 'doc-a' },
    ]);
  });

  it('expires stale active file presence after the grace window', async () => {
    const presence = await loadPresenceModule();
    presence.setLocalPeerId('self-peer');

    presence.hydrateProjectPeers('project-1', [
      { peerId: 'owner-peer', alias: 'owner', role: 'owner', connected: true, activeDoc: null, isSelf: false },
      { peerId: 'self-peer', alias: 'me', role: 'editor', connected: true, activeDoc: null, isSelf: true },
    ]);

    presence.updatePresence('project-1', 'owner-peer', 'session-a', 100, 1, 'owner', 'doc-a', 4, null);
    presence.expireStalePresence(Date.now() + 6_100);

    expect(presence.getVisibleProjectPeers('project-1').map((peer) => ({
      id: peer.id,
      activeDoc: peer.activeDoc,
      online: peer.online,
    }))).toEqual([
      { id: 'owner-peer', activeDoc: null, online: true },
    ]);
    expect(documentsMock.setProjectActivePeers).toHaveBeenLastCalledWith('project-1', new Map());
  });

  it('keeps recent active file presence stable within the grace window', async () => {
    const presence = await loadPresenceModule();
    presence.setLocalPeerId('self-peer');

    presence.hydrateProjectPeers('project-1', [
      { peerId: 'owner-peer', alias: 'owner', role: 'owner', connected: true, activeDoc: null, isSelf: false },
      { peerId: 'self-peer', alias: 'me', role: 'editor', connected: true, activeDoc: null, isSelf: true },
    ]);

    presence.updatePresence('project-1', 'owner-peer', 'session-a', 100, 1, 'owner', 'doc-a', 4, null);
    presence.expireStalePresence(Date.now() + 2_000);

    expect(presence.getVisibleProjectPeers('project-1').map((peer) => ({
      id: peer.id,
      activeDoc: peer.activeDoc,
    }))).toEqual([
      { id: 'owner-peer', activeDoc: 'doc-a' },
    ]);
  });

  it('ignores stale cursor updates from the same session', async () => {
    const presence = await loadPresenceModule();
    presence.setLocalPeerId('self-peer');

    presence.hydrateProjectPeers('project-1', [
      { peerId: 'owner-peer', alias: 'owner', role: 'owner', connected: true, activeDoc: null, isSelf: false },
      { peerId: 'self-peer', alias: 'me', role: 'editor', connected: true, activeDoc: null, isSelf: true },
    ]);

    presence.updatePresence('project-1', 'owner-peer', 'session-a', 100, 2, 'owner', 'doc-a', 10, [10, 12]);
    presence.updatePresence('project-1', 'owner-peer', 'session-a', 100, 1, 'owner', 'doc-a', 4, [4, 6]);

    expect(presence.getRemoteCursorsForDoc('doc-a').map((cursor) => ({
      peerId: cursor.peerId,
      from: cursor.from,
      to: cursor.to,
      seq: cursor.seq,
    }))).toEqual([
      { peerId: 'owner-peer', from: 10, to: 12, seq: 2 },
    ]);
  });

  it('replaces old-session cursor state when a newer session arrives', async () => {
    const presence = await loadPresenceModule();
    presence.setLocalPeerId('self-peer');

    presence.hydrateProjectPeers('project-1', [
      { peerId: 'owner-peer', alias: 'owner', role: 'owner', connected: true, activeDoc: null, isSelf: false },
      { peerId: 'self-peer', alias: 'me', role: 'editor', connected: true, activeDoc: null, isSelf: true },
    ]);

    presence.updatePresence('project-1', 'owner-peer', 'session-a', 100, 3, 'owner', 'doc-a', 10, [10, 10]);
    presence.updatePresence('project-1', 'owner-peer', 'session-b', 200, 1, 'owner', 'doc-b', 2, [2, 5]);

    expect(presence.getRemoteCursorsForDoc('doc-a')).toEqual([]);
    expect(presence.getRemoteCursorsForDoc('doc-b').map((cursor) => ({
      peerId: cursor.peerId,
      docId: cursor.docId,
      seq: cursor.seq,
      sessionId: cursor.sessionId,
    }))).toEqual([
      { peerId: 'owner-peer', docId: 'doc-b', seq: 1, sessionId: 'session-b' },
    ]);
  });
});
