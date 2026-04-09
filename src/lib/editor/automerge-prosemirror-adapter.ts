import * as Automerge from '@automerge/automerge';
import {
  init as initAutomergeProsemirror,
  pmDocFromSpans,
  pmNodeToSpans,
  SchemaAdapter,
  type DocHandle,
} from '@automerge/prosemirror';
import { baseKeymap } from '@tiptap/pm/commands';
import { dropCursor } from '@tiptap/pm/dropcursor';
import { gapCursor } from '@tiptap/pm/gapcursor';
import { keymap } from '@tiptap/pm/keymap';
import { EditorState, Plugin, PluginKey, Selection } from '@tiptap/pm/state';
import { Decoration, DecorationSet, EditorView } from '@tiptap/pm/view';

import { tauriApi } from '../api/tauri.js';
import type { CursorPosition } from '../types/index.js';
import { loadEditorDocument, type StoredNoteDoc } from './document-adapter.js';
import { normalizeInlineTodoIds } from './inline-todos.js';
import { getPreferredPasteText, parsePlainTextPaste } from './paste-normalization.js';
import { getVisibleTextFromDocument, type EditorDocument, type EditorNode } from './schema.js';

const A = Automerge.next;
const RICH_TEXT_PATH = ['richText'];

export type NotesEditor = {
  readonly state: EditorState;
  readonly view: EditorView;
  getJSON(): EditorNode;
  setEditable(editable: boolean): void;
  destroy(): void;
  commands: {
    setContent(content: EditorNode, options?: { emitUpdate?: boolean }): boolean;
    insertContent(content: EditorNode[] | EditorNode): boolean;
  };
};

export type AdapterChange = {
  source: 'local' | 'remote';
  doc: Automerge.Doc<StoredNoteDoc>;
  document: EditorDocument;
  text: string;
};

export type AutomergeEditorAdapter = {
  attach(doc: Automerge.Doc<StoredNoteDoc>, editable: boolean): Automerge.Doc<StoredNoteDoc>;
  detach(): void;
  applyIncremental(bytes: Uint8Array): Automerge.Doc<StoredNoteDoc> | null;
  replaceSnapshot(bytes: Uint8Array, editable: boolean): Automerge.Doc<StoredNoteDoc>;
  getCurrentDoc(): Automerge.Doc<StoredNoteDoc> | null;
  getEditor(): NotesEditor | null;
  updateRemotePresence(cursors: CursorPosition[]): void;
};

export type AutomergeEditorAdapterOptions = {
  onChange?: (change: AdapterChange) => void;
  onSelectionChange?: (cursorPos: number | null, selection: [number, number] | null) => void;
  onFocusChange?: (focused: boolean) => void;
  getProjectId?: () => string | null;
};

export function updateEditorRemotePresence(editor: NotesEditor, cursors: CursorPosition[]) {
  editor.view.dispatch(editor.state.tr.setMeta(remotePresencePluginKey, cursors));
}

type ChangeListener<T> = (payload: {
  handle: DocHandle<T>;
  doc: Automerge.Doc<T>;
  patches: Automerge.Patch[];
  patchInfo: Automerge.PatchInfo<T>;
}) => void;

type BridgeDoc = StoredNoteDoc & {
  richText?: unknown;
};

const remotePresencePluginKey = new PluginKey<DecorationSet>('remote-presence');
const blobUrlCache = new Map<string, string>();

function buildRemoteDecorations(doc: Parameters<typeof DecorationSet.create>[0], cursors: CursorPosition[]) {
  const decorations: Decoration[] = [];
  for (const cursor of cursors) {
    const from = Math.max(0, Math.min(cursor.from, doc.content.size));
    const to = Math.max(from, Math.min(cursor.to, doc.content.size));
    if (to > from) {
      decorations.push(
        Decoration.inline(from, to, {
          class: 'remote-selection',
          style: `--remote-selection-color: ${cursor.cursorColor};`,
        }),
      );
    }

    decorations.push(
      Decoration.widget(to, () => {
        const caret = document.createElement('span');
        caret.className = 'remote-caret';
        caret.style.setProperty('--remote-caret-color', cursor.cursorColor);

        const label = document.createElement('span');
        label.className = 'remote-caret-label';
        label.textContent = cursor.alias;
        label.style.setProperty('--remote-caret-color', cursor.cursorColor);

        caret.appendChild(label);
        return caret;
      }, { side: 1 }),
    );
  }
  return DecorationSet.create(doc, decorations);
}

