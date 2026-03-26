import { tauriApi } from '../api/tauri.js';
import { clearActiveDocSelection, getActiveDoc } from '../state/documents.svelte.js';
import { openEditorSession, closeEditorSession } from '../session/editor-session.svelte.js';
import { uiState } from '../state/ui.svelte.js';
import { selectVersion, versionState, leaveHistoryReview } from '../state/versions.svelte.js';
import { loadProjectDocs, hasHydratedProject } from '../state/documents.svelte.js';

export type WorkspaceRoute =
  | { kind: 'settings' }
  | { kind: 'project'; projectId: string }
  | { kind: 'doc'; projectId: string; docId: string; mode: 'live' }
  | { kind: 'doc'; projectId: string; docId: string; mode: 'history'; versionId: string };

export function getWorkspaceRoute(): WorkspaceRoute | null {
  if (uiState.view === 'settings') {
    return { kind: 'settings' };
  }

  const activeDoc = getActiveDoc();
  if (activeDoc) {
    if (uiState.view === 'history-review' && versionState.selectedVersionId) {
      return {
        kind: 'doc',
        projectId: activeDoc.projectId,
        docId: activeDoc.id,
        mode: 'history',
        versionId: versionState.selectedVersionId,
      };
    }

    return {
      kind: 'doc',
      projectId: activeDoc.projectId,
      docId: activeDoc.id,
      mode: 'live',
    };
  }

  if (uiState.activeProjectId) {
    return { kind: 'project', projectId: uiState.activeProjectId };
  }

  return null;
}

export function isProjectRoute(route: WorkspaceRoute | null): route is Extract<WorkspaceRoute, { kind: 'project' }> {
  return route?.kind === 'project';
}

export function isDocRoute(route: WorkspaceRoute | null): route is Extract<WorkspaceRoute, { kind: 'doc' }> {
  return route?.kind === 'doc';
}

export function isHistoryRoute(route: WorkspaceRoute | null): route is Extract<WorkspaceRoute, { kind: 'doc'; mode: 'history' }> {
  return route?.kind === 'doc' && route.mode === 'history';
}

export async function navigateToProject(projectId: string) {
  await closeEditorSession();
  clearActiveDocSelection();
  uiState.view = 'project-overview';
  uiState.activeProjectId = projectId;

  if (!hasHydratedProject(projectId)) {
    await loadProjectDocs(projectId, { connectPeers: true });
  } else {
    await tauriApi.openProject(projectId, true);
  }
}

export async function navigateToDoc(projectId: string, docId: string) {
  await openEditorSession(projectId, docId);
}

export async function navigateToHistory(projectId: string, docId: string, versionId: string) {
  if (getActiveDoc()?.id !== docId) {
    await navigateToDoc(projectId, docId);
  }

  uiState.view = 'history-review';
  uiState.historyReviewSessionId = versionId;
  await selectVersion(projectId, docId, versionId);
}

export function navigateBackToLive() {
  leaveHistoryReview();
}

export function navigateToSettings() {
  uiState.view = 'settings';
}

export function navigateBackFromSettings() {
  const route = getWorkspaceRoute();
  if (route?.kind === 'settings') {
    if (getActiveDoc()) {
      uiState.view = 'editor';
      return;
    }

    if (uiState.activeProjectId) {
      uiState.view = 'project-overview';
      return;
    }
  }

  uiState.view = 'editor';
}
