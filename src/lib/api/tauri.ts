import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AppSettings,
  BackendDocInfo,
  BackendPeerStatusEvent,
  BackendPeerStatusSummary,
  BackendPresenceEvent,
  BackendProjectSummary,
  BackendRemoteChangeEvent,
  BackendSearchResult,
  BackendSyncStatusEvent,
  BackendUnseenDocInfo,
  BackendVersion,
  BackendDocBlame,
} from '../types/index.js';
import { assertTauriRuntime } from '../runtime/tauri.js';

async function guardedInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  assertTauriRuntime(`invoke:${command}`);
  return invoke<T>(command, args);
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
  openProject: (name: string) => guardedInvoke<void>('open_project', { name }),
  listFiles: (project: string) => guardedInvoke<BackendDocInfo[]>('list_files', { project }),
  createNote: (project: string, path: string) =>
    guardedInvoke<string>('create_note', { project, path }),
  openDoc: (project: string, docId: string) => guardedInvoke<void>('open_doc', { project, docId }),
  closeDoc: (project: string, docId: string) => guardedInvoke<void>('close_doc', { project, docId }),
  deleteNote: (project: string, docId: string) =>
    guardedInvoke<void>('delete_note', { project, docId }),
  renameNote: (project: string, docId: string, newPath: string) =>
    guardedInvoke<void>('rename_note', { project, docId, newPath }),
  getDocBinary: async (project: string, docId: string) => {
    const raw = await guardedInvoke<ArrayBuffer | number[]>('get_doc_binary', { project, docId });
    if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
    return new Uint8Array(raw);
  },
  getDocText: (project: string, docId: string) =>
    guardedInvoke<string>('get_doc_text', { project, docId }),
  applyChanges: (project: string, docId: string, data: Uint8Array) =>
    guardedInvoke<void>('apply_changes', { project, docId, data: Array.from(data) }),
  saveDoc: (project: string, docId: string) => guardedInvoke<void>('save_doc', { project, docId }),
  compactDoc: (project: string, docId: string) =>
    guardedInvoke<void>('compact_doc', { project, docId }),
  getPeerStatus: (project: string) =>
    guardedInvoke<BackendPeerStatusSummary[]>('get_peer_status', { project }),
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
  getDocBlame: (project: string, docId: string) =>
    guardedInvoke<BackendDocBlame>('get_doc_blame', { project, docId }),
  searchNotes: (query: string, limit?: number) =>
    guardedInvoke<BackendSearchResult[]>('search_notes', { query, limit }),
  searchProjectNotes: (query: string, project: string, limit?: number) =>
    guardedInvoke<BackendSearchResult[]>('search_project_notes', { query, project, limit }),
  getUnseenDocs: (project: string) =>
    guardedInvoke<BackendUnseenDocInfo[]>('get_unseen_docs', { project }),
  markDocSeen: (project: string, docId: string) =>
    guardedInvoke<void>('mark_doc_seen', { project, docId }),
  getSettings: () => guardedInvoke<AppSettings>('get_settings'),
  updateSettings: (settings: AppSettings) => guardedInvoke<void>('update_settings', { settings }),
  onRemoteChange: (handler: (payload: BackendRemoteChangeEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendRemoteChangeEvent>('p2p:remote-change', handler),
  onSyncStatus: (handler: (payload: BackendSyncStatusEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendSyncStatusEvent>('p2p:sync-status', handler),
  onPeerStatus: (handler: (payload: BackendPeerStatusEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendPeerStatusEvent>('p2p:peer-status', handler),
  onPresenceUpdate: (handler: (payload: BackendPresenceEvent) => void): Promise<UnlistenFn> =>
    guardedListen<BackendPresenceEvent>('p2p:presence-update', handler),
};
