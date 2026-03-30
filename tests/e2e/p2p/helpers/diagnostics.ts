import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

import { multiremotebrowser } from '@wdio/globals';

import { APP_INSTANCE_NAMES, type AppInstanceName } from './runtime.js';

export async function bundleFailureArtifacts(testName: string) {
  const outputDir = path.join(process.cwd(), 'artifacts', 'wdio', sanitize(testName));
  await mkdir(outputDir, { recursive: true });

  for (const name of APP_INSTANCE_NAMES) {
    await captureInstanceArtifacts(name, outputDir);
  }
}

async function captureInstanceArtifacts(name: AppInstanceName, outputDir: string) {
  try {
    const instance = multiremotebrowser.getInstance(name);
    await instance.saveScreenshot(path.join(outputDir, `${name}.png`));
    const source = await instance.getPageSource();
    await writeFile(path.join(outputDir, `${name}.html`), source, 'utf8');
  } catch {
    // Best-effort diagnostics only.
  }
}

function sanitize(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9_-]+/g, '-');
}
