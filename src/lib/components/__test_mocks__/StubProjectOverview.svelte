<script lang="ts">
  let { project }: { project: { id: string; name: string } } = $props();
</script>

<div data-testid="project-overview">project:{project.id}:{project.name}</div>
