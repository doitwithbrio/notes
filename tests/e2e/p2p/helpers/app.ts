import { multiremotebrowser } from '@wdio/globals';

import type { AppInstanceName } from './runtime.js';
import {
  selectors,
  peerRemoveConfirmSelector,
  peerRemoveTriggerSelector,
  peerRowSelector,
  projectAddNoteSelector,
  projectOpenSelector,
} from './selectors.js';

type SyncSnapshot = {
  connection: string;
  peerCount: number;
  unsentChanges: number;
  isSharedProject: boolean;
};

type E2EBridge = {
  isReady(): boolean;
  setNetworkBlocked(blocked: boolean): Promise<void>;
  getSyncState(): SyncSnapshot;
  getPeerId(): Promise<string>;
  getProjectPeerIds(project: string): Promise<string[]>;
};

function app(name: AppInstanceName) {
  return multiremotebrowser.getInstance(name);
}

export async function waitForAppReady(name: AppInstanceName) {
  await (await app(name).$(selectors.appShell)).waitForDisplayed({ timeout: 30_000 });
  await app(name).waitUntil(
    async () => app(name).execute(() => (window as { __P2P_E2E__?: E2EBridge }).__P2P_E2E__?.isReady() ?? false),
    { timeout: 30_000, interval: 250, timeoutMsg: `${name} bridge never reported ready` },
  );
  await app(name).waitUntil(async () => !(await (await app(name).$(selectors.workspaceLoading)).isExisting()), {
    timeout: 30_000,
    interval: 250,
    timeoutMsg: `${name} never became ready`,
  });
}

export async function setNetworkBlocked(name: AppInstanceName, blocked: boolean) {
  await app(name).execute(async (nextBlocked) => {
    await (window as { __P2P_E2E__?: E2EBridge }).__P2P_E2E__?.setNetworkBlocked(Boolean(nextBlocked));
  }, blocked);
}

export async function getSyncSnapshot(name: AppInstanceName) {
  return (await app(name).execute(() => (window as { __P2P_E2E__?: E2EBridge }).__P2P_E2E__?.getSyncState() ?? null)) as SyncSnapshot | null;
}

export async function getPeerId(name: AppInstanceName) {
  return (await app(name).execute(async () => (window as { __P2P_E2E__?: E2EBridge }).__P2P_E2E__?.getPeerId() ?? null)) as string | null;
}

export async function getProjectPeerIds(name: AppInstanceName, projectName: string) {
  return (await app(name).execute(async (nextProjectName) => {
    return (window as { __P2P_E2E__?: E2EBridge }).__P2P_E2E__?.getProjectPeerIds(String(nextProjectName)) ?? [];
  }, projectName)) as string[];
}

export async function waitForUnsentChanges(name: AppInstanceName, minimum: number) {
  await app(name).waitUntil(async () => {
    const snapshot = await getSyncSnapshot(name);
    return Boolean(snapshot && snapshot.unsentChanges >= minimum);
  }, {
    timeout: 30_000,
    interval: 250,
    timeoutMsg: `${name} never reached unsentChanges >= ${minimum}`,
  });
}

export async function createProject(name: AppInstanceName, projectName: string) {
  await (await app(name).$(selectors.createProjectTrigger)).click();
  const input = await app(name).$(selectors.projectNameInput);
  await input.waitForDisplayed({ timeout: 10_000 });
  await input.setValue(projectName);
  await app(name).keys('Enter');
  await waitForProjectVisible(name, projectName);
}

export async function waitForProjectVisible(name: AppInstanceName, projectName: string) {
  await (await app(name).$(projectOpenSelector(projectName))).waitForDisplayed({ timeout: 30_000 });
}

export async function expectProjectNotVisible(name: AppInstanceName, projectName: string) {
  await app(name).waitUntil(async () => !(await (await app(name).$(projectOpenSelector(projectName))).isExisting()), {
    timeout: 10_000,
    interval: 250,
    timeoutMsg: `${name} unexpectedly showed project ${projectName}`,
  });
}

export async function openProject(name: AppInstanceName, projectName: string) {
  await (await app(name).$(projectOpenSelector(projectName))).click();
}

export async function createNote(name: AppInstanceName, projectName: string, noteTitle: string) {
  await (await app(name).$(projectAddNoteSelector(projectName))).click();
  const input = await app(name).$(selectors.noteTitleInput);
  await input.waitForDisplayed({ timeout: 10_000 });
  await input.setValue(noteTitle);
  await app(name).keys('Enter');
  await waitForNoteVisible(name, noteTitle);
}

export async function waitForNoteVisible(name: AppInstanceName, noteTitle: string) {
  await (await noteButton(name, noteTitle)).waitForDisplayed({ timeout: 30_000 });
}

export async function openNote(name: AppInstanceName, noteTitle: string) {
  await (await noteButton(name, noteTitle)).click();
  await (await app(name).$(selectors.editorPane)).waitForDisplayed({ timeout: 30_000 });
}

export async function typeInEditor(name: AppInstanceName, text: string) {
  const editor = await app(name).$(selectors.editorMount);
  await editor.waitForDisplayed({ timeout: 30_000 });
  await editor.click();
  await app(name).keys(text.split(''));
}

export async function readEditorText(name: AppInstanceName) {
  const editor = await app(name).$(selectors.editorMount);
  await editor.waitForDisplayed({ timeout: 30_000 });
  return (await editor.getText()).trim();
}

