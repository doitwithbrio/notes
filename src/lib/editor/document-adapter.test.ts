import * as Automerge from '@automerge/automerge';
import { describe, expect, it } from 'vitest';

import { loadEditorDocument } from './document-adapter.js';
import { getVisibleTextFromDocument } from './schema.js';

function makeLegacyBinary(text: string) {
  return Automerge.save(Automerge.from({ schemaVersion: 1, text })) as Uint8Array;
}

function makeGraphBinary() {
  return Automerge.save(Automerge.from({
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content: [
        {
          type: 'heading',
          attrs: { level: 1 },
          content: [{ type: 'text', text: 'hello' }],
        },
        {
          type: 'paragraph',
          content: [{ type: 'text', text: 'world' }],
        },
      ],
    },
  })) as Uint8Array;
}

describe('document adapter', () => {
  it('opens a legacy text doc through the new adapter without mutating the stored bytes', () => {
    const binary = makeLegacyBinary('first line\n\nsecond line');

    const loaded = loadEditorDocument(binary);

    expect(loaded.sourceSchema).toBe('legacy-text');
    expect(loaded.needsMigration).toBe(true);
    expect(loaded.visibleText).toBe('first line\n\nsecond line');
    expect(getVisibleTextFromDocument(loaded.editorDocument)).toBe('first line\n\nsecond line');
    expect(Array.from(Automerge.save(loaded.storageDoc))).toEqual(Array.from(binary));
  });

  it('keeps markdown-looking literals as visible text when adapting legacy docs', () => {
    const binary = makeLegacyBinary('# title\n\n- [ ] task\n\n**bold** literal');

    const loaded = loadEditorDocument(binary);

    expect(loaded.visibleText).toBe('# title\n\n- [ ] task\n\n**bold** literal');
    expect(getVisibleTextFromDocument(loaded.editorDocument)).toBe('# title\n\n- [ ] task\n\n**bold** literal');
  });

  it('opens a canonical graph doc without requesting migration', () => {
    const loaded = loadEditorDocument(makeGraphBinary());

    expect(loaded.sourceSchema).toBe('graph-v2');
    expect(loaded.needsMigration).toBe(false);
    expect(loaded.visibleText).toBe('hello\n\nworld');
  });

  it('normalizes missing inline task ids when loading graph docs for editing', () => {
    const binary = Automerge.save(Automerge.from({
      schemaVersion: 2,
      doc: {
        type: 'doc',
        content: [{
          type: 'task_list',
          content: [{
            type: 'task_item',
            attrs: { checked: false },
            content: [{ type: 'paragraph', content: [{ type: 'text', text: 'todo' }] }],
          }],
        }],
      },
    })) as Uint8Array;

    const loaded = loadEditorDocument(binary);
    const taskItem = loaded.editorDocument.doc.content?.[0]?.content?.[0];

    expect(loaded.sourceSchema).toBe('graph-v2');
    expect(typeof taskItem?.attrs?.todoId).toBe('string');
    expect(taskItem?.attrs?.todoId).toBeTruthy();
  });

  it('rejects malformed v2 docs without silently treating them as legacy text', () => {
    const binary = Automerge.save(Automerge.from({
      schemaVersion: 2,
      doc: { nope: true },
    })) as Uint8Array;

    expect(() => loadEditorDocument(binary)).toThrow('Stored v2 note is missing a valid root document node');
  });
});
