import * as Automerge from '@automerge/automerge';

import {
  createDocumentFromPlainText,
  getVisibleTextFromDocument,
  type EditorDocument,
  type EditorNode,
} from './schema.js';
import { normalizeInlineTodoIds } from './inline-todos.js';

export type LegacyStoredNoteDoc = {
  schemaVersion?: number;
  text?: string;
};

export type GraphStoredNoteDoc = {
  schemaVersion?: number;
  doc?: EditorNode;
};

export type StoredNoteDoc = LegacyStoredNoteDoc & GraphStoredNoteDoc;

export type LoadedEditorDocument = {
  storageDoc: Automerge.Doc<StoredNoteDoc>;
  editorDocument: EditorDocument;
  visibleText: string;
  sourceSchema: 'legacy-text' | 'graph-v2';
  needsMigration: boolean;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isEditorNode(value: unknown): value is EditorNode {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  if ('text' in value && value.text !== undefined && typeof value.text !== 'string') return false;
  if ('content' in value && value.content !== undefined) {
    if (!Array.isArray(value.content)) return false;
    if (!value.content.every((child) => isEditorNode(child))) return false;
  }
  return true;
}

function isEditorDocumentNode(value: unknown): value is EditorNode {
  return isEditorNode(value) && value.type === 'doc';
}

function getLegacyVisibleText(doc: Automerge.Doc<StoredNoteDoc>): string {
  return typeof doc.text === 'string' ? doc.text : String(doc.text ?? '');
}

export function loadEditorDocument(binary: Uint8Array): LoadedEditorDocument {
  const storageDoc = Automerge.load<StoredNoteDoc>(binary);

  if (storageDoc.schemaVersion === 2) {
    if (!isEditorDocumentNode(storageDoc.doc)) {
      throw new Error('Stored v2 note is missing a valid root document node');
    }

    const plainDoc = JSON.parse(JSON.stringify(storageDoc.doc)) as EditorNode;
    const editorDocument: EditorDocument = {
      schemaVersion: 2,
      doc: plainDoc,
    };
    const normalizedDocument = normalizeInlineTodoIds(editorDocument);
    return {
      storageDoc,
      editorDocument: normalizedDocument,
      visibleText: getVisibleTextFromDocument(normalizedDocument),
      sourceSchema: 'graph-v2',
      needsMigration: false,
    };
  }

  const legacyText = getLegacyVisibleText(storageDoc);
  const editorDocument = normalizeInlineTodoIds(createDocumentFromPlainText(legacyText));
  return {
    storageDoc,
    editorDocument,
    visibleText: legacyText,
    sourceSchema: 'legacy-text',
    needsMigration: true,
  };
}

export function buildStoredDocumentUpdate(
  storageDoc: Automerge.Doc<StoredNoteDoc>,
  editorDocument: EditorDocument,
  visibleText: string,
) {
  const sanitizedDoc = JSON.parse(JSON.stringify(editorDocument.doc)) as EditorDocument['doc'];
  const nextDoc = Automerge.change(storageDoc, (doc) => {
    doc.schemaVersion = 2;
    doc.doc = sanitizedDoc;
    doc.text = visibleText;
  });

  return {
    storageDoc: nextDoc,
    incremental: Automerge.saveIncremental(nextDoc),
  };
}
