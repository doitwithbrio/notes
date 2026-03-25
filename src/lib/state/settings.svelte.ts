import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import type { AppSettings } from '../types/index.js';
import { normalizeAppearance } from '../theme/schema.js';
import { syncAppearancePreference } from './appearance.svelte.js';

const SETTINGS_SCHEMA_VERSION = 2;

export const settingsState = $state({
  settings: null as AppSettings | null,
  loading: false,
  error: null as string | null,
});

function normalizeSettings(settings: AppSettings): AppSettings {
  return {
    ...settings,
    schemaVersion: SETTINGS_SCHEMA_VERSION,
    appearance: normalizeAppearance(settings.appearance),
  };
}

export async function loadSettings() {
  settingsState.loading = true;
  settingsState.error = null;
  let shouldSyncAppearance = false;
  try {
    settingsState.settings = normalizeSettings(await tauriApi.getSettings());
    shouldSyncAppearance = true;
  } catch (err) {
    if (err instanceof TauriRuntimeUnavailableError) {
      // Use sensible defaults when not in Tauri
      settingsState.settings = normalizeSettings({
        schemaVersion: SETTINGS_SCHEMA_VERSION,
        displayName: 'local',
        customRelays: [],
        appearance: {
          mode: 'system',
          accent: 'amber',
        },
        fontSize: 16,
        autoSave: true,
        saveIntervalSecs: 5,
        largeDocWarningWords: 10000,
        idleDocTimeoutSecs: 0,
      });
      shouldSyncAppearance = true;
      return;
    }
    settingsState.error = err instanceof Error ? err.message : 'Failed to load settings';
    console.error('Failed to load settings:', err);
    return;
  } finally {
    if (shouldSyncAppearance && settingsState.settings) {
      syncAppearancePreference(settingsState.settings.appearance, true);
    }
    settingsState.loading = false;
  }
}

export async function saveSettings(settings: AppSettings) {
  const normalized = normalizeSettings(settings);

  settingsState.settings = normalized;
  try {
    await tauriApi.updateSettings(normalized);
    settingsState.error = null;
    syncAppearancePreference(normalized.appearance, true);
  } catch (err) {
    if (err instanceof TauriRuntimeUnavailableError) {
      settingsState.error = null;
      syncAppearancePreference(normalized.appearance, true);
      return;
    }
    settingsState.error = err instanceof Error ? err.message : 'Failed to save settings';
    console.error('Failed to save settings:', err);
  }
}
