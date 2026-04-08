export const uiState = $state({
  sidebarOpen: true,
  rightSidebarOpen: false,
  quickOpenVisible: false,
  revokedProjectNotices: [] as Array<{
    projectId: string;
    backendProjectId: string;
    projectName: string;
    reason: string;
  }>,
});

export function toggleSidebar() {
  uiState.sidebarOpen = !uiState.sidebarOpen;
}

export function toggleQuickOpen() {
  uiState.quickOpenVisible = !uiState.quickOpenVisible;
}

export function openQuickOpen() {
  uiState.quickOpenVisible = true;
}

export function closeQuickOpen() {
  uiState.quickOpenVisible = false;
}

export function toggleRightSidebar() {
  uiState.rightSidebarOpen = !uiState.rightSidebarOpen;
}

export function showRevokedProjectNotice(projectId: string, backendProjectId: string, projectName: string, reason: string) {
  if (uiState.revokedProjectNotices.some((notice) => notice.backendProjectId === backendProjectId && notice.reason === reason)) {
    return;
  }
  uiState.revokedProjectNotices = [
    ...uiState.revokedProjectNotices,
    { projectId, backendProjectId, projectName, reason },
  ];
}

export function clearRevokedProjectNotice(projectId?: string) {
  uiState.revokedProjectNotices = projectId
    ? uiState.revokedProjectNotices.filter((notice) => notice.projectId !== projectId)
    : [];
}
