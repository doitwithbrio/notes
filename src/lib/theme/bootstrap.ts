import { THEME_BOOTSTRAP_KEY, type ThemeBootstrapSnapshot } from './schema.js';

function canUseStorage() {
  return typeof window !== 'undefined' && typeof localStorage !== 'undefined';
}

export function readThemeBootstrapSnapshot(): ThemeBootstrapSnapshot | null {
  if (!canUseStorage()) return null;

  try {
    const raw = localStorage.getItem(THEME_BOOTSTRAP_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<ThemeBootstrapSnapshot>;
    if (parsed.v !== 1) return null;
    if (parsed.mode !== 'system' && parsed.mode !== 'light' && parsed.mode !== 'dark') return null;
    if (parsed.accent !== 'amber' && parsed.accent !== 'slate' && parsed.accent !== 'clay' && parsed.accent !== 'olive') return null;
    if (parsed.resolved !== 'light' && parsed.resolved !== 'dark') return null;
    return parsed as ThemeBootstrapSnapshot;
  } catch {
    return null;
  }
}

export function writeThemeBootstrapSnapshot(snapshot: ThemeBootstrapSnapshot) {
  if (!canUseStorage()) return;

  try {
    localStorage.setItem(THEME_BOOTSTRAP_KEY, JSON.stringify(snapshot));
  } catch {
    // Ignore storage failures.
  }
}

export function clearThemeBootstrapSnapshot() {
  if (!canUseStorage()) return;

  try {
    localStorage.removeItem(THEME_BOOTSTRAP_KEY);
  } catch {
    // Ignore storage failures.
  }
}
