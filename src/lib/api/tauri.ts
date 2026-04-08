import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppSettings,
  BackendDocInfo,
  BackendPeerStatusEvent,
  BackendPeerStatusSummary,
  BackendPresenceEvent,
  BackendProjectSummary,
    BackendPendingJoinResume,
    BackendOwnerInviteStatus,
    BackendInviteAcceptEvent,
    BackendProjectEvictedEvent,
    BackendProjectEvictionNotice,
    BackendRemoteChangeEvent,
  BackendSearchResult,
  BackendSyncStatusEvent,
  BackendTodoItem,
  BackendUnseenDocInfo,
  BackendVersion,
  BackendRecoverableDocCorruptionDetails,
  UpdateInfo,
  UpdaterAvailability,
} from '../types/index.js';
import { assertTauriRuntime } from '../runtime/tauri.js';

type BackendCommandErrorPayload = {
  code?: string;
  message?: string;
  details?: Record<string, unknown>;
};

export class TauriCommandError extends Error {
  code: string;
  details?: Record<string, unknown>;

  constructor(payload: BackendCommandErrorPayload) {
    super(payload.message ?? 'Backend command failed');
    this.name = 'TauriCommandError';
    this.code = payload.code ?? 'UNKNOWN_ERROR';
    this.details = payload.details;
  }
}

function normalizeInvokeError(error: unknown): Error {
  if (error instanceof Error) return error;
  if (error && typeof error === 'object') {
    const payload = error as BackendCommandErrorPayload;
    if (typeof payload.message === 'string' || typeof payload.code === 'string') {
      return new TauriCommandError(payload);
    }
  }
  return new Error(String(error));
}

async function guardedInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  assertTauriRuntime(`invoke:${command}`);
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeInvokeError(error);
  }
}

async function guardedListen<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<UnlistenFn> {
  assertTauriRuntime(`listen:${event}`);
  return listen<T>(event, (payload) => handler(payload.payload));
}

