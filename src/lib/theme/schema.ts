import {
  ACCENT_COLORS,
  DEFAULT_ACCENT,
  type AccentColorId,
  type AppearanceSettings,
  type ThemeMode,
} from '../types/index.js';

export const THEME_BOOTSTRAP_KEY = 'p2p-notes-theme-bootstrap';

export type ResolvedTheme = 'light' | 'dark';

export interface ThemeBootstrapSnapshot {
  v: 1;
  mode: ThemeMode;
  accent: AccentColorId;
  resolved: ResolvedTheme;
}

export interface ResolvedAppearance {
  mode: ThemeMode;
  accent: AccentColorId;
  resolvedTheme: ResolvedTheme;
}

export function defaultAppearance(): AppearanceSettings {
  return {
    mode: 'system',
    accent: DEFAULT_ACCENT,
  };
}

export function isAccentColorId(value: unknown): value is AccentColorId {
  return typeof value === 'string' && ACCENT_COLORS.some((color) => color.id === value);
}

export function normalizeAppearance(input: Partial<AppearanceSettings> | null | undefined): AppearanceSettings {
  return {
    mode: input?.mode === 'light' || input?.mode === 'dark' || input?.mode === 'system'
      ? input.mode
      : 'system',
    accent: isAccentColorId(input?.accent) ? input.accent : DEFAULT_ACCENT,
  };
}

export function resolveTheme(mode: ThemeMode, prefersDark: boolean): ResolvedTheme {
  if (mode === 'light') return 'light';
  if (mode === 'dark') return 'dark';
  return prefersDark ? 'dark' : 'light';
}

export function resolveAppearance(
  appearance: Partial<AppearanceSettings> | null | undefined,
  prefersDark: boolean,
): ResolvedAppearance {
  const normalized = normalizeAppearance(appearance);
  return {
    mode: normalized.mode,
    accent: normalized.accent,
    resolvedTheme: resolveTheme(normalized.mode, prefersDark),
  };
}
