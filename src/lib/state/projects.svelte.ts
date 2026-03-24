import type { Project } from '../types/index.js';

export const projectState = $state({
  projects: [] as Project[],
});