const RemotePresence = new Plugin<DecorationSet>({
  key: remotePresencePluginKey,
  state: {
    init: (_, state) => DecorationSet.create(state.doc, []),
    apply: (tr, oldState) => {
      const next = tr.getMeta(remotePresencePluginKey) as CursorPosition[] | undefined;
      if (next) {
        return buildRemoteDecorations(tr.doc, next);
      }
      return oldState.map(tr.mapping, tr.doc);
    },
  },
  props: {
    decorations(state) {
      return this.getState(state);
    },
  },
});

const schemaAdapter = new SchemaAdapter({
  nodes: {
    doc: { content: 'block+' },
    paragraph: {
      automerge: { block: 'paragraph' },
      content: 'inline*',
      group: 'block',
      parseDOM: [{ tag: 'p' }],
      toDOM() {
        return ['p', 0];
      },
    },
    unknownBlock: {
      automerge: { unknownBlock: true },
      group: 'block',
      content: 'block+',
      parseDOM: [{ tag: 'div', attrs: { 'data-unknown-block': 'true' } }],
      toDOM() {
        return ['div', { 'data-unknown-block': 'true' }, 0];
      },
    },
    blockquote: {
      automerge: { block: 'blockquote' },
      content: 'block+',
      group: 'block',
      parseDOM: [{ tag: 'blockquote' }],
      toDOM() {
        return ['blockquote', 0];
      },
    },
    heading: {
      automerge: {
        block: 'heading',
        attrParsers: {
          fromAutomerge: (block) => ({ level: block.attrs.level ?? 1 }),
          fromProsemirror: (node) => ({ level: node.attrs.level ?? 1 }),
        },
      },
      attrs: { level: { default: 1 } },
      content: 'inline*',
      group: 'block',
      parseDOM: [
        { tag: 'h1', attrs: { level: 1 } },
        { tag: 'h2', attrs: { level: 2 } },
        { tag: 'h3', attrs: { level: 3 } },
        { tag: 'h4', attrs: { level: 4 } },
        { tag: 'h5', attrs: { level: 5 } },
        { tag: 'h6', attrs: { level: 6 } },
      ],
      toDOM(node) {
        return [`h${node.attrs.level ?? 1}`, 0];
      },
    },
    bulletList: {
      content: 'listItem+',
      group: 'block',
      parseDOM: [{ tag: 'ul' }],
      toDOM() {
        return ['ul', 0];
      },
    },
    orderedList: {
      attrs: { order: { default: 1 } },
      content: 'listItem+',
      group: 'block',
      parseDOM: [{
        tag: 'ol',
        getAttrs(dom) {
          return {
            order: dom instanceof HTMLOListElement && dom.hasAttribute('start')
              ? Number(dom.getAttribute('start'))
              : 1,
          };
        },
      }],
      toDOM(node) {
        return node.attrs.order === 1 ? ['ol', 0] : ['ol', { start: node.attrs.order }, 0];
      },
    },
    listItem: {
      automerge: {
        block: {
          within: {
            bulletList: 'unordered-list-item',
            orderedList: 'ordered-list-item',
          },
        },
      },
      content: 'paragraph block*',
      parseDOM: [{ tag: 'li' }],
      toDOM() {
        return ['li', 0];
      },
    },
    taskList: {
      automerge: { block: 'task-list' },
      content: 'taskItem+',
      group: 'block',
      parseDOM: [{ tag: 'ul[data-type="taskList"]' }],
      toDOM() {
        return ['ul', { 'data-type': 'taskList' }, 0];
      },
    },
    taskItem: {
      automerge: {
        block: {
          within: {
            taskList: 'task-item',
          },
        },
        attrParsers: {
          fromAutomerge: (block) => ({
            checked: block.attrs.checked === true,
            todoId: typeof block.attrs.todoId === 'string' ? block.attrs.todoId : null,
          }),
          fromProsemirror: (node) => ({
            checked: node.attrs.checked === true,
            ...(typeof node.attrs.todoId === 'string' ? { todoId: node.attrs.todoId } : {}),
          }),
        },
      },
      attrs: {
        checked: { default: false },
        todoId: { default: null },
      },
      content: 'paragraph block*',
      parseDOM: [{
        tag: 'li[data-type="taskItem"]',
        getAttrs(dom) {
          if (!(dom instanceof HTMLElement)) return { checked: false, todoId: null };
          return {
            checked: dom.getAttribute('data-checked') === 'true',
            todoId: dom.getAttribute('data-todo-id'),
          };
        },
      }],
      toDOM(node) {
        return ['li', {
          'data-type': 'taskItem',
          'data-checked': String(node.attrs.checked === true),
          'data-todo-id': node.attrs.todoId ?? '',
        }, 0];
      },
    },
    codeBlock: {
      automerge: { block: 'code-block' },
      content: 'text*',
      marks: '',
      group: 'block',
      code: true,
      parseDOM: [{ tag: 'pre', preserveWhitespace: 'full' }],
      toDOM() {
        return ['pre', ['code', 0]];
      },
    },
    horizontalRule: {
      automerge: { block: 'horizontal-rule', isEmbed: true },
      group: 'block',
      parseDOM: [{ tag: 'hr' }],
      toDOM() {
        return ['hr'];
      },
    },
    hardBreak: {
      automerge: { block: 'hard-break', isEmbed: true },
      inline: true,
      group: 'inline',
      selectable: false,
      parseDOM: [{ tag: 'br' }],
      toDOM() {
        return ['br'];
      },
    },
    image: {
      automerge: {
        block: 'image',
        isEmbed: true,
        attrParsers: {
          fromAutomerge: (block) => ({
            src: typeof block.attrs.src === 'string' ? block.attrs.src : '',
            alt: typeof block.attrs.alt === 'string' ? block.attrs.alt : null,
            title: typeof block.attrs.title === 'string' ? block.attrs.title : null,
          }),
          fromProsemirror: (node) => ({
            src: String(node.attrs.src ?? ''),
            ...(node.attrs.alt ? { alt: String(node.attrs.alt) } : {}),
            ...(node.attrs.title ? { title: String(node.attrs.title) } : {}),
          }),
        },
      },
      inline: true,
      group: 'inline',
      draggable: true,
      attrs: {
        src: { default: '' },
        alt: { default: null },
        title: { default: null },
      },
      parseDOM: [{
        tag: 'img[src]',
        getAttrs(dom) {
          if (!(dom instanceof HTMLImageElement)) return false;
          return {
            src: dom.getAttribute('src') ?? '',
            alt: dom.getAttribute('alt'),
            title: dom.getAttribute('title'),
          };
        },
      }],
      toDOM(node) {
        return ['img', node.attrs];
      },
    },
    text: { group: 'inline' },
  },
  marks: {
    bold: {
      parseDOM: [{ tag: 'strong' }, { tag: 'b' }],
      toDOM() {
        return ['strong', 0];
      },
      automerge: { markName: 'bold' },
    },
    italic: {
      parseDOM: [{ tag: 'em' }, { tag: 'i' }],
      toDOM() {
        return ['em', 0];
      },
      automerge: { markName: 'italic' },
    },
    strike: {
      parseDOM: [{ tag: 's' }, { tag: 'del' }, { tag: 'strike' }],
      toDOM() {
        return ['s', 0];
      },
      automerge: { markName: 'strike' },
    },
    code: {
      parseDOM: [{ tag: 'code' }],
      toDOM() {
        return ['code', 0];
      },
      automerge: { markName: 'code' },
    },
    link: {
      attrs: {
        href: {},
        title: { default: null },
      },
      inclusive: false,
      parseDOM: [{
        tag: 'a[href]',
        getAttrs(dom) {
          if (!(dom instanceof HTMLAnchorElement)) return false;
          return {
            href: dom.getAttribute('href') ?? '',
            title: dom.getAttribute('title'),
          };
        },
      }],
      toDOM(node) {
        return ['a', { href: node.attrs.href, title: node.attrs.title }, 0];
      },
      automerge: {
        markName: 'link',
        parsers: {
          fromAutomerge: (mark) => {
            if (typeof mark === 'string') {
              try {
                const value = JSON.parse(mark) as { href?: string; title?: string | null };
                return {
                  href: value.href ?? '',
                  title: value.title ?? null,
                };
              } catch {
                return { href: '', title: null };
              }
            }
            return { href: '', title: null };
          },
          fromProsemirror: (mark) => JSON.stringify({
            href: mark.attrs.href,
            title: mark.attrs.title,
          }),
        },
      },
    },
  },
});

