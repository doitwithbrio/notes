import { describe, expect, it } from 'vitest';

import { parseExplicitTaskLine, parseExplicitTaskTrigger } from './task-markdown.js';

describe('task markdown parsing', () => {
  it('accepts only explicit dash-prefixed task triggers', () => {
    expect(parseExplicitTaskTrigger('- [ ] ')).toEqual({ checked: false });
    expect(parseExplicitTaskTrigger('- [x] ')).toEqual({ checked: true });
  });

  it('rejects literal or ambiguous checkbox text', () => {
    expect(parseExplicitTaskTrigger('[] ')).toBeNull();
    expect(parseExplicitTaskTrigger('[ ] ')).toBeNull();
    expect(parseExplicitTaskTrigger('alpha - [ ] ')).toBeNull();
  });

  it('parses explicit task lines but keeps literal text out', () => {
    expect(parseExplicitTaskLine('- [ ] ship it')).toEqual({ checked: false, text: 'ship it' });
    expect(parseExplicitTaskLine('- [x] done')).toEqual({ checked: true, text: 'done' });
    expect(parseExplicitTaskLine('[] literal')).toBeNull();
  });
});
