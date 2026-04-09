import { describe, expect, it } from 'vitest';

import { createDocumentFromPlainText, getVisibleTextFromDocument, type EditorDocument } from './schema.js';

describe('editor schema text conversion', () => {
  it('round-trips punctuation and blank lines literally', () => {
    const text = 'first.\n\n\nsecond?\n\nthird!';

    const document = createDocumentFromPlainText(text);

    expect(getVisibleTextFromDocument(document)).toBe(text);
    expect(document.doc.content).toHaveLength(6);
  });

  it('preserves leading and trailing blank lines', () => {
    const text = '\nalpha\n\n';

    const document = createDocumentFromPlainText(text);

    expect(getVisibleTextFromDocument(document)).toBe(text);
  });

  it('preserves single newlines as soft line breaks', () => {
    const text = 'alpha\nbeta';

    const document = createDocumentFromPlainText(text);

    expect(getVisibleTextFromDocument(document)).toBe(text);
    expect(document.doc.content).toHaveLength(1);
    expect(document.doc.content?.[0]?.content?.[1]?.type).toBe('hard_break');
  });

  it('preserves literal bracket and brace text', () => {
    const text = '{} [] () [[link]] { foo: [bar] }';

    const document = createDocumentFromPlainText(text);

    expect(getVisibleTextFromDocument(document)).toBe(text);
  });

  it('exports task items with explicit markdown markers', () => {
    const document: EditorDocument = {
      schemaVersion: 2,
      doc: {
        type: 'doc',
        content: [
          {
            type: 'task_list',
            content: [
              {
                type: 'task_item',
                attrs: { checked: false },
                content: [{ type: 'paragraph', content: [{ type: 'text', text: 'todo item' }] }],
              },
              {
                type: 'task_item',
                attrs: { checked: true },
                content: [{ type: 'paragraph', content: [{ type: 'text', text: 'done item' }] }],
              },
            ],
          },
        ],
      },
    };

    expect(getVisibleTextFromDocument(document)).toBe('- [ ] todo item\n- [x] done item');
  });
});