class MemoryDocHandle<T> implements DocHandle<T> {
  private currentDoc: Automerge.Doc<T>;
  private listeners = new Set<ChangeListener<T>>();
  private readonly beforeCommit?: (doc: T) => void;

  constructor(doc: Automerge.Doc<T>, options?: { beforeCommit?: (doc: T) => void }) {
    this.currentDoc = doc;
    this.beforeCommit = options?.beforeCommit;
  }

  doc() {
    return this.currentDoc as T;
  }

  setDoc(doc: Automerge.Doc<T>) {
    this.currentDoc = doc;
  }

  change(fn: (doc: T) => void) {
    let emittedPatches: Automerge.Patch[] = [];
    let emittedInfo: Automerge.PatchInfo<T> | null = null;
    const nextDoc = Automerge.change(this.currentDoc, {
      patchCallback: (patches, patchInfo) => {
        emittedPatches = patches;
        emittedInfo = patchInfo;
      },
    }, (doc) => {
      fn(doc);
      this.beforeCommit?.(doc);
    });
    this.currentDoc = nextDoc;
    if (emittedInfo != null) {
      this.emit({
        handle: this,
        doc: nextDoc,
        patches: emittedPatches,
        patchInfo: emittedInfo,
      });
    }
  }

  applyIncremental(bytes: Uint8Array) {
    let emittedPatches: Automerge.Patch[] = [];
    let emittedInfo: Automerge.PatchInfo<T> | null = null;
    const nextDoc = Automerge.loadIncremental(this.currentDoc, bytes, {
      patchCallback: (patches, patchInfo) => {
        emittedPatches = patches;
        emittedInfo = patchInfo;
      },
    });
    this.currentDoc = nextDoc;
    if (emittedInfo != null) {
      this.emit({
        handle: this,
        doc: nextDoc,
        patches: emittedPatches,
        patchInfo: emittedInfo,
      });
    }
    return nextDoc;
  }

