export type Platform = 'macos' | 'windows' | 'linux';

/**
 * Detect the current platform.
 * In Tauri, we'd use @tauri-apps/plugin-os. For now, sniff the user agent.
 */
export function getPlatform(): Platform {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) return 'macos';
  if (ua.includes('win')) return 'windows';
  return 'linux';
}

export const isMac = getPlatform() === 'macos';

/** The modifier key label for keyboard shortcuts */
export const modKey = isMac ? '\u2318' : 'Ctrl';
