<script lang="ts">
  import type { MenuItem } from '../ContextMenu.svelte';

  let { items, onclose }: { items: MenuItem[]; onclose?: () => void } = $props();

  function flatten(menuItems: MenuItem[], prefix = ''): Array<{ key: string; action: () => void }> {
    return menuItems.flatMap((item) => {
      const label = prefix ? `${prefix} > ${item.label}` : item.label;
      const own = item.action ? [{ key: label, action: item.action }] : [];
      const children = item.children ? flatten(item.children, label) : [];
      return [...own, ...children];
    });
  }

  const flattened = $derived(flatten(items));
</script>

<div data-testid="context-menu">
  {#each flattened as item}
    <button
      data-testid={`menu-item-${item.key}`}
      onclick={() => {
        item.action();
        onclose?.();
      }}
      type="button"
    >
      {item.key}
    </button>
  {/each}
</div>
