import { afterEach, describe, expect, it } from 'vitest';

import { createEditor } from './setup.js';

describe('editor paste handling', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('prefers plain text markdown over html clipboard payloads', () => {
    const element = document.createElement('div');
    document.body.appendChild(element);
    const editor = createEditor(
      element,
      { schemaVersion: 2, doc: { type: 'doc', content: [{ type: 'paragraph', content: [] }] } },
      () => undefined,
    );

    const event = new Event('paste', { bubbles: true, cancelable: true }) as ClipboardEvent;
    Object.defineProperty(event, 'clipboardData', {
      value: {
        getData(type: string) {
          if (type === 'text/plain') return '**hi**';
          if (type === 'text/html') return '<strong>ignored</strong>';
          return '';
        },
      },
    });

    editor.view.dom.dispatchEvent(event);

    const firstParagraph = editor.getJSON().content?.[0] as any;
    expect(firstParagraph?.content?.[0]?.text).toBe('hi');
    expect(firstParagraph?.content?.[0]?.marks?.[0]?.type).toBe('bold');

    editor.destroy();
  });
});
