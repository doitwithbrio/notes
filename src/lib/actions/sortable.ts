/**
 * Svelte action for lightweight drag-and-drop reordering.
 * Uses pointer events for full styling control.
 *
 * Usage:
 *   <div use:sortable={{ onReorder: ({ fromIndex, toIndex }) => ... }}>
 *     <div>item 0</div>
 *     <div>item 1</div>
 *   </div>
 */

export interface SortableOptions {
  onReorder: (detail: { fromIndex: number; toIndex: number }) => void;
  /** CSS selector to identify draggable children. Defaults to ':scope > *' */
  itemSelector?: string;
  /** Minimum px of movement before drag starts */
  threshold?: number;
  /** Whether sorting is enabled */
  enabled?: boolean;
}

export function sortable(node: HTMLElement, options: SortableOptions) {
  let opts = { threshold: 5, itemSelector: ':scope > *', enabled: true, ...options };

  let dragging = false;
  let dragIndex = -1;
  let startX = 0;
  let startY = 0;
  let origRect: DOMRect | null = null;
  let ghostEl: HTMLElement | null = null;
  let indicatorEl: HTMLElement | null = null;
  let currentDropIndex = -1;
  let items: HTMLElement[] = [];

  function getItems(): HTMLElement[] {
    return Array.from(node.querySelectorAll(opts.itemSelector!)) as HTMLElement[];
  }

  function onPointerDown(e: PointerEvent) {
    if (!opts.enabled) return;
    // Only left mouse button
    if (e.button !== 0) return;

    // Don't drag if clicking on an input or text-editable element
    const target = e.target as HTMLElement;
    if (target.closest('input, textarea, [contenteditable]')) return;

    items = getItems();
    // Find the drag item containing the click target.
    // Note: closest() does not support :scope correctly, so we match against
    // the items already found by querySelectorAll (which handles :scope fine).
    const item = items.find((el) => el === target || el.contains(target)) ?? null;
    if (!item || !node.contains(item)) return;

    // Allow drag if the click is on or inside a designated drag handle
    const dragHandle = target.closest('[data-drag-handle]');
    if (dragHandle && item.contains(dragHandle)) {
      // Handle found — skip button guard, allow drag
    } else {
      // Skip if clicking a button/link that is a CHILD of the drag item (not the item itself)
      const clickedButton = target.closest('button, a');
      if (clickedButton && clickedButton !== item && item.contains(clickedButton)) return;
    }

    dragIndex = items.indexOf(item);
    if (dragIndex < 0) return;

    // Prevent text selection and default browser drag
    e.preventDefault();

    startX = e.clientX;
    startY = e.clientY;

    // Listen globally for move/up
    document.addEventListener('pointermove', onPointerMove);
    document.addEventListener('pointerup', onPointerUp);
  }

  function startDrag(_e: PointerEvent) {
    dragging = true;
    const item = items[dragIndex]!;
    const rect = item.getBoundingClientRect();

    // Store the original rect once — reuse in onPointerMove
    origRect = rect;

    // Create ghost
    ghostEl = item.cloneNode(true) as HTMLElement;
    ghostEl.style.position = 'fixed';
    ghostEl.style.left = `${rect.left}px`;
    ghostEl.style.top = `${rect.top}px`;
    ghostEl.style.width = `${rect.width}px`;
    ghostEl.style.height = `${rect.height}px`;
    ghostEl.style.pointerEvents = 'none';
    ghostEl.style.zIndex = '9999';
    ghostEl.style.opacity = '0.85';
    ghostEl.style.transition = 'none';
    ghostEl.style.boxShadow = '0 4px 12px rgba(0,0,0,0.08)';
    ghostEl.style.borderRadius = '8px';
    ghostEl.style.background = 'var(--surface)';
    document.body.appendChild(ghostEl);

    // Create drop indicator
    indicatorEl = document.createElement('div');
    indicatorEl.style.position = 'fixed';
    indicatorEl.style.height = '2px';
    indicatorEl.style.background = 'var(--accent)';
    indicatorEl.style.borderRadius = '1px';
    indicatorEl.style.zIndex = '9998';
    indicatorEl.style.pointerEvents = 'none';
    indicatorEl.style.display = 'none';
    document.body.appendChild(indicatorEl);

    // Dim original
    item.style.opacity = '0.25';

    // Set grabbing cursor
    document.body.style.cursor = 'grabbing';
    document.body.style.userSelect = 'none';
  }

  function onPointerMove(e: PointerEvent) {
    const dx = e.clientX - startX;
    const dy = e.clientY - startY;

    if (!dragging) {
      // Check threshold
      if (Math.abs(dx) + Math.abs(dy) > opts.threshold!) {
        startDrag(e);
      }
      return;
    }

    // Move ghost using stored origRect
    if (ghostEl && origRect) {
      ghostEl.style.left = `${origRect.left + dx}px`;
      ghostEl.style.top = `${origRect.top + dy}px`;
    }

    // Calculate drop position
    const pointerY = e.clientY;
    let dropIdx = items.length;

    for (let i = 0; i < items.length; i++) {
      const rect = items[i]!.getBoundingClientRect();
      const midY = rect.top + rect.height / 2;
      if (pointerY < midY) {
        dropIdx = i;
        break;
      }
    }

    currentDropIndex = dropIdx;

    // Position indicator
    if (indicatorEl && items.length > 0) {
      const containerRect = node.getBoundingClientRect();
      let indicatorY: number;

      if (dropIdx === 0) {
        indicatorY = items[0]!.getBoundingClientRect().top;
      } else if (dropIdx >= items.length) {
        const lastRect = items[items.length - 1]!.getBoundingClientRect();
        indicatorY = lastRect.bottom;
      } else {
        const prevRect = items[dropIdx - 1]!.getBoundingClientRect();
        const nextRect = items[dropIdx]!.getBoundingClientRect();
        indicatorY = (prevRect.bottom + nextRect.top) / 2;
      }

      indicatorEl.style.display = 'block';
      indicatorEl.style.left = `${containerRect.left + 4}px`;
      indicatorEl.style.top = `${indicatorY - 1}px`;
      indicatorEl.style.width = `${containerRect.width - 8}px`;
    }
  }

  function onPointerUp(e: PointerEvent) {
    document.removeEventListener('pointermove', onPointerMove);
    document.removeEventListener('pointerup', onPointerUp);

    if (dragging) {
      e.preventDefault();

      // Clean up ghost and indicator
      ghostEl?.remove();
      ghostEl = null;
      indicatorEl?.remove();
      indicatorEl = null;
      origRect = null;

      // Restore original item
      if (items[dragIndex]) {
        items[dragIndex]!.style.opacity = '';
      }

      // Restore cursor
      document.body.style.cursor = '';
      document.body.style.userSelect = '';

      // Suppress the synthetic click that follows pointerup on buttons
      document.addEventListener(
        'click',
        (clickE) => {
          clickE.stopPropagation();
          clickE.preventDefault();
        },
        { capture: true, once: true },
      );

      // Fire callback
      let toIndex = currentDropIndex;
      // Adjust: if dropping after the original position, account for the removed item
      if (toIndex > dragIndex) toIndex--;
      if (toIndex !== dragIndex && toIndex >= 0) {
        opts.onReorder({ fromIndex: dragIndex, toIndex });
      }
    }

    dragging = false;
    dragIndex = -1;
    currentDropIndex = -1;
  }

  node.addEventListener('pointerdown', onPointerDown);

  return {
    update(newOptions: SortableOptions) {
      opts = { threshold: 5, itemSelector: ':scope > *', enabled: true, ...newOptions };
    },
    destroy() {
      node.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('pointermove', onPointerMove);
      document.removeEventListener('pointerup', onPointerUp);
      ghostEl?.remove();
      indicatorEl?.remove();
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    },
  };
}
