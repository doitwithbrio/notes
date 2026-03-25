<script lang="ts">
  import type { DiffBlock } from '../../types/index.js';

  let {
    diffBlocks = [],
    totalLines = 0,
    onClickRegion,
  }: {
    diffBlocks: DiffBlock[];
    totalLines: number;
    onClickRegion?: (lineStart: number) => void;
  } = $props();

  // Map diff blocks to minibar segments
  const segments = $derived.by(() => {
    if (totalLines <= 0 || diffBlocks.length === 0) return [];

    return diffBlocks
      .filter((b) => b.type !== 'unchanged')
      .map((block) => {
        const startPct = ((block.lineStart - 1) / totalLines) * 100;
        // Each block gets at least 2% height for visibility
        const heightPct = Math.max(2, (1 / totalLines) * 100);

        return {
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
        return 'var(--diff-added, #6BAA8A)';
      case 'removed':
        return 'var(--diff-removed, #C4836A)';
      case 'changed':
        return 'var(--diff-changed, #C4A64E)';
      default:
        return 'transparent';
    }
  }
</script>

{#if segments.length > 0}
  <div class="minibar">
    {#each segments as seg (seg.lineStart)}
      <button
        class="segment"
        style="top: {seg.top}%; height: {seg.height}%; background: {colorForType(seg.type)};"
        onclick={() => onClickRegion?.(seg.lineStart)}
        title="{seg.type} at line {seg.lineStart}"
      ></button>
    {/each}
  </div>
{/if}

<style>
  .minibar {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 5px;
    z-index: 5;
  }

  .segment {
    position: absolute;
    left: 0;
    width: 100%;
    border-radius: 1px;
    opacity: 0.7;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .segment:hover {
    opacity: 1;
  }
</style>
