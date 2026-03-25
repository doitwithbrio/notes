import { ACCENT_COLORS } from '../types/index.js';
import type { ResolvedAppearance } from './schema.js';

export function applyAppearanceToDocument(appearance: ResolvedAppearance) {
  if (typeof document === 'undefined') return;

  const root = document.documentElement;
  const accent = ACCENT_COLORS.find((color) => color.id === appearance.accent) ?? ACCENT_COLORS[0]!;
  const tints = accent.tints[appearance.resolvedTheme];

  root.dataset.theme = appearance.resolvedTheme;
  root.dataset.themeMode = appearance.mode;
  root.dataset.accent = appearance.accent;
  root.style.setProperty('--accent', accent.hex);
  root.style.colorScheme = appearance.resolvedTheme;

  // Apply accent-tinted surface tokens
  root.style.setProperty('--bg-tint', tints.bgTint);
  root.style.setProperty('--surface', tints.surface);
  root.style.setProperty('--surface-hover', tints.surfaceHover);
  root.style.setProperty('--surface-active', tints.surfaceActive);
  root.style.setProperty('--surface-sidebar', tints.surfaceSidebar);
  root.style.setProperty('--border-subtle', tints.borderSubtle);
  root.style.setProperty('--border-default', tints.borderDefault);
  root.style.setProperty('--overlay-backdrop', tints.overlayBackdrop);
}
