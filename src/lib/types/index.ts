// ── Accent Color ──

export const ACCENT_COLORS = [
  { id: 'mint', hex: '#2AC994', label: 'Mint' },
  { id: 'blue', hex: '#5B8DEF', label: 'Blue' },
  { id: 'pink', hex: '#FF3B8B', label: 'Pink' },
  { id: 'amber', hex: '#EA8913', label: 'Amber' },
] as const;

export type AccentColorId = (typeof ACCENT_COLORS)[number]['id'];

export const DEFAULT_ACCENT: AccentColorId = 'mint';

// ── Cursor Colors (remote peers) ──

export const CURSOR_COLORS = [
  '#FF0000',
  '#0000FF',
  '#FF00FF',
  '#FF8800',
  '#8800FF',
] as const;

// ── Sync & Connection ──

export type ConnectionStatus = 'connected' | 'slow' | 'offline';

export type SyncStatus = 'synced' | 'syncing' | 'local-only';

// ── Peers ──

export interface Peer {
  id: string;
  alias: string;
  online: boolean;
  cursorColor: string;
}

export interface CursorPosition {
  peerId: string;
  docId: string;
  /** ProseMirror position */
  from: number;
  to: number;
  lastActive: number;
}

// ── Projects & Documents ──

export type PeerRole = 'owner' | 'editor' | 'viewer';

export interface Project {
  id: string;
  name: string;
  path: string;
  shared: boolean;
  role: PeerRole;
}

export interface Document {
  id: string;
  projectId: string;
  path: string;
  title: string;
  syncStatus: SyncStatus;
  wordCount: number;
  /** Peer IDs that currently have this doc open */
  activePeers: string[];
}

// ── UI ──

export type AppView = 'editor' | 'settings';
