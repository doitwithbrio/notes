import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync, symlinkSync } from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';

function resolveTauriAppPath() {
  if (process.env.TAURI_APP_PATH && existsSync(process.env.TAURI_APP_PATH)) {
    return process.env.TAURI_APP_PATH;
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

  return candidates.find((candidate) => existsSync(candidate)) ?? null;
}

function ensureTauriServiceCompatibilityPath(appPath) {
  const cwd = process.cwd();
  const binaryName = process.platform === 'win32' ? 'notes.exe' : 'notes';
  const compatibilityDir = path.join(cwd, 'src-tauri', 'target', 'release');
  const compatibilityPath = path.join(compatibilityDir, binaryName);

  mkdirSync(compatibilityDir, { recursive: true });

  try {
    if (existsSync(compatibilityPath)) {
      rmSync(compatibilityPath, { force: true });
    }
  } catch {
    // Best-effort cleanup before recreating the compatibility path.
  }

  if (process.platform === 'win32') {
    copyFileSync(appPath, compatibilityPath);
  } else {
    symlinkSync(appPath, compatibilityPath);
    chmodSync(appPath, 0o755);
  }

  return compatibilityPath;
}

if (process.platform === 'darwin') {
  console.error('P2P desktop E2E currently targets Linux/Windows CI. macOS WebDriver support for Tauri is not available.');
  process.exit(1);
}

const appPath = resolveTauriAppPath();
if (!appPath) {
  console.error('No Tauri binary found. Build one first or set TAURI_APP_PATH.');
  process.exit(1);
}

ensureTauriServiceCompatibilityPath(appPath);

const child = spawn(
  process.platform === 'win32' ? 'bunx.cmd' : 'bunx',
  ['wdio', 'run', 'wdio.conf.ts'],
  {
    cwd: process.cwd(),
    stdio: 'inherit',
    env: {
      ...process.env,
      TAURI_APP_PATH: appPath,
      P2P_E2E: '1',
      P2P_MONITOR_INTERVAL_MS: process.env.P2P_MONITOR_INTERVAL_MS ?? '1000',
      P2P_SYNC_DEBOUNCE_MS: process.env.P2P_SYNC_DEBOUNCE_MS ?? '150',
      P2P_INVITE_TTL_SECS: process.env.P2P_INVITE_TTL_SECS ?? '20',
    },
  },
);

child.on('exit', (code) => {
  process.exit(code ?? 1);
});

child.on('error', (error) => {
  console.error(`Failed to start WebdriverIO: ${error.message}`);
  process.exit(1);
});

child.on('close', (_code, signal) => {
  if (signal) {
    console.error(`WebdriverIO was terminated by signal ${signal}`);
    process.exit(1);
  }
});
