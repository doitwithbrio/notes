import { expect, multiremotebrowser } from '@wdio/globals';

import { waitForAppReady } from './helpers/app.js';

describe('p2p desktop smoke', () => {
  it('loads the owner, editor, and viewer app shells', async () => {
    await waitForAppReady('owner');
    await waitForAppReady('editor');
    await waitForAppReady('viewer');

    await expect(await multiremotebrowser.getInstance('owner').$('[data-testid="app-shell"]')).toBeDisplayed();
    await expect(await multiremotebrowser.getInstance('editor').$('[data-testid="app-shell"]')).toBeDisplayed();
    await expect(await multiremotebrowser.getInstance('viewer').$('[data-testid="app-shell"]')).toBeDisplayed();
  });
});