export async function isEditorEditable(name: AppInstanceName) {
  const editor = await app(name).$(selectors.editorMount);
  await editor.waitForDisplayed({ timeout: 30_000 });
  return (await editor.getAttribute('contenteditable')) !== 'false';
}

export async function waitForEditorText(name: AppInstanceName, snippet: string) {
  await app(name).waitUntil(async () => (await readEditorText(name)).includes(snippet), {
    timeout: 45_000,
    interval: 500,
    timeoutMsg: `${name} never showed editor text containing: ${snippet}`,
  });
}

export async function expectEditorTextNotToContainWithin(name: AppInstanceName, snippet: string, timeoutMs: number) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if ((await readEditorText(name)).includes(snippet)) {
      throw new Error(`${name} unexpectedly showed editor text containing: ${snippet}`);
    }
    await app(name).pause(250);
  }
}

export async function openPeersPanel(name: AppInstanceName) {
  const peersSection = await app(name).$(selectors.peersSection);
  if (await peersSection.isExisting()) {
    return;
  }
  await (await app(name).$(selectors.rightSidebarPeersTrigger)).click();
  await peersSection.waitForDisplayed({ timeout: 10_000 });
}

export async function generateInvite(name: AppInstanceName, role: 'editor' | 'viewer') {
  await openPeersPanel(name);
  await (await app(name).$(selectors.peersInviteTrigger)).click();
  await (await app(name).$(selectors.shareDialog)).waitForDisplayed({ timeout: 10_000 });
  const roleButton = role === 'viewer' ? selectors.shareRoleViewer : selectors.shareRoleEditor;
  await (await app(name).$(roleButton)).click();
  await (await app(name).$(selectors.shareGenerate)).click();
  const passphraseEl = await app(name).$(selectors.sharePassphrase);
  await passphraseEl.waitForDisplayed({ timeout: 30_000 });
  const peerIdEl = await app(name).$(selectors.sharePeerId);
  return {
    passphrase: (await passphraseEl.getText()).trim(),
    peerId: (await peerIdEl.getText()).trim(),
  };
}

export async function waitForInviteExpired(name: AppInstanceName) {
  await (await app(name).$(selectors.shareExpired)).waitForDisplayed({ timeout: 30_000 });
}

export async function joinProject(name: AppInstanceName, passphrase: string, ownerPeerId: string) {
  await (await app(name).$(selectors.joinProjectTrigger)).click();
  await (await app(name).$(selectors.joinDialog)).waitForDisplayed({ timeout: 10_000 });
  await (await app(name).$(selectors.joinPassphraseInput)).setValue(passphrase);
  await (await app(name).$(selectors.joinPeerIdInput)).setValue(ownerPeerId);
  await (await app(name).$(selectors.joinSubmit)).click();
}

export async function openJoinedProject(name: AppInstanceName) {
  const openButton = await app(name).$(selectors.joinOpenProject);
  await openButton.waitForDisplayed({ timeout: 30_000 });
  await openButton.click();
}

export async function waitForJoinError(name: AppInstanceName, expectedText?: string) {
  const error = await app(name).$(selectors.joinError);
  await error.waitForDisplayed({ timeout: 30_000 });
  const message = (await error.getText()).trim();
  if (expectedText && !message.toLowerCase().includes(expectedText.toLowerCase())) {
    throw new Error(`Join error mismatch for ${name}. Expected to include "${expectedText}", got "${message}"`);
  }
  return message;
}

export async function expectJoinFailure(name: AppInstanceName, expectedText?: string) {
  await waitForJoinError(name, expectedText);
  await (await app(name).$(selectors.joinDialog)).waitForDisplayed({ timeout: 30_000 });
}

export async function waitForPeerRow(name: AppInstanceName, peerId: string) {
  await openPeersPanel(name);
  await (await app(name).$(peerRowSelector(peerId))).waitForDisplayed({ timeout: 30_000 });
}

export async function removePeer(name: AppInstanceName, peerId: string) {
  await openPeersPanel(name);
  const trigger = await app(name).$(peerRemoveTriggerSelector(peerId));
  await trigger.waitForDisplayed({ timeout: 30_000 });
  await trigger.click();
  const confirm = await app(name).$(peerRemoveConfirmSelector(peerId));
  await confirm.waitForDisplayed({ timeout: 10_000 });
  await confirm.click();
}

export async function waitForPeerMissing(name: AppInstanceName, peerId: string) {
  await app(name).waitUntil(async () => !(await (await app(name).$(peerRowSelector(peerId))).isExisting()), {
    timeout: 30_000,
    interval: 250,
    timeoutMsg: `${name} still shows peer ${peerId}`,
  });
}

export async function waitForPeerRevoked(name: AppInstanceName, projectName: string, peerId: string) {
  await app(name).waitUntil(async () => {
    const peerIds = await getProjectPeerIds(name, projectName);
    return !peerIds.includes(peerId);
  }, {
    timeout: 30_000,
    interval: 250,
    timeoutMsg: `${name} still reports peer ${peerId} in project ${projectName}`,
  });
}

export async function waitForConnectionState(name: AppInstanceName, state: string) {
  await app(name).waitUntil(async () => {
    const current = await (await app(name).$(selectors.connectionStatus)).getAttribute('data-state');
    return current === state;
  }, {
    timeout: 30_000,
    interval: 250,
    timeoutMsg: `${name} never reached connection state ${state}`,
  });
}

async function noteButton(name: AppInstanceName, noteTitle: string) {
  return app(name).$(`[data-note-title="${noteTitle}"]`);
}
