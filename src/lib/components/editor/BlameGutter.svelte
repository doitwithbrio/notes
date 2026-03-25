<script lang="ts">
  import type { Editor } from '@tiptap/core';
  import { CURSOR_COLORS, type BackendDocBlame } from '../../types/index.js';

  let {
    editor,
    blame,
  }: {
    editor: Editor | null;
    blame: BackendDocBlame;
  } = $props();

  interface GutterRow {
    top: number;
    height: number;
    alias: string;
    color: string;
    timestamp: number | null;
    /** Only show the label on the first row of a consecutive same-author run */
    showLabel: boolean;
  }

  let gutterRows = $state<GutterRow[]>([]);
  let tickCounter = $state(0);

  // Debounced re-measure on editor transactions
  $effect(() => {
    if (!editor) return;

    let timer: ReturnType<typeof setTimeout> | null = null;
    const measure = () => {
      timer = null;
      tickCounter++;
    };

    const handler = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(measure, 120);
    };

    editor.on('transaction', handler);

    // Initial measure
    tickCounter++;

    return () => {
      editor!.off('transaction', handler);
      if (timer) clearTimeout(timer);
    };
  });

  // Compute gutter rows from editor view + blame data
  $effect(() => {
    // Access tickCounter to re-run when editor changes
    void tickCounter;

    if (!editor || !blame || blame.spans.length === 0) {
      gutterRows = [];
      return;
    }

    const view = editor.view;
    const doc = view.state.doc;
    const rows: GutterRow[] = [];
    let prevActor: string | null = null;

    doc.forEach((node, offset) => {
      // Get the DOM node for this block
      const dom = view.nodeDOM(offset);
      if (!dom || !(dom instanceof HTMLElement)) return;

      const rect = dom.getBoundingClientRect();
      const scrollEl = dom.closest('.editor-scroll');
      if (!scrollEl) return;
      const scrollRect = scrollEl.getBoundingClientRect();

      const top = rect.top - scrollRect.top + scrollEl.scrollTop;
      const height = rect.height;

      // Map this block's character range to a blame span
      // offset is the ProseMirror position; we need the text offset
      // ProseMirror positions include structural characters, but blame spans
      // use plain-text character offsets. Approximate by counting text content
      // up to this node.
      const textBefore = doc.textBetween(0, offset, '\n');
      const textLen = node.textContent.length;
      const textStart = textBefore.length;
      const textEnd = textStart + textLen;

      // Find the blame span that covers the start of this block
      const span = blame.spans.find(
        (s) => s.start < textEnd && s.end > textStart,
      );

      if (!span) {
        prevActor = null;
        return;
      }

      const actor = blame.actors[span.actor];
      const isSelf = span.alias === 'You';
      const color = isSelf
        ? 'var(--accent)'
        : CURSOR_COLORS[
            (actor?.colorIndex ?? 0) % CURSOR_COLORS.length
          ] ?? CURSOR_COLORS[0];

      const showLabel = span.actor !== prevActor;
      prevActor = span.actor;

      rows.push({
        top,
        height,
        alias: span.alias ?? actor?.alias ?? 'unknown',
        color,
        timestamp: span.timestamp,
        showLabel,
      });
    });

    gutterRows = rows;
  });

  function formatRelativeTime(ts: number | null): string {
    if (!ts) return '';
    const now = Date.now() / 1000;
    const diff = now - ts;
    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
    if (diff < 604800) return `${Math.floor(diff / 86400)}d`;
    return `${Math.floor(diff / 604800)}w`;
  }
</script>

<div class="blame-gutter">
  {#each gutterRows as row, i (i)}
    <div
      class="blame-row"
      class:has-label={row.showLabel}
      style="top: {row.top}px; height: {row.height}px; --blame-color: {row.color};"
    >
      {#if row.showLabel}
        <span class="blame-author">{row.alias}</span>
        {#if row.timestamp}
          <span class="blame-time">{formatRelativeTime(row.timestamp)}</span>
        {/if}
      {/if}
    </div>
  {/each}
</div>

<style>
  .blame-gutter {
    position: relative;
    width: 130px;
    flex-shrink: 0;
    user-select: none;
    pointer-events: none;
  }

  .blame-row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: flex-start;
    gap: 4px;
    padding-top: 3px;
    padding-right: 12px;
    border-left: 3px solid var(--blame-color);
    box-sizing: border-box;
    overflow: hidden;
  }

  .blame-row:not(.has-label) {
    border-left-color: var(--blame-color);
    opacity: 0.3;
  }

  .blame-author {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
    padding-left: 8px;
  }

  .blame-time {
    font-size: 10px;
    color: var(--text-tertiary);
    white-space: nowrap;
    flex-shrink: 0;
  }
</style>
