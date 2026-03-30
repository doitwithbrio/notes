import { createNote, createProject, expectJoinFailure, expectProjectNotVisible, generateInvite, joinProject, openJoinedProject, waitForAppReady, waitForInviteExpired, waitForNoteVisible, waitForProjectVisible } from './helpers/app.js';
import { uniqueName } from './helpers/runtime.js';

describe('p2p invite robustness', () => {
  it('shows a clear failure when the invite code is wrong', async () => {
    const projectName = uniqueName('invite-wrong-code');
    const noteTitle = 'shared';

    await waitForAppReady('owner');
    await waitForAppReady('editor');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);

    const invite = await generateInvite('owner', 'editor');
    await joinProject('editor', `${invite.passphrase}-wrong`, invite.peerId);

    await expectJoinFailure('editor', 'wrong invite code');
    await expectProjectNotVisible('editor', projectName);
  });

  it('rejects reusing the same invite code after a successful join', async () => {
    const projectName = uniqueName('invite-reuse');
    const noteTitle = 'shared';

    await waitForAppReady('owner');
    await waitForAppReady('editor');
    await waitForAppReady('viewer');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);

    const invite = await generateInvite('owner', 'editor');

    await joinProject('editor', invite.passphrase, invite.peerId);
    await openJoinedProject('editor');
    await waitForProjectVisible('editor', projectName);
    await waitForNoteVisible('editor', noteTitle);

    await joinProject('viewer', invite.passphrase, invite.peerId);

    await expectJoinFailure('viewer');
    await expectProjectNotVisible('viewer', projectName);
  });

  it.skip('exhausts invite attempts after repeated wrong codes from the same invitee', async () => {
    const projectName = uniqueName('invite-exhaust');
    const noteTitle = 'shared';

    await waitForAppReady('owner');
    await waitForAppReady('editor');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);

    const invite = await generateInvite('owner', 'editor');

    for (let attempt = 0; attempt < 3; attempt += 1) {
      await joinProject('editor', `${invite.passphrase}-bad-${attempt}`, invite.peerId);
      await expectJoinFailure('editor');
    }

    await joinProject('editor', invite.passphrase, invite.peerId);
    await expectJoinFailure('editor');
    await expectProjectNotVisible('editor', projectName);
  });

  it('rejects expired invite codes', async () => {
    const projectName = uniqueName('invite-expired');
    const noteTitle = 'shared';

    await waitForAppReady('owner');
    await waitForAppReady('editor');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);

    const invite = await generateInvite('owner', 'editor');
    await waitForInviteExpired('owner');

    await joinProject('editor', invite.passphrase, invite.peerId);
    await expectJoinFailure('editor');
    await expectProjectNotVisible('editor', projectName);
  });
});
