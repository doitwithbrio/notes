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
  supported: true,
  availabilityReason: null as null | 'restart-required' | 'temporarily-unavailable',

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

const refreshTimers = new Map<string, ReturnType<typeof setTimeout>>();
let warnedUnsupportedVersionApi = false;

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

function isTemporarilyUnavailable(error: unknown): boolean {
  return getErrorMessage(error).includes('version history is temporarily unavailable');
}

function disableVersionFeatures(
  message: string,
  reason?: unknown,
  availabilityReason: 'restart-required' | 'temporarily-unavailable' = 'temporarily-unavailable',
) {
  versionState.supported = false;
  versionState.availabilityReason = availabilityReason;
  versionState.loading = false;
  versionState.error = message;
  versionState.versions = [];
  versionState.savePromptVisible = false;
  versionState.previewLoading = false;
  versionState.previewError = message;
  versionState.previewText = null;
  if (reason) {
    console.warn(message, getErrorMessage(reason));
  }
}

function disableVersionApi(reason: unknown) {
  disableVersionFeatures(
    'Version history requires a desktop app restart so the Rust backend can pick up the new commands.',
    reason,
    'restart-required',
  );
  if (!warnedUnsupportedVersionApi) {
    warnedUnsupportedVersionApi = true;
    console.warn(
      'Version history API unavailable in current backend build. Fully restart the desktop app after rebuilding the Tauri backend:',
      getErrorMessage(reason),
    );
  }
}

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
  if (!versionState.supported) return;
  try {
    versionState.deviceActorId = await tauriApi.getDeviceActorId();
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return;
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return;
    }
    if (isTemporarilyUnavailable(error)) {
      disableVersionFeatures(
        'Version history is temporarily unavailable while notes preserves an older history database.',
        error,
        'temporarily-unavailable',
      );
      return;
    }
    console.warn('Failed to load device actor ID:', error);
  }
}

/** Load all versions for a document. */
export async function loadVersions(docId: string) {
  if (!versionState.supported) return;
  versionState.loading = true;
  versionState.error = null;
  versionState.activeDocId = docId;
  try {
    const versions = await tauriApi.getDocVersions(docId);
    if (versionState.activeDocId === docId) {
      versionState.versions = versions;
    }
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return;
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return;
    }
    if (isTemporarilyUnavailable(error)) {
      disableVersionFeatures(
        'Version history is temporarily unavailable while notes preserves an older history database.',
        error,
        'temporarily-unavailable',
      );
      return;
    }
    if (versionState.activeDocId === docId) {
      versionState.error =
        getErrorMessage(error) || 'Failed to load versions';
      versionState.versions = [];
    }
  } finally {
    if (versionState.activeDocId === docId) {
      versionState.loading = false;
    }
  }
}

export function scheduleVersionRefresh(docId: string, delayMs = 300) {
  if (!versionState.supported) return;
  const existing = refreshTimers.get(docId);
  if (existing) {
    clearTimeout(existing);
  }

  refreshTimers.set(
    docId,
    setTimeout(() => {
      refreshTimers.delete(docId);
      if (versionState.activeDocId && versionState.activeDocId !== docId) {
        return;
      }
      void loadVersions(docId);
    }, delayMs),
  );
}

/** Create a new version (auto or named). */
export async function createVersion(
  project: string,
  docId: string,
  label?: string,
): Promise<BackendVersion | null> {
  if (!versionState.supported) return null;
  try {
    const version = await tauriApi.createVersion(project, docId, label);
    // Reload versions to get the updated list
    scheduleVersionRefresh(docId, 0);
    return version;
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return null;
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return null;
    }
    if (isTemporarilyUnavailable(error)) {
      disableVersionFeatures(
        'Version history is temporarily unavailable while notes preserves an older history database.',
        error,
        'temporarily-unavailable',
      );
      return null;
    }
    // "no significant changes" is expected for auto-versions — don't log as error
    const msg = getErrorMessage(error);
    if (msg.includes('no significant changes')) return null;
    versionState.error = msg || 'Failed to create version';
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
  if (!versionState.supported) return;
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
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return;
    }
    if (isTemporarilyUnavailable(error)) {
      disableVersionFeatures(
        'Version history is temporarily unavailable while notes preserves an older history database.',
        error,
        'temporarily-unavailable',
      );
      return;
    }
    versionState.previewError =
      getErrorMessage(error) || 'Failed to load version';
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
  if (!versionState.supported) return;
  try {
    await tauriApi.restoreToVersion(project, docId, versionId);
    exitVersionReview();
    await loadVersions(docId);
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return;
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return;
    }
    if (isTemporarilyUnavailable(error)) {
      disableVersionFeatures(
        'Version history is temporarily unavailable while notes preserves an older history database.',
        error,
        'temporarily-unavailable',
      );
      return;
    }
    throw error;
  }
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
  if (!versionState.supported) return;
  versionState.savePromptVisible = true;
}

/** Hide the Cmd+S save version prompt. */
export function hideSavePrompt() {
  versionState.savePromptVisible = false;
}

/** Clear all version state (e.g., when switching projects). */
export function clearVersions() {
  for (const timer of refreshTimers.values()) {
    clearTimeout(timer);
  }
  refreshTimers.clear();
  versionState.versions = [];
  versionState.activeDocId = null;
  versionState.error = null;
  versionState.availabilityReason = null;
  versionState.savePromptVisible = false;
  exitVersionReview();
}
