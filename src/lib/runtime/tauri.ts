export class TauriRuntimeUnavailableError extends Error {
  constructor(context: string) {
    super(`Tauri runtime unavailable: ${context}`);
    this.name = 'TauriRuntimeUnavailableError';
  }
}

type TauriInternals = {
  invoke?: unknown;
  transformCallback?: unknown;
};

function readTauriInternals(): TauriInternals | undefined {
  if (typeof window === 'undefined') return undefined;
  return (window as typeof window & { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
}

export function hasTauriInternals(): boolean {
  const internals = readTauriInternals();
  return !!internals;
}

export function isTauriRuntimeReady(): boolean {
  const internals = readTauriInternals();
  return !!internals?.invoke && !!internals?.transformCallback;
}

export async function waitForTauriRuntime(timeoutMs: number): Promise<boolean> {
  if (isTauriRuntimeReady()) return true;

  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    await new Promise((resolve) => setTimeout(resolve, 25));
    if (isTauriRuntimeReady()) {
      return true;
    }
  }

  return false;
}

export function assertTauriRuntime(context: string) {
  if (!isTauriRuntimeReady()) {
    throw new TauriRuntimeUnavailableError(context);
  }
}
