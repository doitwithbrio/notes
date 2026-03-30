export const uiState = $state({
  sidebarOpen: true,
  rightSidebarOpen: false,
  quickOpenVisible: false,
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
