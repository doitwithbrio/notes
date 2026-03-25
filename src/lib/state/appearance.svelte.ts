import type { AppearanceSettings } from '../types/index.js';
import { writeThemeBootstrapSnapshot } from '../theme/bootstrap.js';
import { applyAppearanceToDocument } from '../theme/dom.js';
import {
  defaultAppearance,
  normalizeAppearance,
  resolveAppearance,
  type ResolvedAppearance,
} from '../theme/schema.js';

export const appearanceState = $state({
  preference: defaultAppearance() as AppearanceSettings,
  resolvedTheme: 'light' as ResolvedAppearance['resolvedTheme'],
  systemTheme: 'light' as ResolvedAppearance['resolvedTheme'],
  ready: false,
});

let mediaQuery: MediaQueryList | null = null;
let removeSystemListener: (() => void) | null = null;

function prefersDarkTheme() {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return false;
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

function applyResolvedAppearance(preference: AppearanceSettings | null | undefined, persistBootstrap: boolean) {
  const normalized = normalizeAppearance(preference);
  const resolved = resolveAppearance(normalized, prefersDarkTheme());
  appearanceState.preference = normalized;
  appearanceState.resolvedTheme = resolved.resolvedTheme;
  appearanceState.systemTheme = prefersDarkTheme() ? 'dark' : 'light';
  appearanceState.ready = true;

  applyAppearanceToDocument(resolved);

  if (persistBootstrap) {
    writeThemeBootstrapSnapshot({
      v: 1,
      mode: resolved.mode,
      accent: resolved.accent,
      resolved: resolved.resolvedTheme,
    });
  }
}

function handleSystemThemeChange() {
  applyResolvedAppearance(appearanceState.preference, true);
}

function ensureSystemListener() {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;

  if (!mediaQuery) {
    mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
  }

  if (appearanceState.preference.mode === 'system') {
    if (!removeSystemListener) {
      const listener = () => handleSystemThemeChange();
      if (typeof mediaQuery.addEventListener === 'function') {
        mediaQuery.addEventListener('change', listener);
        removeSystemListener = () => mediaQuery?.removeEventListener('change', listener);
      } else {
        mediaQuery.addListener(listener);
        removeSystemListener = () => mediaQuery?.removeListener(listener);
      }
    }
    return;
  }

  removeSystemListener?.();
  removeSystemListener = null;
}

export function syncAppearancePreference(preference: AppearanceSettings, persistBootstrap = false) {
  applyResolvedAppearance(preference, persistBootstrap);
  ensureSystemListener();
}

export function getEffectiveThemeLabel() {
  return appearanceState.preference.mode === 'system'
    ? appearanceState.systemTheme
    : appearanceState.preference.mode;
}

export function teardownAppearance() {
  removeSystemListener?.();
  removeSystemListener = null;
}
