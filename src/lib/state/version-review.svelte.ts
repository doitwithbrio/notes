import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import { getSignificantVersions, versionState } from './versions.svelte.js';

export const versionReviewState = $state({
  previewVersionId: null as string | null,
  previewVersionIndex: -1,
  previewText: null as string | null,
  previewLoading: false,
  previewError: null as string | null,
  savePromptVisible: false,
});

let previewRequestToken = 0;

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
  versionReviewState.previewError = versionState.error;
  versionReviewState.previewText = null;
  versionReviewState.savePromptVisible = false;
  console.warn(versionState.error, getErrorMessage(reason));
}

export async function previewVersion(project: string, docId: string, versionId: string) {
  if (!versionState.supported) return;
  const requestToken = ++previewRequestToken;
  versionReviewState.previewVersionId = versionId;
  versionReviewState.previewLoading = true;
  versionReviewState.previewError = null;
  versionReviewState.previewText = null;

  const significant = getSignificantVersions();
  versionReviewState.previewVersionIndex = significant.findIndex((v) => v.id === versionId);

  try {
    const text = await tauriApi.getVersionText(project, docId, versionId);
    if (requestToken === previewRequestToken && versionReviewState.previewVersionId === versionId) {
      versionReviewState.previewText = text;
    }
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) {
      if (requestToken === previewRequestToken && versionReviewState.previewVersionId === versionId) {
        versionReviewState.previewText = '';
      }
      return;
    }
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return;
    }
    if (requestToken === previewRequestToken && versionReviewState.previewVersionId === versionId) {
      versionReviewState.previewError = getErrorMessage(error) || 'Failed to load version';
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

export async function restoreVersionData(project: string, docId: string, versionId: string) {
  if (!versionState.supported) return;
  try {
    await tauriApi.restoreToVersion(project, docId, versionId);
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return;
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return;
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
}

export function showSavePrompt() {
  if (!versionState.supported) return;
  versionReviewState.savePromptVisible = true;
}

export function hideSavePrompt() {
  versionReviewState.savePromptVisible = false;
}
