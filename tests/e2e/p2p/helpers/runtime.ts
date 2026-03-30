import { existsSync } from 'node:fs';
import path from 'node:path';

export const APP_INSTANCE_NAMES = ['owner', 'editor', 'viewer'] as const;

export type AppInstanceName = (typeof APP_INSTANCE_NAMES)[number];

export function isSupportedDesktopE2EPlatform() {
  return process.platform === 'linux' || process.platform === 'win32';
}

export function resolveTauriAppPath(explicitPath = process.env.TAURI_APP_PATH) {
  if (explicitPath && existsSync(explicitPath)) {
    return explicitPath;
  }

  const cwd = process.cwd();
  const candidates = process.platform === 'win32'
    ? [
        path.join(cwd, 'target', 'debug', 'notes.exe'),
        path.join(cwd, 'target', 'release', 'notes.exe'),
      ]
    : [
        path.join(cwd, 'target', 'debug', 'notes'),
        path.join(cwd, 'target', 'release', 'notes'),
      ];

  const resolved = candidates.find((candidate) => existsSync(candidate));
  if (resolved) {
    return resolved;
  }

  throw new Error(
    'Unable to find a Tauri app binary. Set TAURI_APP_PATH or build the app first.',
  );
}

export function uniqueName(prefix: string) {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}
