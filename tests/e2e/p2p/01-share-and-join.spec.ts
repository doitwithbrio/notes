import { expect } from '@wdio/globals';

import {
  createNote,
  createProject,
  generateInvite,
  joinProject,
  openJoinedProject,
  openNote,
  readEditorText,
  typeInEditor,
  waitForAppReady,
  waitForNoteVisible,
  waitForProjectVisible,
} from './helpers/app.js';
import { uniqueName } from './helpers/runtime.js';

describe('p2p share and join', () => {
  it('lets an editor join a shared project and see the owner note content', async () => {
    const projectName = uniqueName('alpha');
    const noteTitle = 'welcome';

    await waitForAppReady('owner');
    await waitForAppReady('editor');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);
    await openNote('owner', noteTitle);
    await typeInEditor('owner', 'hello from owner');

    const invite = await generateInvite('owner', 'editor');
    await joinProject('editor', invite.passphrase, invite.peerId);
    await openJoinedProject('editor');

    await waitForProjectVisible('editor', projectName);
    await waitForNoteVisible('editor', noteTitle);
    await openNote('editor', noteTitle);

    await expect(await readEditorText('editor')).toContain('hello from owner');
  });
});
