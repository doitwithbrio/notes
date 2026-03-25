import { tauriApi } from '../api/tauri.js';
import { TauriRuntimeUnavailableError } from '../runtime/tauri.js';
import type { AppSettings } from '../types/index.js';

export const settingsState = $state({
  settings: null as AppSettings | null,
  loading: false,
  error: null as string | null,
});

export async function loadSettings() {
  settingsState.loading = true;
  settingsState.error = null;
  try {
    settingsState.settings = await tauriApi.getSettings();
  } catch (err) {
    if (err instanceof TauriRuntimeUnavailableError) {
      // Use sensible defaults when not in Tauri
      settingsState.settings = {
        displayName: 'local',
        customRelays: [],
        theme: 'system',
        fontSize: 16,
        autoSave: true,
        saveIntervalSecs: 5,
        largeDocWarningWords: 10000,
        idleDocTimeoutSecs: 0,
      };
      return;
    }
    settingsState.error = err instanceof Error ? err.message : 'Failed to load settings';
    console.error('Failed to load settings:', err);
  } finally {
    settingsState.loading = false;
  }
}

export async function saveSettings(settings: AppSettings) {
  settingsState.settings = settings;
  try {
    await tauriApi.updateSettings(settings);
  } catch (err) {
    if (!(err instanceof TauriRuntimeUnavailableError)) {
      console.error('Failed to save settings:', err);
    }
  }
}
