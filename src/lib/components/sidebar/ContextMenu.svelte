<script module lang="ts">
  import type { Component } from 'svelte';

  export type MenuItem = {
    label: string;
    icon?: Component;
    danger?: boolean;
    children?: MenuItem[];
    action?: () => void;
  };
</script>

<script lang="ts">
  let {
    x,
    y,
    items,
    onclose,
  }: {
    x: number;
    y: number;
    items: MenuItem[];
    onclose: () => void;
  } = $props();

  let menuEl = $state<HTMLDivElement | null>(null);
  let expandedIndex = $state<number | null>(null);

  function handleItemClick(item: MenuItem) {
    if (item.children) {
      return;
    }
    item.action?.();
    onclose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onclose();
    }
  }

  function handleBackdropClick(e: MouseEvent) {
    if (menuEl && !menuEl.contains(e.target as Node)) {
      onclose();
    }
  }

  function handleBackdropContextMenu(e: MouseEvent) {
    e.preventDefault();
    onclose();
  }

  // Clamp position to viewport
  const clampedX = $derived(Math.min(x, window.innerWidth - 200));
  const clampedY = $derived(Math.min(y, window.innerHeight - items.length * 36 - 16));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="context-backdrop" onclick={handleBackdropClick} oncontextmenu={handleBackdropContextMenu} onkeydown={handleKeydown}>
  <div
    class="context-menu"
    bind:this={menuEl}
    style="left: {clampedX}px; top: {clampedY}px"
  >
    {#each items as item, i (item.label)}
      {#if item.children}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="menu-item has-children"
          onmouseenter={() => (expandedIndex = i)}
          onmouseleave={() => (expandedIndex = null)}
        >
          <span class="item-label">{item.label}</span>
          <span class="submenu-arrow">›</span>

          {#if expandedIndex === i}
            <div class="submenu">
              {#each item.children as child (child.label)}
                <button
                  class="menu-item"
                  class:danger={child.danger}
                  onclick={() => handleItemClick(child)}
                >
                  <span class="item-label">{child.label}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <button
          class="menu-item"
          class:danger={item.danger}
          onclick={() => handleItemClick(item)}
        >
          <span class="item-label">{item.label}</span>
        </button>
      {/if}
    {/each}
  </div>
</div>

<style>
  .context-backdrop {
    position: fixed;
    inset: 0;
    z-index: 200;
  }

  .context-menu {
    position: fixed;
    min-width: 160px;
    padding: 4px;
    border-radius: 10px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.08), 0 1px 4px rgba(0, 0, 0, 0.04);
    animation: menuIn 100ms ease;
  }

  @keyframes menuIn {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 7px 12px;
    font-size: 13px;
    font-weight: 400;
    color: var(--text-primary);
    text-align: left;
    border-radius: 6px;
    transition: background var(--transition-fast);
    position: relative;
    background: none;
    border: none;
    cursor: default;
    font-family: var(--font-body);
  }

  .menu-item:hover {
    background: var(--surface-hover);
  }

  .menu-item.danger {
    color: #c0392b;
  }

  .menu-item.danger:hover {
    background: rgba(192, 57, 43, 0.08);
  }

  .submenu-arrow {
    font-size: 14px;
    color: var(--text-tertiary);
    margin-left: 8px;
  }

  .submenu {
    position: absolute;
    left: 100%;
    top: -4px;
    min-width: 140px;
    padding: 4px;
    border-radius: 10px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.08), 0 1px 4px rgba(0, 0, 0, 0.04);
    margin-left: 2px;
  }
</style>
