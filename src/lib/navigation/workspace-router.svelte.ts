import { tauriApi } from '../api/tauri.js';
import { getDocById } from '../state/documents.svelte.js';
import { openEditorSession, closeEditorSession, reloadActiveSession } from '../session/editor-session.svelte.js';
import { loadVersions } from '../state/versions.svelte.js';
import { clearVersionPreview, getAdjacentSignificantVersionId, previewVersion, restoreVersionData } from '../state/version-review.svelte.js';
import { loadProjectDocs, hasHydratedProject } from '../state/documents.svelte.js';
import { getProject } from '../state/projects.svelte.js';

export type WorkspaceRoute =
  | { kind: 'settings' }
  | { kind: 'project'; projectId: string }
  | { kind: 'doc'; projectId: string; docId: string; mode: 'live' }
  | { kind: 'doc'; projectId: string; docId: string; mode: 'history'; versionId: string };

const routerState = $state({
  currentRoute: null as WorkspaceRoute | null,
  settingsReturnRoute: null as WorkspaceRoute | null,
  pendingDocMove: null as null | {
    fromProjectId: string;
    fromDocId: string;
    toProjectId: string;
    toDocId: string;
  },
});

function setCurrentRoute(route: WorkspaceRoute | null) {
  routerState.currentRoute = route;
}

function fallbackRouteForProject(projectId: string): WorkspaceRoute | null {
  return getProject(projectId) ? { kind: 'project', projectId } : null;
}

function rewriteDocRoute(
  route: WorkspaceRoute | null,
  matcher: (route: Extract<WorkspaceRoute, { kind: 'doc' }>) => WorkspaceRoute | null,
) {
  if (!route || route.kind !== 'doc') return route;
  return matcher(route);
}

export function getWorkspaceRoute(): WorkspaceRoute | null {
  return routerState.currentRoute;
}

export function getWorkspaceContextRoute(): WorkspaceRoute | null {
  const route = getWorkspaceRoute();
  if (route?.kind === 'settings') {
    return routerState.settingsReturnRoute;
  }
  return route;
}

export function getWorkspaceProjectId(): string | null {
  const route = getWorkspaceContextRoute();
  if (!route) return null;
  if (route.kind === 'project' || route.kind === 'doc') {
    return route.projectId;
  }
  return null;
}

export function getSelectedProjectId(): string | null {
  return getWorkspaceProjectId();
}

export function getWorkspaceDocId(): string | null {
  const route = getWorkspaceContextRoute();
  return route?.kind === 'doc' ? route.docId : null;
}

export function getSelectedDocId(): string | null {
  return getWorkspaceDocId();
}

export function getSelectedDoc() {
  return getDocById(getSelectedDocId());
}

export function getHistoryVersionId(): string | null {
  const route = getWorkspaceRoute();
  return isHistoryRoute(route) ? route.versionId : null;
}

export function getSelectedHistoryVersionId(): string | null {
  const route = getWorkspaceContextRoute();
  return isHistoryRoute(route) ? route.versionId : null;
}

