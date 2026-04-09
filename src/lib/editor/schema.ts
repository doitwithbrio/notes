export type EditorMark = {
  type: string;
  attrs?: Record<string, unknown>;
};

export type EditorNode = {
  type: string;
  attrs?: Record<string, unknown>;
  content?: EditorNode[];
  text?: string;
  marks?: EditorMark[];
};

export type EditorDocument = {
  schemaVersion: 2;
  doc: EditorNode;
};

function createTextNode(text: string): EditorNode {
  return { type: 'text', text };
}

export function createParagraphNode(text: string): EditorNode {
  const lines = text.split('\n');
  const content: EditorNode[] = [];

  lines.forEach((line, index) => {
    if (line.length > 0) {
      content.push(createTextNode(line));
    }
    if (index < lines.length - 1) {
      content.push({ type: 'hard_break' });
    }
  });

  return {
    type: 'paragraph',
    content,
  };
}

export function createDocumentFromPlainText(text: string): EditorDocument {
  const lines = text.split('\n');
  const content: EditorNode[] = [];
  let currentParagraph: string[] = [];

  const flushParagraph = () => {
    if (currentParagraph.length === 0) return;
    content.push(createParagraphNode(currentParagraph.join('\n')));
    currentParagraph = [];
  };

  for (const line of lines) {
    if (line.length === 0) {
      flushParagraph();
      content.push(createParagraphNode(''));
      continue;
    }

    currentParagraph.push(line);
  }

  flushParagraph();

  if (content.length === 0) {
    content.push(createParagraphNode(''));
  }

  return {
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content,
    },
  };
}

function collectInlineText(node: EditorNode | undefined): string {
  if (!node) return '';
  if (node.type === 'text') return node.text ?? '';
  if (node.type === 'hard_break') return '\n';
  if (!node.content || node.content.length === 0) return '';
  return node.content.map((child) => collectInlineText(child)).join('');
}

function getTaskItemText(node: EditorNode): string {
  return (node.content ?? [])
    .filter((child) => child.type !== 'task_list')
    .map((child) => getVisibleTextFromNode(child))
    .join('')
    .trim();
}

function serializeDocBlocks(nodes: EditorNode[]): string {
  let output = '';

  nodes.forEach((node, index) => {
    const value = getVisibleTextFromNode(node);
    output += value;

    if (index >= nodes.length - 1) return;
    const nextValue = getVisibleTextFromNode(nodes[index + 1]);
    output += value.length === 0 || nextValue.length === 0 ? '\n' : '\n\n';
  });

  return output;
}

export function getVisibleTextFromNode(node: EditorNode | undefined): string {
  if (!node) return '';

  switch (node.type) {
    case 'doc':
      return serializeDocBlocks(node.content ?? []);
    case 'paragraph':
    case 'heading':
    case 'blockquote':
    case 'list_item':
      return collectInlineText(node);
    case 'task_item':
      return `- [${node.attrs?.checked === true ? 'x' : ' '}] ${getTaskItemText(node)}`.trimEnd();
    case 'bullet_list':
    case 'ordered_list':
    case 'task_list':
      return (node.content ?? []).map((child) => getVisibleTextFromNode(child)).join('\n');
    case 'code_block':
      return collectInlineText(node);
    case 'horizontal_rule':
      return '---';
    case 'image':
      return String(node.attrs?.alt ?? '');
    case 'hard_break':
      return '\n';
    case 'text':
      return node.text ?? '';
    default:
      return collectInlineText(node);
  }
}

export function getVisibleTextFromDocument(document: EditorDocument): string {
  return getVisibleTextFromNode(document.doc);
}
