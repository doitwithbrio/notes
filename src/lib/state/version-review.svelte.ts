import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import { getSignificantVersions, versionState } from './versions.svelte.js';

export const versionReviewState = $state({
  previewVersionId: null as string | null,
  previewVersionIndex: -1,
  previewText: null as string | null,
  previewLoading: false,
  previewError: null as string | null,
  status: 'idle' as 'idle' | 'loading' | 'ready' | 'error',
  viewMode: 'snapshot' as 'snapshot' | 'diff',
  savePromptVisible: false,
});

let previewRequestToken = 0;
const PREVIEW_TIMEOUT_MS = 15_000;

function getErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return ((error as Record<string, unknown>)?.message as string | undefined) ?? String(error);
}

function isMissingVersionCommand(error: unknown): boolean {
  const message = getErrorMessage(error);
  return message.includes('Command get_device_actor_id not found')
    || message.includes('Command get_doc_versions not found')
    || message.includes('Command create_version not found')
    || message.includes('Command get_version_text not found')
    || message.includes('Command restore_to_version_cmd not found');
}

function disableVersionApi(reason: unknown) {
  versionState.supported = false;
  versionState.loading = false;
  versionState.error = 'Version history requires a desktop app restart so the Rust backend can pick up the new commands.';
  versionState.versions = [];
  versionReviewState.previewLoading = false;
  versionReviewState.status = 'error';
  versionReviewState.previewError = versionState.error;
  versionReviewState.previewText = null;
  versionReviewState.savePromptVisible = false;
  console.warn(versionState.error, getErrorMessage(reason));
}

function withTimeout<T>(promise: Promise<T>, timeoutMs = PREVIEW_TIMEOUT_MS): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error('Version preview timed out.'));
    }, timeoutMs);

    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

export async function previewVersion(project: string, docId: string, versionId: string) {
  if (!versionState.supported) return;
  const requestToken = ++previewRequestToken;
  versionReviewState.previewVersionId = versionId;
  versionReviewState.status = 'loading';
  versionReviewState.previewLoading = true;
  versionReviewState.previewError = null;

  const significant = getSignificantVersions();
  versionReviewState.previewVersionIndex = significant.findIndex((v) => v.id === versionId);

  try {
    const text = await withTimeout(tauriApi.getVersionText(project, docId, versionId));
    if (requestToken === previewRequestToken && versionReviewState.previewVersionId === versionId) {
      versionReviewState.previewText = text;
      versionReviewState.status = 'ready';
    }
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) {
      if (requestToken === previewRequestToken && versionReviewState.previewVersionId === versionId) {
        versionReviewState.previewText = '';
        versionReviewState.status = 'ready';
      }
      return;
    }
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return;
    }
    if (requestToken === previewRequestToken && versionReviewState.previewVersionId === versionId) {
      versionReviewState.previewError = getErrorMessage(error) || 'Failed to load version';
      versionReviewState.status = 'error';
    }
  } finally {
    if (requestToken === previewRequestToken && versionReviewState.previewVersionId === versionId) {
      versionReviewState.previewLoading = false;
    }
  }
}

export function getAdjacentSignificantVersionId(versionId: string, direction: 'older' | 'newer') {
  const significant = getSignificantVersions();
  const currentIdx = significant.findIndex((version) => version.id === versionId);
  if (currentIdx === -1) return null;

  if (direction === 'older') {
    return significant[currentIdx + 1]?.id ?? null;
  }

  return significant[currentIdx - 1]?.id ?? null;
}

export async function restoreVersionData(project: string, docId: string, versionId: string): Promise<boolean> {
  if (!versionState.supported) return false;
  try {
    await tauriApi.restoreToVersion(project, docId, versionId);
    return true;
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return false;
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return false;
    }
    throw error;
  }
}

export function clearVersionPreview() {
  previewRequestToken += 1;
  versionReviewState.previewVersionId = null;
  versionReviewState.previewVersionIndex = -1;
  versionReviewState.previewText = null;
  versionReviewState.previewLoading = false;
  versionReviewState.previewError = null;
  versionReviewState.status = 'idle';
  versionReviewState.viewMode = 'snapshot';
}

export function setVersionViewMode(mode: 'snapshot' | 'diff') {
  versionReviewState.viewMode = mode;
}

export function showSavePrompt() {
  if (!versionState.supported) return;
  versionReviewState.savePromptVisible = true;
}

export function hideSavePrompt() {
  versionReviewState.savePromptVisible = false;
}
