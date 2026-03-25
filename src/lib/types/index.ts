export const ACCENT_COLORS = [
  { id: 'amber', hex: '#B68D5E', label: 'Amber' },
  { id: 'slate', hex: '#7B8FA3', label: 'Slate' },
  { id: 'clay', hex: '#B87A6A', label: 'Clay' },
  { id: 'olive', hex: '#8A9A6B', label: 'Olive' },
] as const;

export type AccentColorId = (typeof ACCENT_COLORS)[number]['id'];
export const DEFAULT_ACCENT: AccentColorId = 'amber';

export const CURSOR_COLORS = [
  '#FF5C5C',
  '#3B82F6',
  '#A855F7',
  '#F59E0B',
  '#10B981',
] as const;

export type ConnectionStatus = 'connected' | 'slow' | 'offline';
export type SyncStatus = 'synced' | 'syncing' | 'local-only';
export type PeerRole = 'owner' | 'editor' | 'viewer';
export type AppView = 'editor' | 'settings' | 'project-overview' | 'history-review';

export interface BackendProjectSummary {
  name: string;
  path: string;
  shared: boolean;
  role: PeerRole;
  peerCount: number;
}

export interface BackendDocInfo {
  id: string;
  path: string;
  fileType: 'note' | 'asset';
  created: string;
}

export interface BackendUnseenDocInfo {
  docId: string;
  path: string;
  hasUnseenChanges: boolean;
  lastSeenAt: string | null;
}

export interface BackendHistorySession {
  id: string;
  actor: string;
  startedAt: number;
  endedAt: number;
  changeCount: number;
  opCount: number;
  firstChangeHash: string;
  lastChangeHash: string;
}

export interface DiffBlock {
  type: 'added' | 'removed' | 'changed' | 'unchanged';
  content: string;
  lineStart: number;
  lineEnd: number;
}

export interface BackendSearchResult {
  docId: string;
  project: string;
  path: string;
  title: string;
  snippet: string;
}

export interface BackendRemoteChangeEvent {
  docId: string;
  peerId: string | null;
}

export interface BackendSyncStatusEvent {
  docId: string;
  state: SyncStatus;
  unsentChanges: number;
}

export interface BackendPresenceEvent {
  peerId: string;
  alias: string;
  activeDoc: string | null;
  cursorPos: number | null;
  selection: [number, number] | null;
}

export interface BackendPeerStatusEvent {
  peerId: string;
  state: 'connected' | 'disconnected';
  alias: string | null;
}

export interface BackendPeerStatusSummary {
  peerId: string;
  connected: boolean;
  alias: string | null;
  role: PeerRole | null;
  activeDoc: string | null;
}

export interface AppSettings {
  displayName: string;
  customRelays: string[];
  theme: string;
  fontSize: number;
  autoSave: boolean;
  saveIntervalSecs: number;
  largeDocWarningWords: number;
  idleDocTimeoutSecs: number;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  shared: boolean;
  role: PeerRole;
  peerCount: number;
}

export interface Document {
  id: string;
  projectId: string;
  path: string;
  title: string;
  syncStatus: SyncStatus;
  wordCount: number;
  activePeers: string[];
  hasUnread: boolean;
  createdAt?: string | null;
  lastSeenAt?: string | null;
}

export interface Peer {
  id: string;
  alias: string;
  online: boolean;
  cursorColor: string;
  role?: PeerRole | null;
  activeDoc?: string | null;
}

export interface CursorPosition {
  peerId: string;
  docId: string;
  from: number;
  to: number;
  lastActive: number;
}

export interface TodoItem {
  id: string;
  projectId: string;
  text: string;
  done: boolean;
  linkedDocId?: string;
  createdAt: number;
}
