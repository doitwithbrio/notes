import { expect, multiremotebrowser } from '@wdio/globals';

import {
  createNote,
  createProject,
  generateInvite,
  joinProject,
  openJoinedProject,
  openNote,
  typeInEditor,
  waitForAppReady,
  waitForEditorText,
  waitForNoteVisible,
} from './helpers/app.js';
import { uniqueName } from './helpers/runtime.js';

describe('p2p basic sync', () => {
  it('syncs owner edits to the joined editor without reopening the note', async () => {
    const projectName = uniqueName('sync-owner-to-editor');
    const noteTitle = 'shared';

    await waitForAppReady('owner');
    await waitForAppReady('editor');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);

    const invite = await generateInvite('owner', 'editor');
    await joinProject('editor', invite.passphrase, invite.peerId);
    await openJoinedProject('editor');
    await waitForNoteVisible('editor', noteTitle);

    await openNote('owner', noteTitle);
    await openNote('editor', noteTitle);
    await typeInEditor('owner', 'owner update');

    await waitForEditorText('editor', 'owner update');
    await expect(await readCurrentText('editor')).toContain('owner update');
  });

  it('syncs editor edits back to the owner', async () => {
    const projectName = uniqueName('sync-editor-to-owner');
    const noteTitle = 'reply';

    await waitForAppReady('owner');
    await waitForAppReady('editor');

    await createProject('owner', projectName);
    await createNote('owner', projectName, noteTitle);

    const invite = await generateInvite('owner', 'editor');
    await joinProject('editor', invite.passphrase, invite.peerId);
    await openJoinedProject('editor');
    await waitForNoteVisible('editor', noteTitle);

    await openNote('owner', noteTitle);
    await openNote('editor', noteTitle);
    await typeInEditor('editor', 'invitee update');

    await waitForEditorText('owner', 'invitee update');
    await expect(await readCurrentText('owner')).toContain('invitee update');
  });
});

async function readCurrentText(name: 'owner' | 'editor') {
  const editor = await multiremotebrowser.getInstance(name).$('[data-testid="editor-mount"] .editor-content');
  return editor.getText();
}
