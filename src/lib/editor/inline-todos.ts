import type { EditorDocument, EditorNode } from './schema.js';

export type InlineTodoItem = {
  id: string;
  text: string;
  done: boolean;
  order: number;
  depth: number;
};

type NormalizeOptions = {
  createId?: () => string;
};

function cloneNode(node: EditorNode): EditorNode {
  return {
    ...node,
    attrs: node.attrs ? { ...node.attrs } : undefined,
    marks: node.marks?.map((mark) => ({ ...mark, attrs: mark.attrs ? { ...mark.attrs } : undefined })),
    content: node.content?.map((child) => cloneNode(child)),
  };
}

function createTodoId() {
  return `todo_${crypto.randomUUID()}`;
}

function getInlineText(node: EditorNode | undefined): string {
  if (!node) return '';
  if (node.type === 'text') return node.text ?? '';
  if (node.type === 'hard_break') return '\n';
  return (node.content ?? []).map((child) => getInlineText(child)).join('');
}

function getTaskItemText(node: EditorNode): string {
  return (node.content ?? [])
    .filter((child) => child.type !== 'task_list')
    .map((child) => getInlineText(child))
    .join('')
    .trim();
}

function normalizeNode(node: EditorNode, seen: Set<string>, createId: () => string): EditorNode {
  const normalized = cloneNode(node);
  normalized.content = normalized.content?.map((child) => normalizeNode(child, seen, createId));

  if (normalized.type !== 'task_item') {
    return normalized;
  }

  const attrs = normalized.attrs ? { ...normalized.attrs } : {};
  const currentId = typeof attrs.todoId === 'string' && attrs.todoId.length > 0 ? attrs.todoId : null;

  if (!currentId || seen.has(currentId)) {
    let nextId = createId();
    while (seen.has(nextId)) {
      nextId = createId();
    }
    attrs.todoId = nextId;
    seen.add(nextId);
  } else {
    seen.add(currentId);
  }

  normalized.attrs = attrs;
  return normalized;
}

export function normalizeInlineTodoIds(document: EditorDocument, options?: NormalizeOptions): EditorDocument {
  const createIdFn = options?.createId ?? createTodoId;
  const seen = new Set<string>();
  return {
    schemaVersion: 2,
    doc: normalizeNode(document.doc, seen, createIdFn),
  };
}

function extractNodeTodos(node: EditorNode, depth: number, items: InlineTodoItem[]) {
  if (node.type === 'task_item') {
    const todoId = typeof node.attrs?.todoId === 'string' ? node.attrs.todoId : null;
    if (todoId) {
      items.push({
        id: todoId,
        text: getTaskItemText(node),
        done: node.attrs?.checked === true,
        depth,
        order: items.length,
      });
    }
  }

  const nextDepth = node.type === 'task_item' ? depth + 1 : depth;
  for (const child of node.content ?? []) {
    extractNodeTodos(child, nextDepth, items);
  }
}

export function extractInlineTodos(document: EditorDocument): InlineTodoItem[] {
  const items: InlineTodoItem[] = [];
  extractNodeTodos(document.doc, 0, items);
  return items;
}

type ToggleResult = {
  document: EditorDocument;
  done: boolean;
};

function toggleNode(node: EditorNode, todoId: string): ToggleResult | null {
  const normalized = cloneNode(node);

  if (normalized.type === 'task_item' && normalized.attrs?.todoId === todoId) {
    const attrs = normalized.attrs ? { ...normalized.attrs } : {};
    const done = attrs.checked !== true;
    attrs.checked = done;
    normalized.attrs = attrs;
    return {
      document: { schemaVersion: 2, doc: normalized },
      done,
    };
  }

  for (let index = 0; index < (normalized.content?.length ?? 0); index += 1) {
    const child = normalized.content?.[index];
    if (!child) continue;
    const toggled = toggleNode(child, todoId);
    if (!toggled) continue;
    normalized.content![index] = toggled.document.doc;
    return {
      document: { schemaVersion: 2, doc: normalized },
      done: toggled.done,
    };
  }

  return null;
}

export function toggleInlineTodoInDocument(document: EditorDocument, todoId: string): ToggleResult | null {
  const toggled = toggleNode(document.doc, todoId);
  if (!toggled) return null;
  return {
    document: {
      schemaVersion: 2,
      doc: toggled.document.doc,
    },
    done: toggled.done,
  };
}