  on(event: 'change', callback: ChangeListener<T>) {
    if (event === 'change') {
      this.listeners.add(callback);
    }
  }

  off(event: 'change', callback: ChangeListener<T>) {
    if (event === 'change') {
      this.listeners.delete(callback);
    }
  }

  private emit(payload: Parameters<ChangeListener<T>>[0]) {
    for (const listener of this.listeners) {
      listener(payload);
    }
  }
}

function mapNodeTypeToPm(type: string) {
  switch (type) {
    case 'bullet_list': return 'bulletList';
    case 'ordered_list': return 'orderedList';
    case 'list_item': return 'listItem';
    case 'task_list': return 'taskList';
    case 'task_item': return 'taskItem';
    case 'code_block': return 'codeBlock';
    case 'horizontal_rule': return 'horizontalRule';
    case 'hard_break': return 'hardBreak';
    default: return type;
  }
}

function mapNodeTypeFromPm(type: string) {
  switch (type) {
    case 'bulletList': return 'bullet_list';
    case 'orderedList': return 'ordered_list';
    case 'listItem': return 'list_item';
    case 'taskList': return 'task_list';
    case 'taskItem': return 'task_item';
    case 'codeBlock': return 'code_block';
    case 'horizontalRule': return 'horizontal_rule';
    case 'hardBreak': return 'hard_break';
    default: return type;
  }
}

function sanitizeForAutomerge<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function compatibilityNodeToPm(node: EditorNode): EditorNode {
  return {
    ...node,
    type: mapNodeTypeToPm(node.type),
    marks: node.marks?.map((mark) => ({ ...mark })),
    content: node.content?.map(compatibilityNodeToPm),
  };
}

