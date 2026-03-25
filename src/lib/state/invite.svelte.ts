import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import { loadProjects } from './projects.svelte.js';
import { loadProjectDocs } from './documents.svelte.js';
import type { GenerateInviteResult, AcceptInviteResult } from '../types/index.js';

export const inviteState = $state({
  // Generate flow (owner side)
  generating: false,
  activeInvite: null as GenerateInviteResult | null,
  generateError: null as string | null,
  localPeerId: null as string | null,

  // Accept flow (invitee side)
  joinDialogOpen: false,
  accepting: false,
  acceptError: null as string | null,
  acceptResult: null as AcceptInviteResult | null,

  // Share dialog visibility
  shareDialogOpen: false,
  shareProjectId: null as string | null,
  inviteRole: 'editor' as 'editor' | 'viewer',
});

export function openShareDialog(projectId: string, role: 'editor' | 'viewer' = 'editor') {
  inviteState.shareProjectId = projectId;
  inviteState.shareDialogOpen = true;
  inviteState.inviteRole = role;
  inviteState.activeInvite = null;
  inviteState.generateError = null;
}

export async function generateInvite(projectId: string, role: 'editor' | 'viewer') {
  inviteState.generating = true;
  inviteState.generateError = null;
  inviteState.activeInvite = null;
  inviteState.shareProjectId = projectId;
  inviteState.shareDialogOpen = true;
  inviteState.inviteRole = role;

  try {
    // Fetch local peer ID for display
    try {
      inviteState.localPeerId = await tauriApi.getPeerId();
    } catch {
      inviteState.localPeerId = null;
    }

    const result = await tauriApi.generateInvite(projectId, role);
    inviteState.activeInvite = result;
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) {
      // Dev mode — show mock data for testing
      inviteState.activeInvite = {
        passphrase: 'tiger marble ocean violet canyon frost',
        peerId: 'mock-peer-id-for-dev',
        expiresAt: new Date(Date.now() + 10 * 60 * 1000).toISOString(),
      };
      inviteState.localPeerId = 'mock-local-peer-id';
      return;
    }
    inviteState.generateError = error instanceof Error ? error.message : 'failed to generate invite';
  } finally {
    inviteState.generating = false;
  }
}

export function closeShareDialog() {
  inviteState.shareDialogOpen = false;
  inviteState.activeInvite = null;
  inviteState.generateError = null;
  inviteState.shareProjectId = null;
  inviteState.localPeerId = null;
  inviteState.inviteRole = 'editor';
}

export function openJoinDialog() {
  inviteState.joinDialogOpen = true;
  inviteState.acceptError = null;
  inviteState.acceptResult = null;
  inviteState.accepting = false;
}

export function closeJoinDialog() {
  inviteState.joinDialogOpen = false;
  inviteState.acceptError = null;
  inviteState.acceptResult = null;
  inviteState.accepting = false;
}

export async function acceptInvite(passphrase: string, ownerPeerId: string) {
  inviteState.accepting = true;
  inviteState.acceptError = null;
  inviteState.acceptResult = null;

  try {
    const result = await tauriApi.acceptInvite(passphrase.trim(), ownerPeerId.trim());
    inviteState.acceptResult = result;

    // Reload projects and docs to include the newly joined project
    await loadProjects();
    await loadProjectDocs(result.projectId, { force: true, connectPeers: true });
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) {
      // Dev mode — show mock success
      inviteState.acceptResult = {
        projectId: 'mock-project',
        projectName: 'Mock Shared Project',
        role: 'editor',
      };
      return;
    }
    inviteState.acceptError = error instanceof Error ? error.message : 'failed to join project';
  } finally {
    inviteState.accepting = false;
  }
}

export async function removePeer(projectId: string, peerId: string) {
  await tauriApi.removePeer(projectId, peerId);
}
