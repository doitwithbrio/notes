import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import PeersSection from './PeersSection.svelte';

const mockState = vi.hoisted(() => ({
  presenceState: {
    loadingProjectIds: [] as string[],
  },
  getProject: vi.fn(),
  getWorkspaceProjectId: vi.fn(),
  getVisibleProjectPeers: vi.fn(),
  isProjectPeersLoading: vi.fn(),
  openShareDialog: vi.fn(),
  removePeer: vi.fn(),
}));

vi.mock('../../state/presence.svelte.js', () => ({
  presenceState: mockState.presenceState,
  getVisibleProjectPeers: mockState.getVisibleProjectPeers,
  isProjectPeersLoading: mockState.isProjectPeersLoading,
}));

vi.mock('../../state/projects.svelte.js', () => ({
  getProject: mockState.getProject,
}));

vi.mock('../../state/invite.svelte.js', () => ({
  openShareDialog: mockState.openShareDialog,
  removePeer: mockState.removePeer,
}));

vi.mock('../../navigation/workspace-router.svelte.js', () => ({
  getWorkspaceProjectId: mockState.getWorkspaceProjectId,
}));

describe('PeersSection', () => {
  beforeEach(() => {
    mockState.getWorkspaceProjectId.mockReturnValue('project-1');
    mockState.getProject.mockReturnValue({ id: 'project-1', canManagePeers: true });
    mockState.getVisibleProjectPeers.mockReturnValue([]);
    mockState.isProjectPeersLoading.mockReturnValue(false);
    mockState.openShareDialog.mockReset();
    mockState.removePeer.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it('renders one flat roster with offline members included and no self row', () => {
    mockState.getVisibleProjectPeers.mockReturnValue([
      { id: 'owner-peer', alias: 'owner', role: 'owner', online: true, cursorColor: '#00aa00', activeDoc: null, isSelf: false, projectId: 'project-1' },
      { id: 'viewer-peer', alias: 'viewer', role: 'viewer', online: false, cursorColor: '#999999', activeDoc: null, isSelf: false, projectId: 'project-1' },
    ]);

    render(PeersSection);

    expect(screen.getByTestId('peer-row-owner-peer')).toBeTruthy();
    expect(screen.getByTestId('peer-row-viewer-peer')).toBeTruthy();
    expect(screen.getByText('2')).toBeTruthy();
    expect(screen.queryByText(/no peers connected/i)).toBeNull();
  });

  it('shows none only after the resolved roster is empty', () => {
    mockState.isProjectPeersLoading.mockReturnValue(false);
    mockState.getVisibleProjectPeers.mockReturnValue([]);

    render(PeersSection);

    expect(screen.getByText('none')).toBeTruthy();
  });
});