function pmNodeToCompatibility(node: EditorNode): EditorNode {
  return {
    ...node,
    type: mapNodeTypeFromPm(node.type),
    marks: node.marks?.map((mark) => ({ ...mark })),
    content: node.content?.map(pmNodeToCompatibility),
  };
}

function documentFromPmJson(json: EditorNode): EditorDocument {
  return normalizeInlineTodoIds({
    schemaVersion: 2,
    doc: pmNodeToCompatibility(json),
  });
}

function compatDocumentToPmJson(document: EditorDocument): EditorNode {
  return compatibilityNodeToPm(document.doc);
}

function readShadowDocument(doc: Automerge.Doc<StoredNoteDoc>) {
  if (doc.schemaVersion === 2 && doc.doc && typeof doc.doc === 'object') {
    const compatDocument = normalizeInlineTodoIds({
      schemaVersion: 2,
      doc: sanitizeForAutomerge(doc.doc as EditorNode),
    });
    return {
      document: compatDocument,
      text: getVisibleTextFromDocument(compatDocument),
    };
  }

  const loaded = loadEditorDocument(Automerge.save(doc) as Uint8Array);
  return {
    document: loaded.editorDocument,
    text: loaded.visibleText,
  };
}

function syncCompatibilityFields(doc: BridgeDoc) {
  const spans = A.spans(doc, RICH_TEXT_PATH);
  const pmDoc = pmDocFromSpans(schemaAdapter, spans);
  const compatDocument = documentFromPmJson(pmDoc.toJSON() as EditorNode);
  const visibleText = getVisibleTextFromDocument(compatDocument);
  doc.schemaVersion = 2;
  doc.doc = sanitizeForAutomerge(compatDocument.doc);
  doc.text = visibleText;
}

function ensureRichTextDoc(doc: Automerge.Doc<StoredNoteDoc>) {
  if ((doc as BridgeDoc).richText !== undefined) {
    return doc;
  }

  const shadow = readShadowDocument(doc);
  return Automerge.change(doc, (draft: BridgeDoc) => {
    draft.richText = '';
    const pmDoc = schemaAdapter.schema.nodeFromJSON(compatDocumentToPmJson(shadow.document));
    A.updateSpans(draft, RICH_TEXT_PATH, pmNodeToSpans(schemaAdapter, pmDoc), schemaAdapter.updateSpansConfig());
    draft.schemaVersion = 2;
    draft.doc = sanitizeForAutomerge(shadow.document.doc);
    draft.text = shadow.text;
  });
}

function pmDocFromCompatibilityContent(content: EditorNode[] | EditorNode) {
  const compatContent = Array.isArray(content) ? content : [content];
  return schemaAdapter.schema.nodeFromJSON({
    type: 'doc',
    content: compatContent.map(compatibilityNodeToPm),
  });
}

async function resolveBlobImageUrl(projectId: string, hash: string) {
  const cached = blobUrlCache.get(hash);
  if (cached) return cached;

  const availability = await tauriApi.ensureBlobAvailable(projectId, hash);
  if (!availability.available) return null;

  const bytes = await tauriApi.getImage(hash);
  const url = URL.createObjectURL(new Blob([bytes]));
  blobUrlCache.set(hash, url);
  return url;
}

function parseBlobHash(src: string | null | undefined) {
  if (!src || !src.startsWith('blob:')) return null;
  return src.slice('blob:'.length);
}

class BlobImageNodeView {
  dom = document.createElement('span');
  private image = document.createElement('img');
  private status = document.createElement('span');
  private node: { attrs: Record<string, unknown> };
  private readonly getProjectId: () => string | null;
  private disposed = false;

  constructor(node: { attrs: Record<string, unknown> }, getProjectId: () => string | null) {
    this.node = node;
    this.getProjectId = getProjectId;
    this.dom.className = 'blob-image-node';
    this.dom.dataset.testid = 'blob-image-node';
    this.status.className = 'blob-image-status';
    this.image.className = 'blob-image';
    this.dom.append(this.image, this.status);
    void this.render();
  }

  update(node: { attrs: Record<string, unknown> }) {
    this.node = node;
    void this.render();
    return true;
  }

  ignoreMutation() {
    return true;
  }

