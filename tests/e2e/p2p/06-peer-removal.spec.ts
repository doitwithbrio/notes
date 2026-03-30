import { expect } from '@wdio/globals';

import { expectEditorTextNotToContainWithin, isEditorEditable, readEditorText, removePeer, typeInEditor, waitForEditorText, waitForPeerRevoked } from './helpers/app.js';
import { setupSharedProject } from './helpers/session.js';

describe('p2p peer removal', () => {
  it('stops a removed editor from receiving new owner updates', async () => {
    const shared = await setupSharedProject({
      invitee: 'editor',
      noteTitle: 'removal-receive',
      initialText: 'before removal',
    });

    await removePeer('owner', shared.inviteePeerId);
    await waitForPeerRevoked('owner', shared.projectName, shared.inviteePeerId);
    await typeInEditor('owner', ' owner post-removal update');

    await expectEditorTextNotToContainWithin('editor', 'owner post-removal update', 6_000);
    await expect(await readEditorText('owner')).toContain('owner post-removal update');
  });

  it('prevents a removed editor from syncing changes back to the owner', async () => {
    const shared = await setupSharedProject({
      invitee: 'editor',
      noteTitle: 'removal-send',
      initialText: 'baseline',
    });

    await removePeer('owner', shared.inviteePeerId);
    await waitForPeerRevoked('owner', shared.projectName, shared.inviteePeerId);
    await typeInEditor('editor', ' editor should stay isolated');

    await expect(await readEditorText('editor')).toContain('editor should stay isolated');
    await expectEditorTextNotToContainWithin('owner', 'editor should stay isolated', 6_000);
  });

  it('keeps viewers read-only while still receiving owner updates', async () => {
    await setupSharedProject({
      invitee: 'viewer',
      noteTitle: 'viewer-stays-current',
      initialText: 'viewer baseline',
    });

    await expect(await isEditorEditable('viewer')).toBe(false);

    await typeInEditor('owner', ' owner pushes update');
    await waitForEditorText('viewer', 'owner pushes update');

    await expect(await readEditorText('viewer')).toContain('owner pushes update');
    await expect(await isEditorEditable('viewer')).toBe(false);
  });
});
