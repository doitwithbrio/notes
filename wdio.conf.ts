import { bundleFailureArtifacts } from './tests/e2e/p2p/helpers/diagnostics.js';
import { isSupportedDesktopE2EPlatform, resolveTauriAppPath } from './tests/e2e/p2p/helpers/runtime.js';

const appPath = resolveTauriAppPath();
const serviceAppPath = process.cwd();

function tauriCapability() {
  return {
    browserName: 'tauri',
    'tauri:options': {
      application: appPath,
      args: [],
    },
    'wdio:tauriServiceOptions': {
      captureBackendLogs: true,
      captureFrontendLogs: true,
      backendLogLevel: 'info',
      frontendLogLevel: 'info',
    },
  };
}

function tauriMultiremoteCapability() {
  const capability = tauriCapability();
  return {
    ...capability,
    capabilities: {
      ...capability,
      'tauri:options': {
        application: serviceAppPath,
        args: [],
      },
    },
  };
}

export const config = {
  runner: 'local',
  specs: ['./tests/e2e/p2p/**/*.spec.ts'],
  maxInstances: 1,
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 120_000,
    retries: process.env.CI ? 1 : 0,
  },
  reporters: ['spec'],
  services: [[
    '@wdio/tauri-service',
    {
      autoInstallTauriDriver: true,
      commandTimeout: 30_000,
      debug: Boolean(process.env.DEBUG_TAURI_E2E),
    },
  ]],
  capabilities: {
    owner: tauriMultiremoteCapability(),
    editor: tauriMultiremoteCapability(),
    viewer: tauriMultiremoteCapability(),
  },
  before() {
    if (!isSupportedDesktopE2EPlatform()) {
      throw new Error('P2P desktop E2E is currently supported only on Linux and Windows runners.');
    }
  },
  afterTest: async function (test: { parent?: string; title: string }, _context: unknown, result: { passed: boolean }) {
    if (!result.passed) {
      await bundleFailureArtifacts(`${test.parent} ${test.title}`);
    }
  },
};
