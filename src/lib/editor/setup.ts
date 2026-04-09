import * as Automerge from '@automerge/automerge';

import type { CursorPosition } from '../types/index.js';
import {
  createAutomergeProsemirrorAdapter,
  type AutomergeEditorAdapter,
  type AutomergeEditorAdapterOptions,
  type NotesEditor,
  updateEditorRemotePresence,
} from './automerge-prosemirror-adapter.js';
import type { AdapterChange } from './automerge-prosemirror-adapter.js';
import type { StoredNoteDoc } from './document-adapter.js';
import type { EditorDocument } from './schema.js';
import { getVisibleTextFromDocument } from './schema.js';

function escapeHtml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

export function textToEditorHtml(text: string) {
  if (!text.trim()) return '<p></p>';
  const paragraphs = text.split(/\n{2,}/).map((paragraph) => paragraph.replace(/\n/g, '<br />'));
  return paragraphs.map((paragraph) => `<p>${escapeHtml(paragraph)}</p>`).join('');
}

export type Editor = NotesEditor;
export type EditorAdapter = AutomergeEditorAdapter;

export function editorToPlainText(editor: Editor) {
  return getVisibleTextFromDocument(editorToDocument(editor));
}

export function editorToDocument(editor: Editor): EditorDocument {
  return {
    schemaVersion: 2,
    doc: editor.getJSON(),
  };
}

export function createEditorAdapter(
  element: HTMLElement,
  options: AutomergeEditorAdapterOptions = {},
) {
  return createAutomergeProsemirrorAdapter(element, options);
}

export function createEditor(
  element: HTMLElement,
  initialDocument: EditorDocument,
  onDocumentChange: (document: EditorDocument, text: string) => void,
  onSelectionChange?: (cursorPos: number | null, selection: [number, number] | null) => void,
  onFocusChange?: (focused: boolean) => void,
): Editor {
  const adapter = createEditorAdapter(element, {
    onChange: ({ source, document, text }: AdapterChange) => {
      if (source === 'local') {
        onDocumentChange(document, text);
      }
    },
    onSelectionChange,
    onFocusChange,
  });

  const doc = Automerge.from<StoredNoteDoc>({
    schemaVersion: 2,
    doc: initialDocument.doc,
    text: getVisibleTextFromDocument(initialDocument),
  });
  adapter.attach(doc, true);
  const editor = adapter.getEditor();
  if (!editor) {
    throw new Error('Failed to create editor');
  }
  return editor;
}

export function updateRemotePresence(editor: Editor, cursors: CursorPosition[]) {
  updateEditorRemotePresence(editor, cursors);
}