  destroy() {
    this.disposed = true;
  }

  private async render() {
    const src = typeof this.node.attrs.src === 'string' ? this.node.attrs.src : '';
    const alt = typeof this.node.attrs.alt === 'string' ? this.node.attrs.alt : '';
    const title = typeof this.node.attrs.title === 'string' ? this.node.attrs.title : '';
    this.image.alt = alt;
    this.image.title = title;

    const blobHash = parseBlobHash(src);
    if (!blobHash) {
      this.image.src = src;
      this.status.textContent = '';
      this.dom.dataset.state = 'ready';
      return;
    }

    const projectId = this.getProjectId();
    if (!projectId) {
      this.image.removeAttribute('src');
      this.status.textContent = 'image unavailable';
      this.dom.dataset.state = 'missing';
      return;
    }

    this.image.removeAttribute('src');
    this.status.textContent = 'loading image...';
    this.dom.dataset.state = 'loading';

    const resolved = await resolveBlobImageUrl(projectId, blobHash).catch(() => null);
    if (this.disposed) return;

    if (!resolved) {
      this.status.textContent = 'image unavailable';
      this.dom.dataset.state = 'missing';
      return;
    }

    this.image.src = resolved;
    this.status.textContent = '';
    this.dom.dataset.state = 'ready';
  }
}

async function maybeImportImageFile(
  view: EditorView,
  file: File,
  getProjectId: () => string | null,
  pos?: number,
) {
  if (!file.type.startsWith('image/')) return false;
  const projectId = getProjectId();
  if (!projectId) return false;

  const bytes = new Uint8Array(await file.arrayBuffer());
  const meta = await tauriApi.importImage(projectId, bytes, file.name || 'image');
  const imageType = schemaAdapter.schema.nodes.image;
  if (!imageType) return false;
  const imageNode = imageType.create({
    src: `blob:${meta.hash}`,
    alt: file.name || '',
    title: file.name || '',
  });
  const tr = pos != null
    ? view.state.tr.insert(pos, imageNode)
    : view.state.tr.replaceSelectionWith(imageNode);
  view.dispatch(tr.scrollIntoView());
  return true;
}

function createNotesEditor(
  element: HTMLElement,
  docHandle: MemoryDocHandle<BridgeDoc>,
  editableInitial: boolean,
  onSelectionChange?: (cursorPos: number | null, selection: [number, number] | null) => void,
  onFocusChange?: (focused: boolean) => void,
  getProjectId?: () => string | null,
) {
  const { pmDoc, plugin } = initAutomergeProsemirror(docHandle, RICH_TEXT_PATH, {
    schemaAdapter,
  });

  let editable = editableInitial;
  let suppressDocEvent = false;

  const view = new EditorView(element, {
    state: EditorState.create({
      schema: schemaAdapter.schema,
      doc: pmDoc,
      plugins: [
        keymap(baseKeymap),
        dropCursor({ color: 'var(--accent)', width: 2 }),
        gapCursor(),
        RemotePresence,
        plugin,
      ],
    }),
    editable: () => editable,
    nodeViews: {
      image(node) {
        return new BlobImageNodeView(node, () => getProjectId?.() ?? null);
      },
    },
    dispatchTransaction(transaction) {
      const nextState = view.state.apply(transaction);
      view.updateState(nextState);

      if (!suppressDocEvent) {
        const { from, to } = view.state.selection;
        onSelectionChange?.(to, [from, to]);
      }
    },
    handleDOMEvents: {
      focus: () => {
        onFocusChange?.(true);
        return false;
      },
      blur: () => {
        onFocusChange?.(false);
        onSelectionChange?.(null, null);
        return false;
      },
    },
    handlePaste: (_view, event) => {
      const files = Array.from(event.clipboardData?.files ?? []);
      const imageFile = files.find((file) => file.type.startsWith('image/'));
      if (imageFile) {
        event.preventDefault();
        void maybeImportImageFile(view, imageFile, () => getProjectId?.() ?? null);
        return true;
      }

      const plainText = getPreferredPasteText(event.clipboardData);
      if (!plainText) return false;

      event.preventDefault();
      const parsed = parsePlainTextPaste(plainText);
      const pmParsed = pmDocFromCompatibilityContent(parsed.doc.content ?? []);
      view.dispatch(view.state.tr.replaceSelection(pmParsed.slice(0)).scrollIntoView());
      return true;
    },
    handleDrop: (_view, event) => {
      const files = Array.from(event.dataTransfer?.files ?? []);
      const imageFile = files.find((file) => file.type.startsWith('image/'));
      if (!imageFile) return false;

      event.preventDefault();
      const coords = view.posAtCoords({ left: event.clientX, top: event.clientY });
      void maybeImportImageFile(view, imageFile, () => getProjectId?.() ?? null, coords?.pos);
      return true;
    },
  });
  view.dom.classList.add('editor-content');

  const editor: NotesEditor = {
    get state() {
      return view.state;
    },
    view,
    getJSON() {
      return pmNodeToCompatibility(view.state.doc.toJSON() as EditorNode);
    },
    setEditable(nextEditable: boolean) {
      editable = nextEditable;
      view.setProps({
        editable: () => editable,
      });
    },
    destroy() {
      view.destroy();
    },
    commands: {
      setContent(content: EditorNode, options?: { emitUpdate?: boolean }) {
        const nextDoc = schemaAdapter.schema.nodeFromJSON(compatibilityNodeToPm(content));
        suppressDocEvent = options?.emitUpdate === false;
        const nextState = EditorState.create({
          schema: schemaAdapter.schema,
          doc: nextDoc,
          plugins: view.state.plugins,
          selection: Selection.atStart(nextDoc),
        });
        view.updateState(nextState);
        suppressDocEvent = false;
        return true;
      },
      insertContent(content: EditorNode[] | EditorNode) {
        const nextDoc = pmDocFromCompatibilityContent(content);
        const tr = view.state.tr.replaceSelection(nextDoc.slice(0));
        view.dispatch(tr.scrollIntoView());
        return true;
      },
    },
  };

  return editor;
}

