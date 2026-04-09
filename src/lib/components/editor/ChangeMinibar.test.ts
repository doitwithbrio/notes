import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import ChangeMinibar from './ChangeMinibar.svelte';

describe('ChangeMinibar', () => {
  afterEach(() => {
    cleanup();
  });

  it('renders non-interactive diff segments when no click handler is provided', () => {
    render(ChangeMinibar, {
      diffBlocks: [{ type: 'changed', content: 'Old text', lineStart: 3, lineEnd: 3 }],
      totalLines: 20,
    });

    const segment = screen.getByRole('button');
    expect(segment.hasAttribute('disabled')).toBe(true);
  });

  it('calls the click handler for a selected diff region when interactive', async () => {
    const onClickRegion = vi.fn();
    render(ChangeMinibar, {
      diffBlocks: [{ type: 'changed', content: 'Old text', lineStart: 3, lineEnd: 3 }],
      totalLines: 20,
      onClickRegion,
    });

    await fireEvent.click(screen.getByRole('button', { name: /changed change/i }));

    expect(onClickRegion).toHaveBeenCalledWith('changed-3-0');
  });
});
