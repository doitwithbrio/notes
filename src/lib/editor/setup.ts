import { Editor } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import Image from '@tiptap/extension-image';
import Link from '@tiptap/extension-link';
import CodeBlockLowlight from '@tiptap/extension-code-block-lowlight';
import { Table, TableRow, TableCell, TableHeader } from '@tiptap/extension-table';
import TaskList from '@tiptap/extension-task-list';
import TaskItem from '@tiptap/extension-task-item';
import Placeholder from '@tiptap/extension-placeholder';
import Typography from '@tiptap/extension-typography';
import Dropcursor from '@tiptap/extension-dropcursor';
import Gapcursor from '@tiptap/extension-gapcursor';
import { common, createLowlight } from 'lowlight';

const lowlight = createLowlight(common);

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

export function editorToPlainText(editor: Editor) {
  return editor.getText({ blockSeparator: '\n\n' });
}

export function createEditor(
  element: HTMLElement,
  initialText: string,
  onTextChange: (text: string) => void,
): Editor {
  return new Editor({
    element,
    extensions: [
      StarterKit.configure({
        codeBlock: false,
        dropcursor: false,
        gapcursor: false,
        // Disable undo/redo — Automerge handles this
        undoRedo: false,
      }),
      Image.configure({
        inline: true,
        allowBase64: false,
      }),
      Link.configure({
        openOnClick: false,
        autolink: true,
      }),
      CodeBlockLowlight.configure({
        lowlight,
      }),
      Table.configure({
        resizable: true,
      }),
      TableRow,
      TableCell,
      TableHeader,
      TaskList,
      TaskItem.configure({
        nested: true,
      }),
      Placeholder.configure({
        placeholder: 'begin writing...',
      }),
      Typography,
      Dropcursor.configure({
        color: '#B68D5E',
        width: 2,
      }),
      Gapcursor,
    ],
    content: textToEditorHtml(initialText),
    editorProps: {
      attributes: {
        class: 'editor-content',
      },
    },
    onUpdate: ({ editor }) => {
      onTextChange(editorToPlainText(editor));
    },
  });
}
