import { expect } from '@wdio/globals';

import {
  pasteImageIntoEditor,
  readEditorText,
  waitForBlobImageCount,
  waitForBlobImageState,
  waitForEditorText,
} from './helpers/app.js';
import { setupSharedProject } from './helpers/session.js';

const ONE_BY_ONE_PNG_BASE64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9WnSUswAAAAASUVORK5CYII=';

describe('p2p blob images', () => {
  it('lazy fetches and renders a shared image on the joined editor peer', async () => {
    const shared = await setupSharedProject({
      invitee: 'editor',
      noteTitle: 'shared-image',
      initialText: 'image baseline',
    });

    await pasteImageIntoEditor('owner', {
      filename: 'pixel.png',
      mimeType: 'image/png',
      base64: ONE_BY_ONE_PNG_BASE64,
    });
    await waitForBlobImageCount('owner', 1);
    await waitForBlobImageState('owner', 'ready');

    await waitForEditorText('editor', 'image baseline');
    await waitForBlobImageCount(shared.invitee, 1);
    await waitForBlobImageState(shared.invitee, ['loading', 'ready']);
    await waitForBlobImageState(shared.invitee, 'ready');

    await expect(await readEditorText(shared.invitee)).toContain('image baseline');
  });
});
