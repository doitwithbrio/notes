export interface AccentTints {
  bgTint: string;
  surface: string;
  surfaceHover: string;
  surfaceActive: string;
  surfaceSidebar: string;
  borderSubtle: string;
  borderDefault: string;
  overlayBackdrop: string;
}

export interface AccentColor {
  readonly id: string;
  readonly hex: string;
  readonly label: string;
  readonly tints: {
    readonly light: AccentTints;
    readonly dark: AccentTints;
  };
}

export const ACCENT_COLORS: readonly AccentColor[] = [
  {
    id: 'amber',
    hex: '#B68D5E',
    label: 'Amber',
    tints: {
      light: {
        bgTint: '#FAF7F2',
        surface: '#FDFBF7',
        surfaceHover: '#F7F3EC',
        surfaceActive: '#F0EBE3',
        surfaceSidebar: '#F5F1EA',
        borderSubtle: '#EBE5DC',
        borderDefault: '#DED7CC',
        overlayBackdrop: 'rgba(250, 247, 242, 0.6)',
      },
      dark: {
        bgTint: '#161413',
        surface: '#1e1b18',
        surfaceHover: '#2a2621',
        surfaceActive: '#35302a',
        surfaceSidebar: '#1a1714',
        borderSubtle: '#332e28',
        borderDefault: '#403a33',
        overlayBackdrop: 'rgba(14, 12, 10, 0.65)',
      },
    },
  },
  {
    id: 'slate',
    hex: '#7B8FA3',
    label: 'Slate',
    tints: {
      light: {
        bgTint: '#F4F6F8',
        surface: '#F9FAFB',
        surfaceHover: '#EFF2F5',
        surfaceActive: '#E5EAF0',
        surfaceSidebar: '#EDF0F4',
        borderSubtle: '#DDE2E9',
        borderDefault: '#CED5DE',
        overlayBackdrop: 'rgba(244, 246, 248, 0.6)',
      },
      dark: {
        bgTint: '#131518',
        surface: '#191c21',
        surfaceHover: '#22272e',
        surfaceActive: '#2b323b',
        surfaceSidebar: '#15181d',
        borderSubtle: '#2c3340',
        borderDefault: '#3a4250',
        overlayBackdrop: 'rgba(10, 11, 14, 0.65)',
      },
    },
  },
  {
    id: 'clay',
    hex: '#B87A6A',
    label: 'Clay',
    tints: {
      light: {
        bgTint: '#FAF5F3',
        surface: '#FDF9F7',
        surfaceHover: '#F7F0ED',
        surfaceActive: '#F0E7E3',
        surfaceSidebar: '#F5EEEB',
        borderSubtle: '#EBE1DD',
        borderDefault: '#DED3CD',
        overlayBackdrop: 'rgba(250, 245, 243, 0.6)',
      },
      dark: {
        bgTint: '#171413',
        surface: '#1e1a18',
        surfaceHover: '#2b2522',
        surfaceActive: '#362e2a',
        surfaceSidebar: '#1b1715',
        borderSubtle: '#342d29',
        borderDefault: '#423a34',
        overlayBackdrop: 'rgba(14, 11, 10, 0.65)',
      },
    },
  },
  {
    id: 'olive',
    hex: '#8A9A6B',
    label: 'Olive',
    tints: {
      light: {
        bgTint: '#F7F8F2',
        surface: '#FBFCF7',
        surfaceHover: '#F1F4EA',
        surfaceActive: '#E6ECDA',
        surfaceSidebar: '#EEF1E7',
        borderSubtle: '#E0E5D2',
        borderDefault: '#D3DAC3',
        overlayBackdrop: 'rgba(247, 248, 242, 0.6)',
      },
      dark: {
        bgTint: '#141614',
        surface: '#1a1d19',
        surfaceHover: '#252922',
        surfaceActive: '#2e342a',
        surfaceSidebar: '#161915',
        borderSubtle: '#2d332a',
        borderDefault: '#3b4236',
        overlayBackdrop: 'rgba(10, 12, 10, 0.65)',
      },
    },
  },
] as const;

export type AccentColorId = (typeof ACCENT_COLORS)[number]['id'];
export const DEFAULT_ACCENT: AccentColorId = 'amber';
export type ThemeMode = 'system' | 'light' | 'dark';

export const CURSOR_COLORS = [
  '#FF5C5C',
  '#3B82F6',
  '#A855F7',
  '#F59E0B',
  '#10B981',
] as const;

export type ConnectionStatus = 'connected' | 'slow' | 'offline' | 'local';
export type SyncStatus = 'synced' | 'syncing' | 'local-only';
export type PeerRole = 'owner' | 'editor' | 'viewer';

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

export type VersionSignificance = 'skip' | 'minor' | 'significant' | 'named';
export type VersionType = 'auto' | 'named';

export interface BackendVersion {
  id: string;
  docId: string;
  project: string;
  type: VersionType;
  name: string;
  label: string | null;
  heads: string[];
  actor: string;
  createdAt: number;
  changeCount: number;
  charsAdded: number;
  charsRemoved: number;
  blocksChanged: number;
  significance: VersionSignificance;
  seq: number;
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

export interface GenerateInviteResult {
  passphrase: string;
  peerId: string;
  expiresAt: string;
}

export interface AcceptInviteResult {
  projectId: string;
  projectName: string;
  role: string;
}

export interface BackendPeerStatusSummary {
  peerId: string;
  connected: boolean;
  alias: string | null;
  role: PeerRole | null;
  activeDoc: string | null;
}

export interface AppSettings {
  schemaVersion: number;
  displayName: string;
  customRelays: string[];
  appearance: AppearanceSettings;
  fontSize: number;
  autoSave: boolean;
  saveIntervalSecs: number;
  largeDocWarningWords: number;
  idleDocTimeoutSecs: number;
}

export interface AppearanceSettings {
  mode: ThemeMode;
  accent: AccentColorId;
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

// ── Auto-update types ────────────────────────────────────────────────

/** Lifecycle states for the update flow. */
export type UpdateStatus =
  | 'idle'         // No update activity
  | 'checking'     // Fetching latest.json from GitHub
  | 'available'    // Newer version found, waiting for user action
  | 'downloading'  // .app.tar.gz being downloaded
  | 'installing'   // Extracting and replacing the .app bundle
  | 'ready'        // Install done, about to relaunch
  | 'error';       // Something went wrong

/** Mirrors the Rust UpdateInfo struct. */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  body: string | null;
  date: string | null;
}
