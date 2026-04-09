import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import JoinDialog from './JoinDialog.svelte';

const mockState = vi.hoisted(() => ({
  inviteState: {
    joinDialogOpen: true,
    acceptResult: null as null | { projectId: string; projectName: string; role: string },
    acceptError: null as string | null,
    accepting: false,
  },
  acceptInvite: vi.fn(async () => undefined),
  closeJoinDialog: vi.fn(),
  navigateToProject: vi.fn(async () => undefined),
}));

vi.mock('../../state/invite.svelte.js', () => ({
  inviteState: mockState.inviteState,
  acceptInvite: mockState.acceptInvite,
  closeJoinDialog: mockState.closeJoinDialog,
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  navigateToProject: mockState.navigateToProject,
}));

describe('JoinDialog', () => {
  beforeEach(() => {
    mockState.inviteState.acceptResult = null;
    mockState.inviteState.acceptError = null;
    mockState.inviteState.accepting = false;
    mockState.acceptInvite.mockReset();
    mockState.closeJoinDialog.mockReset();
    mockState.navigateToProject.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it('focuses the invite code field and closes via backdrop', async () => {
    render(JoinDialog);

    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByTestId('join-passphrase-input'));
    });

    await fireEvent.click(screen.getByRole('button', { name: /close join dialog/i }));
    expect(mockState.closeJoinDialog).toHaveBeenCalledTimes(1);
  });

  it('submits via the form when both fields are present', async () => {
    render(JoinDialog);

    const passphrase = screen.getByTestId('join-passphrase-input');
    const peerId = screen.getByTestId('join-peer-id-input');
    await fireEvent.input(passphrase, { target: { value: 'tiger-marble' } });
    await fireEvent.input(peerId, { target: { value: 'peer-1' } });
    await fireEvent.submit(passphrase.closest('form')!);

    expect(mockState.acceptInvite).toHaveBeenCalledWith('tiger-marble', 'peer-1');
  });

  it('does not submit via the form while accepting or with blank values', async () => {
    render(JoinDialog);

    await fireEvent.submit(screen.getByTestId('join-passphrase-input').closest('form')!);
    expect(mockState.acceptInvite).not.toHaveBeenCalled();

    mockState.inviteState.accepting = true;
    await fireEvent.input(screen.getByTestId('join-passphrase-input'), { target: { value: 'tiger-marble' } });
    await fireEvent.input(screen.getByTestId('join-peer-id-input'), { target: { value: 'peer-1' } });
    await fireEvent.submit(screen.getByTestId('join-peer-id-input').closest('form')!);

    expect(mockState.acceptInvite).not.toHaveBeenCalled();
  });

  it('opens the joined project by local project name', async () => {
    mockState.inviteState.acceptResult = {
      projectId: 'project-123',
      projectName: 'Joined Project',
      role: 'editor',
    };
    render(JoinDialog);

    await fireEvent.click(screen.getByTestId('join-open-project'));

    expect(mockState.navigateToProject).toHaveBeenCalledWith('Joined Project');
    expect(mockState.closeJoinDialog).toHaveBeenCalledTimes(1);
  });
});
