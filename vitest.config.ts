import { defineConfig } from 'vitest/config';

import viteConfig from './vite.config.js';

export default defineConfig({
  ...viteConfig,
  resolve: {
    ...viteConfig.resolve,
    conditions: ['browser'],
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
  },
});
