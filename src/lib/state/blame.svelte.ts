import type { BackendDocBlame } from '../types/index.js';
import { tauriApi } from '../api/tauri.js';

export const blameState = $state({
  data: null as BackendDocBlame | null,
  loading: false,
  error: null as string | null,
  /** Track which doc the current blame data belongs to */
  docId: null as string | null,
});

export async function loadBlame(project: string, docId: string) {
  blameState.loading = true;
  blameState.error = null;

  try {
    const data = await tauriApi.getDocBlame(project, docId);
    blameState.data = data;
    blameState.docId = docId;
  } catch (err) {
    blameState.error = err instanceof Error ? err.message : 'Failed to load blame';
    blameState.data = null;
    blameState.docId = null;
  } finally {
    blameState.loading = false;
  }
}

export function clearBlame() {
  blameState.data = null;
  blameState.loading = false;
  blameState.error = null;
  blameState.docId = null;
}
