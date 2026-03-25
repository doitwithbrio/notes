/**
 * Version history state management.
 *
 * Replaces the old history.svelte.ts with a cleaner model:
 * - Versions are created at meaningful moments (not every keystroke)
 * - Each version has a sea creature name
 * - Named versions (Cmd+S) are prominent, auto-versions are subtle
 */

import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import type { BackendVersion, DiffBlock } from '../types/index.js';

export const versionState = $state({
  versions: [] as BackendVersion[],
  loading: false,
  error: null as string | null,
  activeDocId: null as string | null,

  // Version review mode
  selectedVersionId: null as string | null,
  selectedVersionIndex: -1,
  previewText: null as string | null,
  previewLoading: false,
  previewError: null as string | null,
  diffBlocks: [] as DiffBlock[],

  // Cmd+S save prompt
  savePromptVisible: false,

  // Device actor ID
  deviceActorId: '' as string,
});

/** Get only significant versions (skip trivial ones). */
export function getSignificantVersions(): BackendVersion[] {
  return versionState.versions.filter(
    (v) => v.significance !== 'skip',
  );
}

/** Get only named versions. */
export function getNamedVersions(): BackendVersion[] {
  return versionState.versions.filter((v) => v.type === 'named');
}

/** Load the device actor ID on startup. */
export async function loadDeviceActorId() {
  try {
    versionState.deviceActorId = await tauriApi.getDeviceActorId();
  } catch (error) {
    if (!(error instanceof TauriRuntimeUnavailableError)) {
      console.warn('Failed to load device actor ID:', error);
    }
  }
}

/** Load all versions for a document. */
export async function loadVersions(docId: string) {
  versionState.loading = true;
  versionState.error = null;
  versionState.activeDocId = docId;
  try {
    versionState.versions = await tauriApi.getDocVersions(docId);
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return;
    versionState.error =
      error instanceof Error
        ? error.message
        : (error as Record<string, unknown>)?.message as string ?? 'Failed to load versions';
    versionState.versions = [];
  } finally {
    versionState.loading = false;
  }
}

/** Create a new version (auto or named). */
export async function createVersion(
  project: string,
  docId: string,
  label?: string,
): Promise<BackendVersion | null> {
  try {
    const version = await tauriApi.createVersion(project, docId, label);
    // Reload versions to get the updated list
    await loadVersions(docId);
    return version;
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return null;
    // "no significant changes" is expected for auto-versions — don't log as error
    const msg = error instanceof Error ? error.message : String(error);
    if (msg.includes('no significant changes')) return null;
    console.warn('Failed to create version:', error);
    return null;
  }
}

/** Select a version for preview. */
export async function selectVersion(
  project: string,
  docId: string,
  versionId: string,
) {
  versionState.selectedVersionId = versionId;
  versionState.previewLoading = true;
  versionState.previewError = null;
  versionState.previewText = null;
  versionState.diffBlocks = [];

  // Find index
  const significant = getSignificantVersions();
  versionState.selectedVersionIndex = significant.findIndex(
    (v) => v.id === versionId,
  );

  try {
    const text = await tauriApi.getVersionText(project, docId, versionId);
    versionState.previewText = text;
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) {
      versionState.previewText = '';
      return;
    }
    versionState.previewError =
      error instanceof Error
        ? error.message
        : (error as Record<string, unknown>)?.message as string ?? 'Failed to load version';
  } finally {
    versionState.previewLoading = false;
  }
}

/** Navigate to previous version. */
export async function selectPrevVersion(project: string, docId: string) {
  const significant = getSignificantVersions();
  const currentIdx = versionState.selectedVersionIndex;
  const nextIdx = currentIdx + 1; // versions are most-recent-first, so +1 = older
  if (nextIdx < significant.length) {
    await selectVersion(project, docId, significant[nextIdx]!.id);
  }
}

/** Navigate to next version. */
export async function selectNextVersion(project: string, docId: string) {
  const significant = getSignificantVersions();
  const currentIdx = versionState.selectedVersionIndex;
  const nextIdx = currentIdx - 1; // -1 = newer
  if (nextIdx >= 0) {
    await selectVersion(project, docId, significant[nextIdx]!.id);
  } else {
    // Go back to live
    exitVersionReview();
  }
}

/** Restore document to a specific version. */
export async function restoreVersion(
  project: string,
  docId: string,
  versionId: string,
) {
  await tauriApi.restoreToVersion(project, docId, versionId);
  exitVersionReview();
  await loadVersions(docId);
}

/** Exit version review mode. */
export function exitVersionReview() {
  versionState.selectedVersionId = null;
  versionState.selectedVersionIndex = -1;
  versionState.previewText = null;
  versionState.previewError = null;
  versionState.diffBlocks = [];
}

/** Show the Cmd+S save version prompt. */
export function showSavePrompt() {
  versionState.savePromptVisible = true;
}

/** Hide the Cmd+S save version prompt. */
export function hideSavePrompt() {
  versionState.savePromptVisible = false;
}

/** Clear all version state (e.g., when switching projects). */
export function clearVersions() {
  versionState.versions = [];
  versionState.activeDocId = null;
  versionState.error = null;
  versionState.savePromptVisible = false;
  exitVersionReview();
}
