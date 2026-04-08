/**
 * Local-only ordering persistence via localStorage.
 * Ordering is a UI preference — it is NOT shared with peers.
 */

import type { Project, Document } from '../types/index.js';

const STORAGE_KEY = 'p2p-notes-ordering';

interface SavedOrder {
  projectOrder: string[];
  docOrder: Record<string, string[]>;
}

let order: SavedOrder = { projectOrder: [], docOrder: {} };

/** Load saved order from localStorage. Call once on app boot. */
export function loadOrder() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      order = {
        projectOrder: Array.isArray(parsed.projectOrder) ? parsed.projectOrder : [],
        docOrder: parsed.docOrder && typeof parsed.docOrder === 'object' ? parsed.docOrder : {},
      };
    }
  } catch {
    // localStorage unavailable or corrupt — fall back to empty order
    order = { projectOrder: [], docOrder: {} };
  }
}

/** Persist current order to localStorage. */
function saveOrder() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(order));
  } catch {
    // localStorage unavailable — silently ignore
  }
}

/**
 * Re-sort projects by saved order.
 * Items in the saved order come first (in their saved positions).
 * Items NOT in saved order are appended alphabetically at the end.
 */
export function applyProjectOrder(projects: Project[]): Project[] {
  if (order.projectOrder.length === 0) return projects;
  const byId = new Map(projects.map((p) => [p.id, p]));
  const ordered: Project[] = [];

  // First: items in saved order
  for (const id of order.projectOrder) {
    const p = byId.get(id);
    if (p) {
      ordered.push(p);
      byId.delete(id);
    }
  }

  // Then: remaining items (new, not in saved order) alphabetically
  const remaining = Array.from(byId.values()).sort((a, b) => a.name.localeCompare(b.name));
  return [...ordered, ...remaining];
}

/**
 * Re-sort docs within a project by saved order.
 * Same logic: saved order first, then remaining alphabetically.
 */
export function applyDocOrder(projectId: string, docs: Document[]): Document[] {
  const savedIds = order.docOrder[projectId];
  if (!savedIds || savedIds.length === 0) {
    return docs.sort((a, b) => a.path.localeCompare(b.path));
  }

  const byId = new Map(docs.map((d) => [d.id, d]));
  const ordered: Document[] = [];

  for (const id of savedIds) {
    const d = byId.get(id);
    if (d) {
      ordered.push(d);
      byId.delete(id);
    }
  }

  const remaining = Array.from(byId.values()).sort((a, b) => a.path.localeCompare(b.path));
  return [...ordered, ...remaining];
}

/** Save the current project order after a reorder. */
export function saveProjectOrder(projects: Project[]) {
  order.projectOrder = projects.map((p) => p.id);
  saveOrder();
}

/** Save the current doc order for a project after a reorder. */
export function saveDocOrder(projectId: string, docs: Document[]) {
  order.docOrder[projectId] = docs.map((d) => d.id);
  saveOrder();
}

export function removeProjectOrder(projectId: string) {
  order.projectOrder = order.projectOrder.filter((id) => id !== projectId);
  delete order.docOrder[projectId];
  saveOrder();
}
