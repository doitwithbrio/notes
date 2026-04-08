import {
  createProject,
  openProject,
  setNetworkBlocked,
  waitForConnectionState,
  waitForFileActivePeerCount,
  waitForPeerMissing,
  waitForPeerRow,
  waitForPeersEmpty,
  waitForPeerState,
} from './helpers/app.js';
import { setupSharedProject } from './helpers/session.js';
import { uniqueName } from './helpers/runtime.js';

describe('p2p peer roster', () => {
  it('shows none for an owner-only project', async () => {
    const projectName = uniqueName('solo');

    await createProject('owner', projectName);
    await openProject('owner', projectName);
    await waitForPeersEmpty('owner');
  });

  it('shows accepted members in the project roster and only active members on file rows', async () => {
    const project = await setupSharedProject({
      invitee: 'editor',
      noteTitle: 'shared-note',
      initialText: 'hello',
    });

    await openProject('owner', project.projectName);
    await waitForPeerRow('owner', project.inviteePeerId);
    await waitForPeerState('owner', project.inviteePeerId, 'online');
    await waitForFileActivePeerCount('owner', project.noteTitle, 1);

    await openProject('editor', project.projectName);
    await waitForPeerRow('editor', project.ownerPeerId);
    await waitForPeerMissing('editor', project.inviteePeerId);

    await setNetworkBlocked('owner', true);
    await setNetworkBlocked('editor', true);
    await waitForConnectionState('owner', 'offline');
    await waitForPeerState('owner', project.inviteePeerId, 'offline');

    await waitForFileActivePeerCount('owner', project.noteTitle, 0);

    await setNetworkBlocked('owner', false);
    await setNetworkBlocked('editor', false);
  });
});
