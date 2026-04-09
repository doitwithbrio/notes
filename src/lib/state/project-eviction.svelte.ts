import { closeEditorSession, editorSessionState } from '../session/editor-session.svelte.js';
import { clearProjectDocs } from './documents.svelte.js';
import { removeProject } from './projects.svelte.js';
import { clearProjectTodos } from './todos.svelte.js';
import { clearProjectPeers, getOnlinePeers, hasAnySharedPeers } from './presence.svelte.js';
import { clearVersions } from './versions.svelte.js';
import { clearVersionPreview } from './version-review.svelte.js';
import { clearRevokedProjectNotice, closeQuickOpen, showRevokedProjectNotice } from './ui.svelte.js';
import { handleDeletedProject } from '../navigation/workspace-router.svelte.js';
import { setPeerCount, setSharedProject } from './sync.svelte.js';
import { removeProjectOrder } from './ordering.svelte.js';

export async function evictProject(
  projectId: string,
  reason: string,
  projectName = projectId,
  backendProjectId = projectId,
) {
  if (editorSessionState.projectId === projectId) {
    await closeEditorSession().catch(() => undefined);
    clearVersions();
    clearVersionPreview();
  }

  clearProjectPeers(projectId);
  clearProjectTodos(projectId);
  clearProjectDocs(projectId);
  removeProject(projectId);
  removeProjectOrder(projectId);
  handleDeletedProject(projectId);
  closeQuickOpen();
  setSharedProject(hasAnySharedPeers());
  setPeerCount(getOnlinePeers().length);
  showRevokedProjectNotice(projectId, backendProjectId, projectName, reason);
}

export function dismissRevokedProjectNotice(projectId?: string) {
  clearRevokedProjectNotice(projectId);
}
