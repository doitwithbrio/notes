import { tauriApi } from '../api/tauri.js';
import type { BackendHistorySession, DiffBlock } from '../types/index.js';

export const historyState = $state({
  sessions: [] as BackendHistorySession[],
  loading: false,
  error: null as string | null,
  activeDocId: null as string | null,

  // History review mode
  selectedSessionId: null as string | null,
  previewText: null as string | null,
  previewLoading: false,
  previewError: null as string | null,
  diffBlocks: [] as DiffBlock[],

  // Actor alias resolution
  actorAliases: {} as Record<string, string>,
});

export async function loadHistory(project: string, docId: string) {
  historyState.loading = true;
  historyState.error = null;
  historyState.activeDocId = docId;
  try {
    const [sessions, aliases] = await Promise.all([
      tauriApi.getDocHistory(project, docId),
      tauriApi.getActorAliases(project).catch(() => ({}) as Record<string, string>),
    ]);
    historyState.sessions = sessions;
    historyState.actorAliases = aliases;
  } catch (error) {
    historyState.error = error instanceof Error ? error.message : 'Failed to load history';
    historyState.sessions = [];
  } finally {
    historyState.loading = false;
  }
}

export async function selectSession(project: string, docId: string, sessionId: string) {
  historyState.selectedSessionId = sessionId;
  historyState.previewLoading = true;
  historyState.previewError = null;
  historyState.previewText = null;
  historyState.diffBlocks = [];

  try {
    const text = await tauriApi.getSessionText(project, docId, sessionId);
    historyState.previewText = text;
  } catch (error) {
    historyState.previewError =
      error instanceof Error ? error.message : 'Failed to load version';
  } finally {
    historyState.previewLoading = false;
  }
}

export async function restoreSession(project: string, docId: string, sessionId: string) {
  await tauriApi.restoreToSession(project, docId, sessionId);
  exitHistoryReview();
  // Reload history after restore
  await loadHistory(project, docId);
}

export function exitHistoryReview() {
  historyState.selectedSessionId = null;
  historyState.previewText = null;
  historyState.previewError = null;
  historyState.diffBlocks = [];
}

export function clearHistory() {
  historyState.sessions = [];
  historyState.activeDocId = null;
  historyState.error = null;
  historyState.actorAliases = {};
  exitHistoryReview();
}
