<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    count,
    divider = false,
    className = '',
    testId,
    children,
  }: {
    title: string;
    count?: string | number | null;
    divider?: boolean;
    className?: string;
    testId?: string;
    children?: Snippet;
  } = $props();

  const hasCount = $derived(count !== undefined && count !== null && `${count}`.length > 0);
  const headingId = $derived.by(() => {
    if (testId) return `${testId}-title`;
    return `ds-section-${title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')}`;
  });
</script>

<section aria-labelledby={headingId} class={`ds-section ${className}`.trim()} class:with-divider={divider} data-testid={testId}>
  <div class="ds-section-header">
    <div class="ds-section-heading">
      <h2 class="ds-section-title" id={headingId}>{title}</h2>
      {#if hasCount}
        <span class="ds-section-count">{count}</span>
      {/if}
    </div>
  </div>

  {@render children?.()}
</section>

<style>
  .ds-section {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .ds-section.with-divider {
    border-bottom: 1px solid var(--border-subtle);
  }

  .ds-section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-8);
    padding: var(--space-12) var(--space-16) var(--space-8);
    flex-shrink: 0;
  }

  .ds-section-heading {
    display: flex;
    align-items: center;
    gap: var(--space-6);
    min-width: 0;
  }

  .ds-section-title {
    font-size: var(--text-body-sm);
    font-weight: var(--font-weight-bold);
    letter-spacing: -0.01em;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .ds-section-count {
    font-size: 10px;
    font-weight: var(--font-weight-semibold);
    color: var(--text-primary);
    background: var(--surface-active);
    padding: 0 5px;
    border-radius: var(--radius-md);
    line-height: 16px;
    flex-shrink: 0;
  }

</style>
