import type { AppView } from '../types/index.js';

export const uiState = $state({
  view: 'editor' as AppView,
  sidebarOpen: true,
  rightSidebarOpen: false,
  activeProjectId: null as string | null,
  quickOpenVisible: false,
  historyReviewSessionId: null as string | null,
});

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

export function openSettings() {
  uiState.view = 'settings';
}