export const tauriApi = {
  listProjects: () => guardedInvoke<string[]>('list_projects'),
  listProjectSummaries: () => guardedInvoke<BackendProjectSummary[]>('list_project_summaries'),
  createProject: (name: string) => guardedInvoke<void>('create_project', { name }),
  deleteProject: (name: string) => guardedInvoke<void>('delete_project', { name }),
  purgeProjectLocalData: (project: string, reason: string) =>
    guardedInvoke<void>('purge_project_local_data', { project, reason }),
  listProjectEvictionNotices: () => guardedInvoke<BackendProjectEvictionNotice[]>('list_project_eviction_notices'),
  dismissProjectEvictionNotice: (projectId: string) =>
    guardedInvoke<void>('dismiss_project_eviction_notice', { projectId }),
  openProject: (name: string, connectPeers = false) =>
    guardedInvoke<void>('open_project', { name, connectPeers }),
  listFiles: (project: string) => guardedInvoke<BackendDocInfo[]>('list_files', { project }),
  createNote: (project: string, path: string) =>
    guardedInvoke<string>('create_note', { project, path }),
  openDoc: (project: string, docId: string) => guardedInvoke<void>('open_doc', { project, docId }),
  closeDoc: (project: string, docId: string) => guardedInvoke<void>('close_doc', { project, docId }),
  deleteNote: (project: string, docId: string) =>
    guardedInvoke<void>('delete_note', { project, docId }),
  renameNote: (project: string, docId: string, newPath: string) =>
    guardedInvoke<void>('rename_note', { project, docId, newPath }),
  recoverDocFromMarkdown: (project: string, docId: string) =>
    guardedInvoke<BackendDocInfo>('recover_doc_from_markdown_cmd', { project, docId }),
  getDocBinary: async (project: string, docId: string) => {
    const raw = await guardedInvoke<ArrayBuffer | number[]>('get_doc_binary', { project, docId });
    if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
    return new Uint8Array(raw);
  },
  getDocText: (project: string, docId: string) =>
    guardedInvoke<string>('get_doc_text', { project, docId }),
  importImage: (project: string, data: Uint8Array, filename: string) =>
    guardedInvoke<{ hash: string; size: number; filename: string; mimeType: string }>('import_image', {
      project,
      data: Array.from(data),
      filename,
    }),
  getImage: async (hash: string) => {
    const raw = await guardedInvoke<ArrayBuffer | number[]>('get_image', { hash });
    if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
    return new Uint8Array(raw);
  },
  hasImage: (hash: string) => guardedInvoke<boolean>('has_image', { hash }),
  getDocIncremental: async (project: string, docId: string) => {
    const raw = await guardedInvoke<ArrayBuffer | number[]>('get_doc_incremental', { project, docId });
    if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
    return new Uint8Array(raw);
  },
  getViewerDocSnapshot: async (project: string, docId: string) => {
    const raw = await guardedInvoke<ArrayBuffer | number[]>('get_viewer_doc_snapshot', { project, docId });
    if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
    return new Uint8Array(raw);
  },
  applyChanges: (project: string, docId: string, data: Uint8Array) =>
    guardedInvoke<void>('apply_changes', { project, docId, data: Array.from(data) }),
  saveDoc: (project: string, docId: string) => guardedInvoke<void>('save_doc', { project, docId }),
  compactDoc: (project: string, docId: string) =>
    guardedInvoke<void>('compact_doc', { project, docId }),
  ensureBlobAvailable: (project: string, hash: string) =>
    guardedInvoke<{ available: boolean; fetched: boolean }>('ensure_blob_available', { project, hash }),
  getPeerStatus: (project: string) =>
    guardedInvoke<BackendPeerStatusSummary[]>('get_peer_status', { project }),
  broadcastPresence: (
    project: string,
    activeDoc: string | null,
    cursorPos: number | null,
    selection: [number, number] | null,
  ) => guardedInvoke<void>('broadcast_presence', { project, activeDoc, cursorPos, selection }),
  // Invite & peer management
  generateInvite: (project: string, role: 'editor' | 'viewer') =>
    guardedInvoke<import('../types/index.js').GenerateInviteResult>('generate_invite', { project, role }),
  acceptInvite: (passphrase: string, ownerPeerId: string) =>
    guardedInvoke<import('../types/index.js').AcceptInviteResult>('accept_invite', { passphrase, ownerPeerId }),
  listPendingJoinResumes: () => guardedInvoke<BackendPendingJoinResume[]>('list_pending_join_resumes_cmd'),
  resumePendingJoins: () => guardedInvoke<void>('resume_pending_joins_cmd'),
  listOwnerInvites: (project?: string) => guardedInvoke<BackendOwnerInviteStatus[]>('list_owner_invites_cmd', { project }),
  addPeer: (project: string, peerIdStr: string) =>
    guardedInvoke<void>('add_peer', { project, peerIdStr }),
  removePeer: (project: string, peerIdStr: string) =>
    guardedInvoke<void>('remove_peer', { project, peerIdStr }),
  getPeerId: () => guardedInvoke<string>('get_peer_id'),
  getActorAliases: (project: string) =>
    guardedInvoke<Record<string, string>>('get_actor_aliases', { project }),
  // Version system
  getDeviceActorId: () => guardedInvoke<string>('get_device_actor_id'),
  getDocVersions: (docId: string) =>
    guardedInvoke<BackendVersion[]>('get_doc_versions', { docId }),
  createVersion: (project: string, docId: string, label?: string) =>
    guardedInvoke<BackendVersion>('create_version', { project, docId, label: label ?? null }),
  getVersionText: (project: string, docId: string, versionId: string) =>
    guardedInvoke<string>('get_version_text', { project, docId, versionId }),
  restoreToVersion: (project: string, docId: string, versionId: string) =>
    guardedInvoke<void>('restore_to_version_cmd', { project, docId, versionId }),
  searchNotes: (query: string, limit?: number) =>
    guardedInvoke<BackendSearchResult[]>('search_notes', { query, limit }),
  searchProjectNotes: (query: string, project: string, limit?: number) =>
    guardedInvoke<BackendSearchResult[]>('search_project_notes', { query, project, limit }),
  listProjectTodos: (project: string) =>
    guardedInvoke<BackendTodoItem[]>('list_project_todos', { project }),
  addProjectTodo: (project: string, text: string, linkedDocId?: string) =>
    guardedInvoke<string>('add_project_todo', { project, text, linkedDocId: linkedDocId ?? null }),
  toggleProjectTodo: (project: string, todoId: string) =>
    guardedInvoke<boolean>('toggle_project_todo', { project, todoId }),
  removeProjectTodo: (project: string, todoId: string) =>
    guardedInvoke<void>('remove_project_todo', { project, todoId }),
  updateProjectTodo: (project: string, todoId: string, text: string) =>
    guardedInvoke<void>('update_project_todo', { project, todoId, text }),
  getUnseenDocs: (project: string) =>
    guardedInvoke<BackendUnseenDocInfo[]>('get_unseen_docs', { project }),
  markDocSeen: (project: string, docId: string) =>
    guardedInvoke<void>('mark_doc_seen', { project, docId }),
  getSettings: () => guardedInvoke<AppSettings>('get_settings'),
  updateSettings: (settings: AppSettings) => guardedInvoke<void>('update_settings', { settings }),
  e2eIsEnabled: () => guardedInvoke<boolean>('e2e_is_enabled'),
  e2eSetNetworkBlocked: (blocked: boolean) => guardedInvoke<void>('e2e_set_network_blocked', { blocked }),
  onRemoteChange: (handler: (payload: BackendRemoteChangeEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendRemoteChangeEvent>('p2p:remote-change', handler),
  onSyncStatus: (handler: (payload: BackendSyncStatusEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendSyncStatusEvent>('p2p:sync-status', handler),
  onPeerStatus: (handler: (payload: BackendPeerStatusEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendPeerStatusEvent>('p2p:peer-status', handler),
  onPresenceUpdate: (handler: (payload: BackendPresenceEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendPresenceEvent>('p2p:presence-update', handler),
  onInviteAcceptStatus: (handler: (payload: BackendInviteAcceptEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendInviteAcceptEvent>('p2p:invite-accept', handler),
  onProjectEvicted: (handler: (payload: BackendProjectEvictedEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendProjectEvictedEvent>('p2p:project-evicted', handler),

  // ── Auto-update ──────────────────────────────────────────────────
  getUpdaterAvailability: () => guardedInvoke<UpdaterAvailability>('get_updater_availability'),
  /** Ask the Rust backend to fetch latest.json and compare versions. */
  checkForUpdate: () => guardedInvoke<UpdateInfo | null>('check_for_update'),
};

export function isRecoverableDocCorruption(
  error: unknown,
): error is TauriCommandError & { details: BackendRecoverableDocCorruptionDetails } {
  return error instanceof TauriCommandError && error.code === 'DOC_CORRUPTED_RECOVERABLE';
}
