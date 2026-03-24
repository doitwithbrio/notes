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

export function createEditor(element: HTMLElement): Editor {
  return new Editor({
    element,
    extensions: [
      StarterKit.configure({
        // Disable default codeBlock in favor of lowlight version
        codeBlock: false,
        dropcursor: false,
        gapcursor: false,
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
        placeholder: 'Start writing...',
      }),
      Typography,
      Dropcursor.configure({
        color: '#2AC994',
        width: 2,
      }),
      Gapcursor,
    ],
    content: '',
    editorProps: {
      attributes: {
        class: 'editor-content',
      },
    },
  });
}
