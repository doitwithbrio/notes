import { describe, expect, it } from 'vitest';

import type { EditorDocument } from './schema.js';
import { extractInlineTodos, normalizeInlineTodoIds, toggleInlineTodoInDocument } from './inline-todos.js';

function makeTaskDocument(): EditorDocument {
  return {
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
              content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Ship alpha' }] }],
            },
            {
              type: 'task_item',
              attrs: { checked: true, todoId: 'keep-me' },
              content: [
                { type: 'paragraph', content: [{ type: 'text', text: 'Review beta' }] },
                {
                  type: 'task_list',
                  content: [
                    {
                      type: 'task_item',
                      attrs: { checked: false, todoId: 'duplicate' },
                      content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Nested first' }] }],
                    },
                    {
                      type: 'task_item',
                      attrs: { checked: false, todoId: 'duplicate' },
                      content: [
                        { type: 'paragraph', content: [{ type: 'text', text: 'Nested second' }] },
                        {
                          type: 'task_list',
                          content: [{
                            type: 'task_item',
                            attrs: { checked: true },
                            content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Deep task' }] }],
                          }],
                        },
                      ],
                    },
                  ],
                },
              ],
            },
          ],
        },
      ],
    },
  };
}

describe('inline todo helpers', () => {
  it('assigns ids to missing task items and preserves existing unique ids', () => {
    let nextId = 1;
    const normalized = normalizeInlineTodoIds(makeTaskDocument(), {
      createId: () => `generated-${nextId++}`,
    });

    const todos = extractInlineTodos(normalized);

    expect(todos.map((todo) => todo.id)).toEqual([
      'generated-1',
      'keep-me',
      'duplicate',
      'generated-3',
      'generated-2',
    ]);
  });

  it('preserves ids when checkbox state changes', () => {
    let nextId = 1;
    const normalized = normalizeInlineTodoIds(makeTaskDocument(), {
      createId: () => `generated-${nextId++}`,
    });

    const toggled: EditorDocument = {
      schemaVersion: 2,
      doc: {
        ...normalized.doc,
        content: [
          {
            ...(normalized.doc.content?.[0] ?? { type: 'task_list' }),
            content: (normalized.doc.content?.[0]?.content ?? []).map((node, index) => index === 0
              ? { ...node, attrs: { ...node.attrs, checked: true } }
              : node),
          },
        ],
      },
    };

    const todos = extractInlineTodos(toggled);

    expect(todos[0]).toMatchObject({ id: 'generated-1', done: true });
  });

  it('extracts nested task items in document order with depth', () => {
    let nextId = 1;
    const normalized = normalizeInlineTodoIds(makeTaskDocument(), {
      createId: () => `generated-${nextId++}`,
    });

    expect(extractInlineTodos(normalized)).toEqual([
      { id: 'generated-1', text: 'Ship alpha', done: false, order: 0, depth: 0 },
      { id: 'keep-me', text: 'Review beta', done: true, order: 1, depth: 0 },
      { id: 'duplicate', text: 'Nested first', done: false, order: 2, depth: 1 },
      { id: 'generated-3', text: 'Nested second', done: false, order: 3, depth: 1 },
      { id: 'generated-2', text: 'Deep task', done: true, order: 4, depth: 2 },
    ]);
  });

  it('returns no inline todos for plain paragraph documents', () => {
    expect(extractInlineTodos({
      schemaVersion: 2,
      doc: {
        type: 'doc',
        content: [{ type: 'paragraph', content: [{ type: 'text', text: '- [ ] literal markdown' }] }],
      },
    })).toEqual([]);
  });

  it('toggles only the targeted inline todo by todoId', () => {
    let nextId = 1;
    const normalized = normalizeInlineTodoIds(makeTaskDocument(), {
      createId: () => `generated-${nextId++}`,
    });

    const toggled = toggleInlineTodoInDocument(normalized, 'duplicate');

    expect(toggled?.done).toBe(true);
    expect(extractInlineTodos(toggled!.document)).toEqual([
      { id: 'generated-1', text: 'Ship alpha', done: false, order: 0, depth: 0 },
      { id: 'keep-me', text: 'Review beta', done: true, order: 1, depth: 0 },
      { id: 'duplicate', text: 'Nested first', done: true, order: 2, depth: 1 },
      { id: 'generated-3', text: 'Nested second', done: false, order: 3, depth: 1 },
      { id: 'generated-2', text: 'Deep task', done: true, order: 4, depth: 2 },
    ]);
  });

  it('returns null when the target inline todo does not exist', () => {
    expect(toggleInlineTodoInDocument(makeTaskDocument(), 'missing-id')).toBeNull();
  });
});
