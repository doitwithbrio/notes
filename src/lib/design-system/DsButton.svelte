<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    variant = 'secondary',
    type = 'button',
    disabled = false,
    className = '',
    testId,
    onclick,
    children,
    ...rest
  }: {
    variant?: 'primary' | 'secondary';
    type?: 'button' | 'submit' | 'reset';
    disabled?: boolean;
    className?: string;
    testId?: string;
    onclick?: (event: MouseEvent) => void;
    children?: Snippet;
    [key: string]: unknown;
  } = $props();
</script>

<button
  class={`ds-button ds-button--${variant} ${className}`.trim()}
  data-testid={testId}
  {disabled}
  {type}
  {onclick}
  {...rest}
>
  {@render children?.()}
</button>

<style>
  .ds-button {
    appearance: none;
    border: none;
    background: transparent;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-6);
    font-size: var(--text-body-sm);
    border-radius: var(--radius-md);
    transition: opacity var(--transition-fast), background var(--transition-fast), color var(--transition-fast);
    white-space: nowrap;
  }

  .ds-button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .ds-button--primary {
    padding: 7px 16px;
    font-weight: var(--font-weight-semibold);
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .ds-button--primary:hover:not(:disabled) {
    opacity: 0.85;
  }

  .ds-button--secondary {
    padding: 7px 14px;
    font-weight: var(--font-weight-medium);
    color: var(--text-secondary);
  }

  .ds-button--secondary:hover:not(:disabled) {
    background: var(--surface-hover);
  }
</style>
