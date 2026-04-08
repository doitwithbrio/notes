import type { EditorDocument, EditorMark, EditorNode } from './schema.js';
import { parseExplicitTaskLine } from './task-markdown.js';

type ClipboardLike = {
  getData(type: string): string;
};

function textNode(text: string, marks?: EditorMark[]): EditorNode {
  return marks && marks.length > 0 ? { type: 'text', text, marks } : { type: 'text', text };
}

function paragraphNode(content: EditorNode[]): EditorNode {
  return { type: 'paragraph', content };
}

function headingNode(level: number, content: EditorNode[]): EditorNode {
  return { type: 'heading', attrs: { level }, content };
}

function paragraphFromLines(lines: string[]): EditorNode {
  const content: EditorNode[] = [];

  lines.forEach((line, index) => {
    content.push(...parseInlineMarkdown(line));
    if (index < lines.length - 1) {
      content.push({ type: 'hard_break' });
    }
  });

  return paragraphNode(content);
}

function taskItemNode(text: string, checked: boolean): EditorNode {
  return {
    type: 'task_item',
    attrs: { checked },
    content: [paragraphNode(parseInlineMarkdown(text))],
  };
}

function bulletListNode(items: string[]): EditorNode {
  return {
    type: 'bullet_list',
    content: items.map((item) => ({
      type: 'list_item',
      content: [paragraphNode(parseInlineMarkdown(item))],
    })),
  };
}

function taskListNode(items: Array<{ text: string; checked: boolean }>): EditorNode {
  return {
    type: 'task_list',
    content: items.map((item) => taskItemNode(item.text, item.checked)),
  };
}

function parseInlineMarkdown(text: string): EditorNode[] {
  const nodes: EditorNode[] = [];
  const pattern = /(\*\*([^*]+)\*\*|_([^_]+)_|`([^`]+)`)/g;
  let lastIndex = 0;

  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > lastIndex) {
      nodes.push(textNode(text.slice(lastIndex, index)));
    }

    if (match[2] !== undefined) {
      nodes.push(textNode(match[2], [{ type: 'bold' }]));
    } else if (match[3] !== undefined) {
      nodes.push(textNode(match[3], [{ type: 'italic' }]));
    } else if (match[4] !== undefined) {
      nodes.push(textNode(match[4], [{ type: 'code' }]));
    }

    lastIndex = index + match[0].length;
  }

  if (lastIndex < text.length) {
    nodes.push(textNode(text.slice(lastIndex)));
  }

  return nodes.length > 0 ? nodes : [textNode(text)];
}

function parseBlocks(lines: string[]): EditorNode[] {
  const blocks: EditorNode[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index] ?? '';

    if (line.length === 0) {
      blocks.push(paragraphNode([]));
      index += 1;
      continue;
    }

    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading) {
      const hashes = heading[1] ?? '';
      const headingText = heading[2] ?? '';
      blocks.push(headingNode(hashes.length, parseInlineMarkdown(headingText)));
      index += 1;
      continue;
    }

    const quote = /^>\s?(.*)$/.exec(line);
    if (quote) {
      blocks.push({ type: 'blockquote', content: [paragraphNode(parseInlineMarkdown(quote[1] ?? ''))] });
      index += 1;
      continue;
    }

    const taskItems: Array<{ text: string; checked: boolean }> = [];
    let cursor = index;
    while (cursor < lines.length) {
      const task = parseExplicitTaskLine(lines[cursor] ?? '');
      if (!task) break;
      taskItems.push(task);
      cursor += 1;
    }
    if (taskItems.length > 0) {
      blocks.push(taskListNode(taskItems));
      index = cursor;
      continue;
    }

    const bulletItems: string[] = [];
    cursor = index;
    while (cursor < lines.length) {
      const bullet = /^- (.+)$/.exec(lines[cursor] ?? '');
      const bulletText = bullet?.[1] ?? '';
      if (!bullet || /^\[( |x|X)\] /.test(bulletText)) break;
      bulletItems.push(bulletText);
      cursor += 1;
    }
    if (bulletItems.length > 0) {
      blocks.push(bulletListNode(bulletItems));
      index = cursor;
      continue;
    }

    const paragraphLines: string[] = [];
    cursor = index;
    while (cursor < lines.length && (lines[cursor] ?? '') !== '') {
      const candidate = lines[cursor] ?? '';
      if (/^(#{1,6})\s+/.test(candidate) || /^>\s?/.test(candidate) || /^- \[( |x|X)\] /.test(candidate) || /^- /.test(candidate)) {
        if (cursor === index) {
          paragraphLines.push(candidate);
          cursor += 1;
        }
        break;
      }
      paragraphLines.push(candidate);
      cursor += 1;
    }

    const joined = paragraphLines.join('\n');
    if (joined.length > 0) {
      blocks.push(paragraphFromLines(paragraphLines));
      index = cursor;
      continue;
    }

    blocks.push(paragraphNode(parseInlineMarkdown(line)));
    index += 1;
  }

  return blocks;
}

export function getPreferredPasteText(clipboardData: ClipboardLike | null | undefined): string {
  if (!clipboardData) return '';
  return clipboardData.getData('text/plain') || '';
}

export function parsePlainTextPaste(text: string): EditorDocument {
  if (!text) {
    return {
      schemaVersion: 2,
      doc: { type: 'doc', content: [paragraphNode([])] },
    };
  }
  return {
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: parseBlocks(text.replace(/\r\n?/g, '\n').split('\n')),
    },
  };
}
