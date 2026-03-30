import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import ShareDialog from './ShareDialog.svelte';

const mockState = vi.hoisted(() => ({
  inviteState: {
    shareProjectId: 'project-1',
    inviteRole: 'editor' as 'editor' | 'viewer',
    activeInvite: null as null | { inviteId: string; passphrase: string; peerId: string; expiresAt: string },
    localPeerId: 'peer-local',
    generating: false,
    generateError: null as string | null,
    ownerInviteStatuses: [] as Array<{ inviteId?: string; projectId: string; projectName: string; phase: string }>,
  },
  closeShareDialog: vi.fn(),
  generateInvite: vi.fn(async () => undefined),
  getProject: vi.fn(() => ({ id: 'project-1', name: 'Project 1' })),
  clipboardWriteText: vi.fn(async () => undefined),
}));

vi.mock('../../state/invite.svelte.js', () => ({
  inviteState: mockState.inviteState,
  closeShareDialog: mockState.closeShareDialog,
  generateInvite: mockState.generateInvite,
}));

vi.mock('../../state/projects.svelte.js', () => ({
  getProject: mockState.getProject,
}));

describe('ShareDialog', () => {
  beforeEach(() => {
    mockState.inviteState.shareProjectId = 'project-1';
    mockState.inviteState.inviteRole = 'editor';
    mockState.inviteState.activeInvite = null;
    mockState.inviteState.localPeerId = 'peer-local';
    mockState.inviteState.generating = false;
    mockState.inviteState.generateError = null;
    mockState.inviteState.ownerInviteStatuses = [];
    mockState.closeShareDialog.mockReset();
    mockState.generateInvite.mockReset();
    mockState.getProject.mockReset();
    mockState.getProject.mockImplementation(() => ({ id: 'project-1', name: 'Project 1' }));
    vi.useFakeTimers();
    vi.stubGlobal('navigator', { clipboard: { writeText: mockState.clipboardWriteText } } as any);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    cleanup();
  });

  it('closes via backdrop and keeps the panel interactive', async () => {
    render(ShareDialog);

    await fireEvent.click(screen.getByRole('button', { name: /close share dialog/i }));
    expect(mockState.closeShareDialog).toHaveBeenCalledTimes(1);
  });

  it('generates an invite using the selected role', async () => {
    render(ShareDialog);

    await fireEvent.click(screen.getByTestId('share-role-viewer'));
    await fireEvent.click(screen.getByTestId('share-generate'));

    expect(mockState.generateInvite).toHaveBeenCalledWith('project-1', 'viewer');
  });

  it('copies invite fields when an invite is present', async () => {
    mockState.inviteState.activeInvite = {
      inviteId: 'invite-1',
      passphrase: 'tiger-marble',
      peerId: 'peer-remote',
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
    };

    render(ShareDialog);

    await fireEvent.click(screen.getByTestId('share-copy-passphrase'));

    expect(mockState.clipboardWriteText).toHaveBeenCalledWith('tiger-marble');
  });

  it('shows expired status after the invite timer elapses', async () => {
    mockState.inviteState.activeInvite = {
      inviteId: 'invite-1',
      passphrase: 'tiger-marble',
      peerId: 'peer-remote',
      expiresAt: new Date(Date.now() + 1_000).toISOString(),
    };

    render(ShareDialog);

    expect(screen.getByTestId('share-timer').textContent).toContain('expires in');
    await vi.advanceTimersByTimeAsync(2_000);

    await waitFor(() => {
      expect(screen.getByTestId('share-expired').textContent).toContain('invite expired');
    });
  });

  it('clears the active invite when switching roles', async () => {
    mockState.inviteState.activeInvite = {
      inviteId: 'invite-1',
      passphrase: 'tiger-marble',
      peerId: 'peer-remote',
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
    };

    render(ShareDialog);
    await fireEvent.click(screen.getByTestId('share-role-viewer'));

    expect(mockState.inviteState.activeInvite).toBeNull();
  });

  it('shows owner invite status for the matching local project name', async () => {
    mockState.inviteState.activeInvite = {
      inviteId: 'invite-1',
      passphrase: 'tiger-marble',
      peerId: 'peer-remote',
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
    };
    mockState.inviteState.ownerInviteStatuses = [
      { inviteId: 'invite-1', projectId: 'project-1', projectName: 'project-1', phase: 'reserved' },
    ];

    render(ShareDialog);

    expect(screen.getByTestId('share-owner-status').textContent).toContain('waiting for them to finish joining');
  });

  it('maps owner invite phases and hides the pill for open invites', async () => {
    mockState.inviteState.activeInvite = {
      inviteId: 'invite-1',
      passphrase: 'tiger-marble',
      peerId: 'peer-remote',
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
    };

    mockState.inviteState.ownerInviteStatuses = [
      { inviteId: 'invite-1', projectId: 'project-1', projectName: 'project-1', phase: 'committed-pending-ack' },
    ];
    const view = render(ShareDialog);
    expect(screen.getByTestId('share-owner-status').textContent).toContain('adding them to the project');

    view.unmount();
    mockState.inviteState.ownerInviteStatuses = [
      { inviteId: 'invite-1', projectId: 'project-1', projectName: 'project-1', phase: 'consumed' },
    ];
    render(ShareDialog);
    expect(screen.getByTestId('share-owner-status').textContent).toContain('invite complete');
    expect(screen.getByTestId('share-consumed-note').textContent).toContain('this invite has already been used');
    expect(screen.queryByTestId('share-passphrase')).toBeNull();
    expect(screen.queryByTestId('share-peer-id')).toBeNull();

    cleanup();
    mockState.inviteState.ownerInviteStatuses = [
      { inviteId: 'invite-1', projectId: 'project-1', projectName: 'project-1', phase: 'open' },
    ];
    render(ShareDialog);
    expect(screen.queryByTestId('share-owner-status')).toBeNull();
  });
});
