<script lang="ts">
  import type { DiffBlock } from '../../types/index.js';
  import { getDiffBlockTargetId } from '../../utils/diff.js';

  let {
    diffBlocks = [],
    totalLines = 0,
    onClickRegion,
  }: {
    diffBlocks: DiffBlock[];
    totalLines: number;
    onClickRegion?: (targetId: string) => void;
  } = $props();

  // Map diff blocks to minibar segments
  const segments = $derived.by(() => {
    if (totalLines <= 0 || diffBlocks.length === 0) return [];

    return diffBlocks
      .filter((b) => b.type !== 'unchanged')
      .map((block, index) => {
        const startPct = ((block.lineStart - 1) / totalLines) * 100;
        // Each block gets at least 2% height for visibility
        const heightPct = Math.max(2, (1 / totalLines) * 100);

        return {
          targetId: getDiffBlockTargetId(block, index),
          order: index + 1,
          type: block.type,
          top: startPct,
          height: heightPct,
          lineStart: block.lineStart,
        };
      });
  });

  function colorForType(type: string): string {
    switch (type) {
      case 'added':
        return 'var(--diff-added)';
      case 'removed':
        return 'var(--diff-removed)';
      case 'changed':
        return 'var(--diff-changed)';
      default:
        return 'transparent';
    }
  }
</script>

{#if segments.length > 0}
  <div class="minibar">
    {#each segments as seg (seg.targetId)}
      <button
        class="segment"
        class:clickable={!!onClickRegion}
        style="top: {seg.top}%; height: {seg.height}%; background: {colorForType(seg.type)};"
        onclick={() => onClickRegion?.(seg.targetId)}
        disabled={!onClickRegion}
        aria-label="{seg.type} change {seg.order}"
        title="{seg.type} change {seg.order}"
      ></button>
    {/each}
  </div>
{/if}

<style>
  .minibar {
    position: absolute;
    left: 10px;
    top: 0;
    bottom: 0;
    width: 8px;
    z-index: 5;
    border-radius: 999px;
    background: color-mix(in srgb, var(--border-default) 55%, transparent);
  }

  .segment {
    position: absolute;
    left: 0;
    width: 100%;
    border-radius: 1px;
    opacity: 0.7;
    cursor: default;
    transition: opacity var(--transition-fast);
  }

  .segment.clickable {
    cursor: pointer;
  }

  .segment:hover {
    opacity: 1;
  }
</style>
