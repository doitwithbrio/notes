export const selectors = {
  appShell: '[data-testid="app-shell"]',
  workspaceLoading: '[data-testid="workspace-loading"]',
  editorLoading: '[data-testid="editor-loading"]',
  editorPane: '[data-testid="editor-pane"]',
  editorMount: '[data-testid="editor-mount"] .editor-content',
  blobImageNode: '[data-testid="blob-image-node"]',
  editorDocTitle: '[data-testid="editor-doc-title"]',
  connectionStatus: '[data-testid="connection-status"]',
  sidebar: '[data-testid="sidebar"]',
  createProjectTrigger: '[data-testid="create-project-trigger"]',
  joinProjectTrigger: '[data-testid="join-project-trigger"]',
  quickOpenTrigger: '[data-testid="quick-open-trigger"]',
  rightSidebar: '[data-testid="right-sidebar"]',
  rightSidebarPeersTrigger: '[data-testid="right-sidebar-peers-trigger"]',
  peersSection: '[data-testid="peers-section"]',
  peersInviteTrigger: '[data-testid="peers-invite-trigger"]',
  joinDialog: '[data-testid="join-dialog"]',
  joinPassphraseInput: '[data-testid="join-passphrase-input"]',
  joinPeerIdInput: '[data-testid="join-peer-id-input"]',
  joinSubmit: '[data-testid="join-submit"]',
  joinError: '[data-testid="join-error"]',
  joinOpenProject: '[data-testid="join-open-project"]',
  shareDialog: '[data-testid="share-dialog"]',
  shareRoleEditor: '[data-testid="share-role-editor"]',
  shareRoleViewer: '[data-testid="share-role-viewer"]',
  shareGenerate: '[data-testid="share-generate"]',
  sharePassphrase: '[data-testid="share-passphrase"]',
  sharePeerId: '[data-testid="share-peer-id"]',
  shareExpired: '[data-testid="share-expired"]',
  shareTimer: '[data-testid="share-timer"]',
  shareNewCode: '[data-testid="share-new-code"]',
  projectNameInput: 'input[placeholder="project name"]',
  noteTitleInput: 'input[placeholder="note title"]',
};

export function projectOpenSelector(projectId: string) {
  return `[data-testid="project-open-${projectId}"]`;
}

export function projectAddNoteSelector(projectId: string) {
  return `[data-testid="project-add-note-${projectId}"]`;
}

export function peerRowSelector(peerId: string) {
  return `[data-testid="peer-row-${peerId}"]`;
}

export function peerRemoveTriggerSelector(peerId: string) {
  return `[data-testid="peer-remove-trigger-${peerId}"]`;
}

export function peerRemoveConfirmSelector(peerId: string) {
  return `[data-testid="peer-remove-confirm-${peerId}"]`;
}
