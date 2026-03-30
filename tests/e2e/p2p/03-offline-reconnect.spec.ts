import { expect } from '@wdio/globals';

import {
  getSyncSnapshot,
  readEditorText,
  setNetworkBlocked,
  typeInEditor,
  waitForConnectionState,
  waitForEditorText,
  waitForUnsentChanges,
} from './helpers/app.js';
import { setupSharedProject } from './helpers/session.js';

describe('p2p offline and reconnect', () => {
  it('keeps a local edit and syncs it after both peers reconnect', async () => {
    const project = await setupSharedProject({
      invitee: 'editor',
      noteTitle: 'offline-note',
      initialText: 'online baseline',
    });

    await setNetworkBlocked('owner', true);
    await setNetworkBlocked('editor', true);

    await waitForConnectionState('editor', 'offline');
    await typeInEditor('editor', ' editor offline change');

    await expect(await readEditorText('editor')).toContain('editor offline change');
    await expect(await readEditorText('owner')).not.toContain('editor offline change');

    await setNetworkBlocked('owner', false);
    await setNetworkBlocked('editor', false);

    await waitForEditorText('owner', 'editor offline change');
    await waitForEditorText(project.invitee, 'editor offline change');
    await waitForConnectionState('owner', 'connected');
    await waitForConnectionState('editor', 'connected');
  });

  it('surfaces unsent changes while offline and clears them after reconnect', async () => {
    await setupSharedProject({
      invitee: 'editor',
      noteTitle: 'unsent-note',
      initialText: 'baseline',
    });

    await setNetworkBlocked('owner', true);
    await setNetworkBlocked('editor', true);
    await waitForConnectionState('editor', 'offline');

    await typeInEditor('editor', ' unsent change');
    await waitForUnsentChanges('editor', 1);

    const snapshot = await getSyncSnapshot('editor');
    await expect(snapshot?.unsentChanges ?? 0).toBeGreaterThan(0);

    await setNetworkBlocked('owner', false);
    await setNetworkBlocked('editor', false);
    await waitForEditorText('owner', 'unsent change');

    await browser.waitUntil(async () => {
      const next = await getSyncSnapshot('editor');
      return (next?.unsentChanges ?? 0) === 0;
    }, {
      timeout: 30_000,
      interval: 250,
      timeoutMsg: 'editor unsent changes never drained after reconnect',
    });
  });
});
