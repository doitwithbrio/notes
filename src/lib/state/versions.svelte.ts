/**
 * Version list state management.
 *
 * Preview/review UI state lives in `version-review.svelte.ts`.
 */

import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import type { BackendVersion } from '../types/index.js';

export const versionState = $state({
  versions: [] as BackendVersion[],
  loading: false,
  error: null as string | null,
  activeDocId: null as string | null,
  supported: true,
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

function disableVersionFeatures(message: string, reason?: unknown) {
  versionState.supported = false;
  versionState.loading = false;
  versionState.error = message;
  versionState.versions = [];
  if (reason) {
    console.warn(message, getErrorMessage(reason));
  }
}

function disableVersionApi(reason: unknown) {
  disableVersionFeatures(
    'Version history requires a desktop app restart so the Rust backend can pick up the new commands.',
    reason,
  );
  if (!warnedUnsupportedVersionApi) {
    warnedUnsupportedVersionApi = true;
    console.warn(
      'Version history API unavailable in current backend build. Fully restart the desktop app after rebuilding the Tauri backend:',
      getErrorMessage(reason),
    );
  }
}

export function getSignificantVersions(): BackendVersion[] {
  return versionState.versions.filter((v) => v.significance !== 'skip');
}

export function getNamedVersions(): BackendVersion[] {
  return versionState.versions.filter((v) => v.type === 'named');
}

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
    console.warn('Failed to load device actor ID:', error);
  }
}

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
    if (versionState.activeDocId === docId) {
      versionState.error = getErrorMessage(error) || 'Failed to load versions';
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

export async function createVersion(project: string, docId: string, label?: string): Promise<BackendVersion | null> {
  if (!versionState.supported) return null;
  try {
    const version = await tauriApi.createVersion(project, docId, label);
    scheduleVersionRefresh(docId, 0);
    return version;
  } catch (error) {
    if (error instanceof TauriRuntimeUnavailableError) return null;
    if (isMissingVersionCommand(error)) {
      disableVersionApi(error);
      return null;
    }
    const msg = getErrorMessage(error);
    if (msg.includes('no significant changes')) return null;
    versionState.error = msg || 'Failed to create version';
    console.warn('Failed to create version:', error);
    return null;
  }
}

export function clearVersions() {
  for (const timer of refreshTimers.values()) {
    clearTimeout(timer);
  }
  refreshTimers.clear();
  versionState.versions = [];
  versionState.activeDocId = null;
  versionState.error = null;
}
