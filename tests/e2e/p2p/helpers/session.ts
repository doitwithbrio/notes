import type { AppInstanceName } from './runtime.js';
import { uniqueName } from './runtime.js';
import {
  createNote,
  createProject,
  generateInvite,
  getPeerId,
  joinProject,
  openJoinedProject,
  openNote,
  setNetworkBlocked,
  typeInEditor,
  waitForAppReady,
  waitForConnectionState,
  waitForEditorText,
  waitForNoteVisible,
  waitForPeerRow,
  waitForProjectVisible,
} from './app.js';

export async function launchPair() {
  await waitForAppReady('owner');
  await waitForAppReady('editor');
  await setNetworkBlocked('owner', false);
  await setNetworkBlocked('editor', false);
  return { owner: 'owner' as const, editor: 'editor' as const };
}

export async function launchTrio() {
  await waitForAppReady('owner');
  await waitForAppReady('editor');
  await waitForAppReady('viewer');
  await setNetworkBlocked('owner', false);
  await setNetworkBlocked('editor', false);
  await setNetworkBlocked('viewer', false);
  return { owner: 'owner' as const, editor: 'editor' as const, viewer: 'viewer' as const };
}

export async function setupSharedProject(options?: {
  invitee?: 'editor' | 'viewer';
  projectName?: string;
  noteTitle?: string;
  initialText?: string;
}) {
  const invitee = options?.invitee ?? 'editor';
  const projectName = options?.projectName ?? uniqueName(`p2p-${invitee}`);
  const noteTitle = options?.noteTitle ?? 'shared';
  const initialText = options?.initialText ?? 'hello from owner';

  await waitForAppReady('owner');
  await waitForAppReady(invitee);
  await setNetworkBlocked('owner', false);
  await setNetworkBlocked(invitee, false);

  const ownerPeerId = await getPeerId('owner');

  await createProject('owner', projectName);
  await createNote('owner', projectName, noteTitle);
  await openNote('owner', noteTitle);
  await typeInEditor('owner', initialText);

  const invite = await generateInvite('owner', invitee);
  await joinProject(invitee, invite.passphrase, invite.peerId);
  await openJoinedProject(invitee);
  await waitForProjectVisible(invitee, projectName);
  await waitForNoteVisible(invitee, noteTitle);
  await openNote(invitee, noteTitle);
  const inviteePeerId = await getPeerId(invitee);
  if (!ownerPeerId || !inviteePeerId) {
    throw new Error('Failed to load peer IDs for shared project setup');
  }
  await waitForPeerRow('owner', inviteePeerId);
  await waitForEditorText(invitee, initialText);
  await waitForConnectionState('owner', 'connected');
  await waitForConnectionState(invitee, 'connected');

  return {
    owner: 'owner' as const,
    invitee,
    ownerPeerId,
    inviteePeerId,
    projectName,
    noteTitle,
    invite,
  };
}

export async function openSharedNote(name: AppInstanceName, noteTitle: string) {
  await waitForNoteVisible(name, noteTitle);
  await openNote(name, noteTitle);
}
