import { tauriApi } from '../api/tauri.js';
import type { BackendHistorySession } from '../types/index.js';

export const historyState = $state({
  sessions: [] as BackendHistorySession[],
  loading: false,
  error: null as string | null,
  activeDocId: null as string | null,
});

export async function loadHistory(project: string, docId: string) {
  historyState.loading = true;
  historyState.error = null;
  historyState.activeDocId = docId;
  try {
    historyState.sessions = await tauriApi.getDocHistory(project, docId);
  } catch (error) {
    historyState.error = error instanceof Error ? error.message : 'Failed to load history';
    historyState.sessions = [];
  } finally {
    historyState.loading = false;
  }
}

export function clearHistory() {
  historyState.sessions = [];
  historyState.activeDocId = null;
  historyState.error = null;
}
