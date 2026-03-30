import { expect } from '@wdio/globals';

import {
  createNote,
  createProject,
  generateInvite,
  isEditorEditable,
  joinProject,
  openJoinedProject,
  openNote,
  readEditorText,
  typeInEditor,
  waitForAppReady,
  waitForEditorText,
  waitForNoteVisible,
} from './helpers/app.js';
import { uniqueName } from './helpers/runtime.js';

describe('p2p permissions', () => {
  it('keeps viewers read-only while still receiving updates', async () => {
    const projectName = uniqueName('viewer');
    const noteTitle = 'view-only';

    await waitForAppReady('owner');
    await waitForAppReady('viewer');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);
    await openNote('owner', noteTitle);
    await typeInEditor('owner', 'initial owner text');

    const invite = await generateInvite('owner', 'viewer');
    await joinProject('viewer', invite.passphrase, invite.peerId);
    await openJoinedProject('viewer');
    await waitForNoteVisible('viewer', noteTitle);
    await openNote('viewer', noteTitle);

    await expect(await isEditorEditable('viewer')).toBe(false);

    await typeInEditor('owner', ' updated by owner');
    await waitForEditorText('viewer', 'updated by owner');
    await expect(await readEditorText('viewer')).toContain('updated by owner');
  });
});
