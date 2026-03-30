import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import { loadProjects } from './projects.svelte.js';
import { loadProjectDocs } from './documents.svelte.js';
import type {
  AcceptInviteResult,
  BackendInviteAcceptEvent,
  BackendOwnerInviteStatus,
  BackendPendingJoinResume,
  GenerateInviteResult,
} from '../types/index.js';

/** Extract a human-readable message from a Tauri IPC error.
 *  Backend CoreError is serialized as { code, message }, not a JS Error. */
function extractErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (error && typeof error === 'object') {
    const obj = error as Record<string, unknown>;
    if (typeof obj.message === 'string' && obj.message) return obj.message;
    if (typeof obj.code === 'string' && obj.code) return obj.code;
  }
  if (typeof error === 'string' && error) return error;
  return fallback;
}

/** Map raw backend error messages to user-friendly descriptions. */
function friendlyJoinError(raw: string): string {
  const lower = raw.toLowerCase();
  if (lower.includes('invalid owner peer id')) return 'the owner peer ID is not valid — check you pasted it correctly';
  if (lower.includes('connection failed')) return 'could not reach the owner — make sure they have the app open and both peers can connect';
  if (lower.includes('timed out')) return 'connection timed out — the owner may have closed the app or the invite expired';
  if (lower.includes('wrong code') || lower.includes('handshake failed')) return 'wrong invite code — check the code and try again';
  if (lower.includes('connection lost') || lower.includes('stream') || lower.includes('reset')) return 'the owner closed the connection — the invite may have expired, been used already, or the owner restarted';
  if (lower.includes('decrypt failed')) return 'could not decrypt the invite — the code may be wrong';
  return raw;
}

/** Normalize an invite passphrase so both "fund-crow-gale" and
 *  "fund crow gale" are accepted. Canonical form is hyphen-separated. */
function normalizePassphrase(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .split(/[\s\-]+/)
    .filter(Boolean)
    .join('-');
}

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

  // Resume / owner observability
  pendingJoinResumes: [] as BackendPendingJoinResume[],
  latestInviteEvent: null as BackendInviteAcceptEvent | null,
  ownerInviteStatuses: [] as BackendOwnerInviteStatus[],
});

let ownerInvitePollTimer: ReturnType<typeof setInterval> | null = null;
let clearInviteEventTimer: ReturnType<typeof setTimeout> | null = null;

function startOwnerInvitePolling(projectId: string) {
  stopOwnerInvitePolling();
  ownerInvitePollTimer = setInterval(() => {
    void loadOwnerInviteStatuses(projectId);
  }, 2000);
}

function stopOwnerInvitePolling() {
  if (ownerInvitePollTimer) {
    clearInterval(ownerInvitePollTimer);
    ownerInvitePollTimer = null;
  }
}

export function clearInviteBanner() {
  inviteState.latestInviteEvent = null;
}

export async function hydrateInviteStatus() {
  try {
    inviteState.pendingJoinResumes = await tauriApi.listPendingJoinResumes();
  } catch {
    inviteState.pendingJoinResumes = [];
  }
}

export async function loadOwnerInviteStatuses(projectId?: string | null) {
  try {
    inviteState.ownerInviteStatuses = await tauriApi.listOwnerInvites(projectId ?? undefined);
  } catch {
    inviteState.ownerInviteStatuses = [];
  }
}

export async function resumePendingJoins() {
  try {
    await tauriApi.resumePendingJoins();
  } finally {
    await hydrateInviteStatus();
  }
}

export function handleInviteAcceptEvent(event: BackendInviteAcceptEvent) {
  inviteState.latestInviteEvent = event;
  if (clearInviteEventTimer) clearTimeout(clearInviteEventTimer);

  const localProjectName = event.localProjectName ?? event.projectName;
  const nextResume: BackendPendingJoinResume = {
    sessionId: event.sessionId,
    ownerPeerId: event.ownerPeerId,
    projectId: event.projectId,
    projectName: event.projectName,
    localProjectName,
    role: event.role,
    stage:
      event.stage === 'payload-staged'
        ? 'payload-staged'
        : event.stage === 'finalized'
          ? 'finalized'
          : 'commit-confirmed',
    updatedAt: new Date().toISOString(),
  };

  if (event.stage === 'completed' || event.stage === 'failed') {
    inviteState.pendingJoinResumes = inviteState.pendingJoinResumes.filter(
      (resume) => resume.sessionId !== event.sessionId,
    );
    clearInviteEventTimer = setTimeout(() => {
      if (inviteState.latestInviteEvent?.sessionId === event.sessionId) {
        inviteState.latestInviteEvent = null;
      }
    }, 8000);
  } else {
    const idx = inviteState.pendingJoinResumes.findIndex((resume) => resume.sessionId === event.sessionId);
    if (idx >= 0) inviteState.pendingJoinResumes[idx] = nextResume;
    else inviteState.pendingJoinResumes = [...inviteState.pendingJoinResumes, nextResume];
  }
}

export function openShareDialog(projectId: string, role: 'editor' | 'viewer' = 'editor') {
  inviteState.shareProjectId = projectId;
  inviteState.shareDialogOpen = true;
  inviteState.inviteRole = role;
  inviteState.activeInvite = null;
  inviteState.generateError = null;
  void loadOwnerInviteStatuses(projectId);
  startOwnerInvitePolling(projectId);
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
    await loadOwnerInviteStatuses(projectId);
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) {
      // Dev mode — show mock data for testing
      inviteState.activeInvite = {
        inviteId: 'mock-invite-id',
        passphrase: 'tiger-marble-ocean-violet-canyon-frost',
        peerId: 'mock-peer-id-for-dev',
        expiresAt: new Date(Date.now() + 10 * 60 * 1000).toISOString(),
      };
      inviteState.localPeerId = 'mock-local-peer-id';
      return;
    }
    inviteState.generateError = extractErrorMessage(error, 'failed to generate invite');
  } finally {
    inviteState.generating = false;
  }
}

export function closeShareDialog() {
  stopOwnerInvitePolling();
  inviteState.shareDialogOpen = false;
  inviteState.activeInvite = null;
  inviteState.generateError = null;
  inviteState.shareProjectId = null;
  inviteState.localPeerId = null;
  inviteState.inviteRole = 'editor';
  inviteState.ownerInviteStatuses = [];
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
    // Strip ALL whitespace (not just leading/trailing) from the peer ID — a
    // peer ID pasted from a line-wrapped UI or a mobile share sheet may contain
    // internal spaces or newlines that .trim() alone would miss.
    const normalizedPeerId = ownerPeerId.replace(/\s+/g, '');
    const result = await tauriApi.acceptInvite(normalizePassphrase(passphrase), normalizedPeerId);
    inviteState.acceptResult = result;

    // Reload projects and docs to include the newly joined project.
    // Project state/APIs are keyed by project name, not manifest UUID.
    await loadProjects();
    await loadProjectDocs(result.projectName, { force: true, connectPeers: true });
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
    const raw = extractErrorMessage(error, 'failed to join project');
    inviteState.acceptError = friendlyJoinError(raw);
  } finally {
    inviteState.accepting = false;
  }
}

export async function removePeer(projectId: string, peerId: string) {
  await tauriApi.removePeer(projectId, peerId);
}
