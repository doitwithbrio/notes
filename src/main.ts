import { mount } from 'svelte';
import App from './lib/components/App.svelte';
import { readThemeBootstrapSnapshot } from './lib/theme/bootstrap.js';
import { applyAppearanceToDocument } from './lib/theme/dom.js';
import { defaultAppearance, resolveAppearance } from './lib/theme/schema.js';

const bootstrapSnapshot = readThemeBootstrapSnapshot();
const prefersDark =
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches
    : false;

applyAppearanceToDocument(
  resolveAppearance(
    bootstrapSnapshot
      ? { mode: bootstrapSnapshot.mode, accent: bootstrapSnapshot.accent }
      : defaultAppearance(),
    prefersDark,
  ),
);

const app = mount(App, {
  target: document.getElementById('app')!,
});

export default app;