export function isLiveDocRoute(route: WorkspaceRoute | null): route is Extract<WorkspaceRoute, { kind: 'doc'; mode: 'live' }> {
  return route?.kind === 'doc' && route.mode === 'live';
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

function claimLiveDocRoute(projectId: string, docId: string) {
  setCurrentRoute({ kind: 'doc', projectId, docId, mode: 'live' });
}

export async function navigateToProject(projectId: string) {
  navigateBackToLive();
  await closeEditorSession();
  setCurrentRoute({ kind: 'project', projectId });

  try {
    if (!hasHydratedProject(projectId)) {
      await loadProjectDocs(projectId, { connectPeers: true });
    } else {
      await tauriApi.openProject(projectId, true);
    }
  } catch (error) {
    const eviction = await import('../state/project-eviction.svelte.js');
    if ((error as { code?: string } | null)?.code === 'PROJECT_IDENTITY_MISMATCH'
      || (error as { code?: string } | null)?.code === 'PROJECT_NOT_FOUND') {
      await tauriApi.purgeProjectLocalData(projectId, 'access-revoked').catch(() => undefined);
      await eviction.evictProject(projectId, 'access-revoked');
      return;
    }
    throw error;
  }
}

export async function navigateToDoc(projectId: string, docId: string) {
  navigateBackToLive();
  claimLiveDocRoute(projectId, docId);
  await openEditorSession(projectId, docId);
}

export async function navigateToHistory(projectId: string, docId: string, versionId: string) {
  if (getSelectedDocId() !== docId) {
    await navigateToDoc(projectId, docId);
    const route = getWorkspaceRoute();
    if (!isLiveDocRoute(route) || route.projectId !== projectId || route.docId !== docId) {
      return;
    }
  }

  setCurrentRoute({ kind: 'doc', projectId, docId, mode: 'history', versionId });
  await previewVersion(projectId, docId, versionId);
}

export async function navigateHistoryOlder(projectId: string, docId: string) {
  const versionId = getHistoryVersionId();
  if (!versionId) return;
  const olderVersionId = getAdjacentSignificantVersionId(versionId, 'older');
  if (!olderVersionId) return;
  await navigateToHistory(projectId, docId, olderVersionId);
}

export async function navigateHistoryNewer(projectId: string, docId: string) {
  const versionId = getHistoryVersionId();
  if (!versionId) return;
  const newerVersionId = getAdjacentSignificantVersionId(versionId, 'newer');
  if (!newerVersionId) {
    navigateBackToLive();
    return;
  }
  await navigateToHistory(projectId, docId, newerVersionId);
}

export function navigateBackToLive() {
  const route = getWorkspaceRoute();
  if (isHistoryRoute(route)) {
    setCurrentRoute({ kind: 'doc', projectId: route.projectId, docId: route.docId, mode: 'live' });
  }
  clearVersionPreview();
}

export async function restoreHistoryVersion(projectId: string, docId: string, versionId: string): Promise<boolean> {
  const restored = await restoreVersionData(projectId, docId, versionId);
  if (!restored) {
    return false;
  }

  if (getSelectedProjectId() !== projectId || getSelectedDocId() !== docId) {
    return true;
  }

  try {
    await reloadActiveSession();
    await loadVersions(docId);
  } finally {
    const route = getWorkspaceRoute();
    if (isHistoryRoute(route) && route.projectId === projectId && route.docId === docId && route.versionId === versionId) {
      navigateBackToLive();
    }
  }

  return true;
}

export function navigateToSettings() {
  const current = getWorkspaceRoute();
  if (current?.kind !== 'settings') {
    routerState.settingsReturnRoute = current;
  }
  setCurrentRoute({ kind: 'settings' });
}

export async function navigateBackFromSettings() {
  const target = routerState.settingsReturnRoute;
  routerState.settingsReturnRoute = null;

  if (!target) {
    setCurrentRoute(null);
    return;
  }

  if (target.kind === 'project') {
    setCurrentRoute(target);
    return;
  }

  if (target.kind === 'doc' && target.mode === 'history') {
    await navigateToHistory(target.projectId, target.docId, target.versionId);
    return;
  }

  if (target.kind === 'doc') {
    await navigateToDoc(target.projectId, target.docId);
    return;
  }

  setCurrentRoute(null);
}

export function handleDeletedProject(projectId: string) {
  const currentRoute = getWorkspaceRoute();
  const contextRoute = getWorkspaceContextRoute();

  if (contextRoute && 'projectId' in contextRoute && contextRoute.projectId === projectId) {
    routerState.settingsReturnRoute = null;
  }

  if (currentRoute && 'projectId' in currentRoute && currentRoute.projectId === projectId) {
    setCurrentRoute(null);
  }
}

export function handleDeletedDoc(projectId: string, docId: string) {
  const currentRoute = rewriteDocRoute(getWorkspaceRoute(), (route) => {
    if (route.projectId !== projectId || route.docId !== docId) return route;
    clearVersionPreview();
    return fallbackRouteForProject(projectId);
  });

  const contextRoute = rewriteDocRoute(routerState.settingsReturnRoute, (route) => {
    if (route.projectId !== projectId || route.docId !== docId) return route;
    return fallbackRouteForProject(projectId);
  });

  routerState.settingsReturnRoute = contextRoute;
  if (currentRoute !== getWorkspaceRoute()) {
    setCurrentRoute(currentRoute);
  }
}

export function beginMovedDoc(args: {
  fromProjectId: string;
  fromDocId: string;
  toProjectId: string;
  toDocId: string;
}) {
  routerState.pendingDocMove = args;
}

export function clearMovedDoc(args?: { fromProjectId: string; fromDocId: string }) {
  if (!args) {
    routerState.pendingDocMove = null;
    return;
  }

  if (
    routerState.pendingDocMove?.fromProjectId === args.fromProjectId
    && routerState.pendingDocMove?.fromDocId === args.fromDocId
  ) {
    routerState.pendingDocMove = null;
  }
}

export function handleMovedDoc(args: {
  fromProjectId: string;
  fromDocId: string;
  toProjectId: string;
  toDocId: string;
}) {
  const { fromProjectId, fromDocId, toProjectId, toDocId } = args;
  clearMovedDoc({ fromProjectId, fromDocId });

  const rewriteToDestination = (route: Extract<WorkspaceRoute, { kind: 'doc' }>): WorkspaceRoute | null => {
    if (route.projectId !== fromProjectId || route.docId !== fromDocId) return route;
    clearVersionPreview();
    return { kind: 'doc', projectId: toProjectId, docId: toDocId, mode: 'live' };
  };

  const currentRoute = rewriteDocRoute(getWorkspaceRoute(), rewriteToDestination);
  const contextRoute = rewriteDocRoute(routerState.settingsReturnRoute, rewriteToDestination);

  routerState.settingsReturnRoute = contextRoute;
  if (currentRoute !== getWorkspaceRoute()) {
    setCurrentRoute(currentRoute);
  }
}

export function reconcileMissingSelectedDoc() {
  const route = getWorkspaceRoute();
  if (!route || route.kind !== 'doc') return;
  if (getSelectedDoc()) return;
  if (
    routerState.pendingDocMove?.fromProjectId === route.projectId
    && routerState.pendingDocMove?.fromDocId === route.docId
  ) {
    return;
  }
  clearVersionPreview();
  void closeEditorSession();
  setCurrentRoute(fallbackRouteForProject(route.projectId));
}
