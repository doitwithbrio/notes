import type { Document } from '../types/index.js';

export const documentState = $state({
  activeDocId: null as string | null,
  docs: [] as Document[],
});

export function getActiveDoc(): Document | null {
  return documentState.docs.find((d) => d.id === documentState.activeDocId) ?? null;
}

export function setActiveDoc(docId: string | null) {
  documentState.activeDocId = docId;
}
