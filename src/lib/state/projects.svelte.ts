import { tauriApi } from '../api/tauri.js';
import type { BackendProjectSummary, Project } from '../types/index.js';
import { applyProjectOrder, saveProjectOrder } from './ordering.svelte.js';

export const projectState = $state({
  projects: [] as Project[],
  loading: false,
});

export function mapProject(summary: BackendProjectSummary): Project {
  return {
    id: summary.name,
    name: summary.name,
    path: summary.path,
    shared: summary.shared,
    role: summary.role,
    peerCount: summary.peerCount,
  };
}

export function getProject(projectId: string | null): Project | null {
  if (!projectId) return null;
  return projectState.projects.find((project) => project.id === projectId) ?? null;
}

export async function loadProjects() {
  projectState.loading = true;
  try {
    const projects = await tauriApi.listProjectSummaries();
    projectState.projects = applyProjectOrder(projects.map(mapProject));
  } finally {
    projectState.loading = false;
  }
}

export async function createProject(name: string) {
  await tauriApi.createProject(name);
  await tauriApi.openProject(name);
  await loadProjects();
  return getProject(name);
}

export function removeProject(projectId: string) {
  const index = projectState.projects.findIndex((p) => p.id === projectId);
  if (index >= 0) projectState.projects.splice(index, 1);
}

export function reorderProject(fromIndex: number, toIndex: number) {
  if (fromIndex === toIndex) return;
  const [item] = projectState.projects.splice(fromIndex, 1);
  if (item) projectState.projects.splice(toIndex, 0, item);
  saveProjectOrder(projectState.projects);
}