export function createAutomergeProsemirrorAdapter(
  element: HTMLElement,
  options: AutomergeEditorAdapterOptions = {},
): AutomergeEditorAdapter {
  let currentDoc: Automerge.Doc<StoredNoteDoc> | null = null;
  let currentEditor: NotesEditor | null = null;

  const docHandle = new MemoryDocHandle<BridgeDoc>(Automerge.from<BridgeDoc>({ schemaVersion: 2 }), {
    beforeCommit: syncCompatibilityFields,
  });

  docHandle.on('change', ({ doc, patchInfo }) => {
    const shadow = readShadowDocument(doc as Automerge.Doc<StoredNoteDoc>);
    options.onChange?.({
      source: patchInfo.source === 'change' ? 'local' : 'remote',
      doc: doc as Automerge.Doc<StoredNoteDoc>,
      document: shadow.document,
      text: shadow.text,
    });
  });

  function mountEditor(editable: boolean) {
    currentEditor?.destroy();
    element.innerHTML = '';
    currentEditor = createNotesEditor(
      element,
      docHandle,
      editable,
      options.onSelectionChange,
      options.onFocusChange,
      options.getProjectId,
    );
  }

  return {
    attach(doc, editable) {
      const ensured = ensureRichTextDoc(doc);
      currentDoc = ensured;
      docHandle.setDoc(ensured as Automerge.Doc<BridgeDoc>);
      mountEditor(editable);
      return ensured;
    },
    detach() {
      currentEditor?.destroy();
      currentEditor = null;
      currentDoc = null;
      element.innerHTML = '';
    },
    applyIncremental(bytes) {
      if (!currentDoc || bytes.length === 0) return currentDoc;
      currentDoc = docHandle.applyIncremental(bytes) as Automerge.Doc<StoredNoteDoc>;
      return currentDoc;
    },
    replaceSnapshot(bytes, editable) {
      const loaded = Automerge.load<StoredNoteDoc>(bytes);
      return this.attach(loaded, editable);
    },
    getCurrentDoc() {
      return currentDoc;
    },
    getEditor() {
      return currentEditor;
    },
    updateRemotePresence(cursors) {
      currentEditor?.view.dispatch(currentEditor.view.state.tr.setMeta(remotePresencePluginKey, cursors));
    },
  };
}
