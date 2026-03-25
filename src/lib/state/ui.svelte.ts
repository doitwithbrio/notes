import { ACCENT_COLORS, DEFAULT_ACCENT, type AccentColorId, type AppView } from '../types/index.js';

export const uiState = $state({
  view: 'editor' as AppView,
  sidebarOpen: true,
  rightSidebarOpen: false,
  activeProjectId: null as string | null,
  quickOpenVisible: false,
  accentColorId: DEFAULT_ACCENT as AccentColorId,
});

export function getAccentHex(): string {
  return ACCENT_COLORS.find((c) => c.id === uiState.accentColorId)?.hex ?? ACCENT_COLORS[0]!.hex;
}

export function setAccentColor(id: AccentColorId) {
  uiState.accentColorId = id;
  document.documentElement.style.setProperty(
    '--accent',
    ACCENT_COLORS.find((c) => c.id === id)?.hex ?? ACCENT_COLORS[0]!.hex,
  );
}

export function toggleSidebar() {
  uiState.sidebarOpen = !uiState.sidebarOpen;
}

export function toggleQuickOpen() {
  uiState.quickOpenVisible = !uiState.quickOpenVisible;
}

export function toggleRightSidebar() {
  uiState.rightSidebarOpen = !uiState.rightSidebarOpen;
}

export function openProjectOverview(projectId: string) {
  uiState.view = 'project-overview';
  uiState.activeProjectId = projectId;
}
