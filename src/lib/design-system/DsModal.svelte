<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    closeLabel,
    onclose,
    labelledBy,
    label,
    panelTestId,
    shellStyle = '',
    panelStyle = '',
    children,
  }: {
    closeLabel: string;
    onclose: () => void;
    labelledBy?: string;
    label?: string;
    panelTestId?: string;
    shellStyle?: string;
    panelStyle?: string;
    children?: Snippet;
  } = $props();

  let panelEl = $state<HTMLDivElement | null>(null);
  let previousFocusedElement = $state<HTMLElement | null>(null);

  function getFocusableElements() {
    if (!panelEl) return [] as HTMLElement[];
    return Array.from(
      panelEl.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((element) => !element.hasAttribute('aria-hidden'));
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose();
      return;
    }

    if (event.key !== 'Tab' || !panelEl) return;

    const focusable = getFocusableElements();
    if (focusable.length === 0) {
      event.preventDefault();
      panelEl.focus();
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement as HTMLElement | null;

    if (event.shiftKey && (active === first || active === panelEl)) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first?.focus();
    }
  }

  $effect(() => {
    previousFocusedElement = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;

    queueMicrotask(() => {
      if (!panelEl) return;
      const active = document.activeElement;
      if (active instanceof Node && panelEl.contains(active)) return;
      panelEl.focus();
    });

    return () => {
      previousFocusedElement?.focus();
    };
  });
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div class="ds-modal-shell" style={shellStyle}>
  <button
    aria-label={closeLabel}
    class="ds-modal-backdrop"
    onclick={onclose}
    tabindex="-1"
    type="button"
  ></button>

  <div
    aria-label={label}
    aria-labelledby={labelledBy}
    aria-modal="true"
    bind:this={panelEl}
    class="ds-modal-panel"
    data-testid={panelTestId}
    role="dialog"
    style={panelStyle}
    tabindex="-1"
  >
    {@render children?.()}
  </div>
</div>

<style>
  .ds-modal-shell {
    position: fixed;
    inset: 0;
    z-index: var(--z-overlay);
    display: flex;
    justify-content: center;
    padding-top: var(--overlay-offset-top);
  }

  .ds-modal-backdrop {
    position: absolute;
    inset: 0;
    border: none;
    padding: 0;
    background: var(--overlay-backdrop);
    backdrop-filter: blur(2px);
    -webkit-backdrop-filter: blur(2px);
    animation: ds-modal-backdrop-in var(--motion-enter-fade);
  }

  .ds-modal-panel {
    position: relative;
    z-index: 1;
    width: min(var(--overlay-width-md), calc(100vw - var(--space-32)));
    max-height: min(var(--overlay-height-md), calc(100vh - var(--space-32)));
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-2xl);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: ds-modal-panel-in var(--motion-enter-panel);
  }

  @keyframes ds-modal-backdrop-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes ds-modal-panel-in {
    from {
      opacity: 0;
      transform: scale(0.97) translateY(-4px);
    }

    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }
</style>
