import { describe, expect, it } from 'vitest';

import { getPreferredPasteText, parsePlainTextPaste } from './paste-normalization.js';
import { getVisibleTextFromDocument } from './schema.js';

describe('paste normalization', () => {
  it('always prefers plain text over html clipboard data', () => {
    const text = getPreferredPasteText({
      getData(type: string) {
        if (type === 'text/plain') return '**hi**';
        if (type === 'text/html') return '<b>ignored</b>';
        return '';
      },
    });

    expect(text).toBe('**hi**');
  });

  it('parses inline markdown from pasted plain text', () => {
    const document = parsePlainTextPaste('**hi** and _there_ with `code`');
    const paragraph = document.doc.content?.[0];

    expect(paragraph?.type).toBe('paragraph');
    expect(paragraph?.content?.[0]?.marks?.[0]?.type).toBe('bold');
    expect(paragraph?.content?.[2]?.marks?.[0]?.type).toBe('italic');
    expect(paragraph?.content?.[4]?.marks?.[0]?.type).toBe('code');
  });

  it('parses line-based markdown for headings and tasks', () => {
    const document = parsePlainTextPaste('# Heading\n\n- [ ] task\n[] literal');

    expect(document.doc.content?.[0]?.type).toBe('heading');
    expect(document.doc.content?.[2]?.type).toBe('task_list');
    expect(document.doc.content?.[3]?.type).toBe('paragraph');
    expect(getVisibleTextFromDocument(document)).toBe('Heading\n\n- [ ] task\n\n[] literal');
  });

  it('keeps code-like bracket text literal', () => {
    const document = parsePlainTextPaste('{ foo: [bar] }');

    expect(getVisibleTextFromDocument(document)).toBe('{ foo: [bar] }');
    expect(document.doc.content?.[0]?.type).toBe('paragraph');
  });
});
