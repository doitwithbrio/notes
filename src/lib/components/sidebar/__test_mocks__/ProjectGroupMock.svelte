<script lang="ts">
  let {
    project,
    docs,
    ondoccontextmenu,
  }: {
    project: { id: string };
    docs: Array<{ id: string }>;
    ondoccontextmenu?: (detail: { x: number; y: number; docId: string }) => void;
  } = $props();

  const firstDoc = $derived(docs[0] ?? null);
</script>

<div data-testid={`project-group-${project.id}`}>
  {#if firstDoc}
    <button
      data-testid={`doc-menu-${firstDoc.id}`}
      onclick={() => ondoccontextmenu?.({ x: 0, y: 0, docId: firstDoc.id })}
      type="button"
    >
      menu {firstDoc.id}
    </button>
  {/if}
</div>
