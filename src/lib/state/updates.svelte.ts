/**
 * Update state management.
 *
 * Lifecycle:
 *   idle → checking → available → downloading → installing → ready → (relaunch)
 *                  ↘ idle (no update)
 *                  ↘ error (network/signature failure)
 *
 * checkForUpdate() is called silently on app startup and manually from Settings.
 * installUpdate() is called when the user clicks "Install & Restart".
 * After install completes, we auto-relaunch after 1.5s so the user sees the "ready" state.
 */

import { Channel } from '@tauri-apps/api/core';
import { invoke } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { relaunch } from '@tauri-apps/plugin-process';

import type { UpdateInfo, UpdateStatus, UpdaterAvailability } from '../types/index.js';

export const updateState = $state({
  status: 'idle' as UpdateStatus,
  currentVersion: '',
  info: null as UpdateInfo | null,
  updaterEnabled: true,
  updaterReason: null as string | null,
  updaterChecked: false,
  /** Download progress 0–100. */
  progress: 0,
  /** Total bytes to download (from Content-Length header). */
  totalBytes: 0,
  /** Bytes downloaded so far. */
  downloadedBytes: 0,
  /** Human-readable error message if status is 'error'. */
  error: null as string | null,
  /** Timestamp of the last explicit or silent check. */
  lastCheckedAt: null as number | null,
  /** Result of the last manual check. Silent startup checks leave this alone unless an update is found. */
  lastCheckResult: 'idle' as 'idle' | 'up-to-date' | 'available' | 'error',
});

let versionPromise: Promise<void> | null = null;
let availabilityPromise: Promise<void> | null = null;

export async function ensureUpdaterAvailability(): Promise<void> {
  if (updateState.updaterChecked || availabilityPromise) {
    return availabilityPromise ?? Promise.resolve();
  }

  availabilityPromise = invoke<UpdaterAvailability>('get_updater_availability')
    .then((availability) => {
      updateState.updaterEnabled = availability.enabled;
      updateState.updaterReason = availability.reason;
      updateState.updaterChecked = true;
      if (!availability.enabled) {
        updateState.status = 'idle';
        updateState.info = null;
      }
    })
    .catch(() => {
      updateState.updaterEnabled = true;
      updateState.updaterReason = null;
      updateState.updaterChecked = true;
    })
    .finally(() => {
      availabilityPromise = null;
    });

  return availabilityPromise;
}

/** Load the current installed app version once so Settings can always show it. */
export async function ensureCurrentVersion(): Promise<void> {
  if (updateState.currentVersion || versionPromise) {
    return versionPromise ?? Promise.resolve();
  }

  versionPromise = getVersion()
    .then((version) => {
      updateState.currentVersion = version;
    })
    .catch(() => {
      // Non-fatal: Settings will simply omit the version if runtime metadata is unavailable.
    })
    .finally(() => {
      versionPromise = null;
    });

  return versionPromise;
}

/**
 * Check GitHub Releases for a newer version.
 *
 * @param silent - If true, errors are swallowed (used for the background startup check).
 *                 If false, errors are surfaced in the UI via updateState.error.
 * @returns true if an update is available.
 */
export async function checkForUpdate(silent = false): Promise<boolean> {
  await ensureUpdaterAvailability();
  if (!updateState.updaterEnabled) {
    updateState.status = 'idle';
    updateState.info = null;
    return false;
  }

  if (updateState.status === 'checking' || updateState.status === 'downloading') return false;

  updateState.status = 'checking';
  updateState.error = null;
  updateState.lastCheckedAt = Date.now();

  await ensureCurrentVersion();

  try {
    // Calls the Rust check_for_update command, which fetches latest.json,
    // compares semver, and returns UpdateInfo if newer, null if current.
    const info = await invoke<UpdateInfo | null>('check_for_update');

    if (info) {
      updateState.info = info;
      updateState.currentVersion = info.currentVersion || updateState.currentVersion;
      updateState.status = 'available';
      updateState.lastCheckResult = 'available';
      return true;
    }

    // Already on latest version
    updateState.info = null;
    updateState.status = 'idle';
    if (!silent) {
      updateState.lastCheckResult = 'up-to-date';
    }
    return false;
  } catch (err) {
    if (!silent) {
      updateState.error = err instanceof Error ? err.message : String(err);
      updateState.status = 'error';
      updateState.lastCheckResult = 'error';
    } else {
      // Silent mode: don't bother the user, just go back to idle
      updateState.status = 'idle';
    }
    return false;
  }
}

/**
 * Download and install the pending update.
 *
 * Creates a Tauri Channel to receive real-time progress events from Rust:
 * - Started: total download size (if known)
 * - Progress: each chunk of bytes downloaded
 * - Finished: download complete, install starting
 *
 * After installation, auto-relaunches the app.
 */
export async function installUpdate(): Promise<void> {
  await ensureUpdaterAvailability();
  if (!updateState.updaterEnabled) {
    updateState.status = 'idle';
    return;
  }

  if (updateState.status !== 'available' || !updateState.info) return;

  updateState.status = 'downloading';
  updateState.progress = 0;
  updateState.downloadedBytes = 0;
  updateState.totalBytes = 0;

  try {
    // Create a Channel — this is Tauri's mechanism for streaming events
    // from Rust to JS. The Rust side sends DownloadEvent variants through it.
    const onEvent = new Channel<{ event: string; data: Record<string, unknown> }>();

    onEvent.onmessage = (message) => {
      switch (message.event) {
        case 'Started': {
          const contentLength = message.data?.contentLength as number | undefined;
          if (contentLength) {
            updateState.totalBytes = contentLength;
          }
          break;
        }
        case 'Progress': {
          const chunkLength = message.data?.chunkLength as number;
          updateState.downloadedBytes += chunkLength;
          if (updateState.totalBytes > 0) {
            updateState.progress = Math.round(
              (updateState.downloadedBytes / updateState.totalBytes) * 100,
            );
          }
          break;
        }
        case 'Finished':
          updateState.status = 'installing';
          break;
      }
    };

    // This calls the Rust install_update command, which downloads the
    // platform-specific updater bundle from GitHub Releases, verifies the
    // minisign signature against the embedded pubkey, and lets Tauri apply it.
    await invoke('install_update', { onEvent });

    updateState.status = 'ready';

    // Brief delay so the user sees "Update installed" before the app restarts
    setTimeout(() => {
      void relaunch().catch((err) => {
        updateState.error = err instanceof Error ? err.message : String(err);
        updateState.status = 'error';
        updateState.lastCheckResult = 'error';
      });
    }, 1500);
  } catch (err) {
    updateState.error = err instanceof Error ? err.message : String(err);
    updateState.status = 'error';
    updateState.lastCheckResult = 'error';
  }
}

/** Dismiss the update banner (user doesn't want to update right now). */
export function dismissUpdate() {
  updateState.status = 'idle';
  updateState.info = null;
  updateState.error = null;
}
