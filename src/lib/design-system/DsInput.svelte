<script lang="ts">
  let {
    id,
    label,
    hint,
    describedBy,
    type = 'text',
    placeholder,
    disabled = false,
    mono = false,
    testId,
    value = $bindable(''),
    inputRef = $bindable(null),
    ...rest
  }: {
    id: string;
    label: string;
    hint?: string;
    describedBy?: string;
    type?: string;
    placeholder?: string;
    disabled?: boolean;
    mono?: boolean;
    testId?: string;
    value?: string;
    inputRef?: HTMLInputElement | null;
    [key: string]: unknown;
  } = $props();

  const hintId = $derived(hint ? `${id}-hint` : undefined);
  const describedByValue = $derived([
    hintId,
    describedBy,
  ].filter(Boolean).join(' ') || undefined);
</script>

<div class="ds-field">
  <label class="ds-field-label" for={id}>{label}</label>
  <input
    bind:this={inputRef}
    bind:value
    aria-describedby={describedByValue}
    class="ds-input"
    class:mono
    data-testid={testId}
    {disabled}
    {id}
    {placeholder}
    {type}
    {...rest}
  />
  {#if hint}
    <span class="ds-field-hint" id={hintId}>{hint}</span>
  {/if}
</div>

<style>
  .ds-field {
    margin-bottom: 14px;
  }

  .ds-field-label {
    display: block;
    font-size: var(--text-label);
    font-weight: var(--font-weight-semibold);
    color: var(--text-tertiary);
    margin-bottom: var(--space-6);
  }

  .ds-input {
    box-sizing: border-box;
    width: 100%;
    padding: 11px 14px;
    font-family: var(--font-body);
    font-size: var(--text-body);
    color: var(--text-primary);
    background: var(--surface-hover);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    outline: none;
    transition: border-color var(--transition-fast);
  }

  .ds-input:focus {
    border-color: var(--accent);
  }

  .ds-input::placeholder {
    color: var(--text-tertiary);
  }

  .ds-input.mono {
    font-size: var(--text-label);
    font-family: var(--font-mono);
  }

  .ds-input:disabled {
    opacity: 0.5;
  }

  .ds-field-hint {
    display: block;
    font-size: 11px;
    color: var(--text-tertiary);
    margin-top: var(--space-4);
  }
</style>
