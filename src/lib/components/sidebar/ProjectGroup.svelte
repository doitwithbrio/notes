<script lang="ts">
  import type { Project, Document } from '../../types/index.js';
  import FileItem from './FileItem.svelte';

  let { project, docs }: { project: Project; docs: Document[] } = $props();

  let collapsed = $state(false);
</script>

<div class="project-group">
  <button class="project-header" onclick={() => (collapsed = !collapsed)}>
    <span class="arrow" class:collapsed>{'\u25BE'}</span>
    <span class="project-name">{project.name}</span>
    {#if project.shared}
      <span class="shared-badge">P2P</span>
    {/if}
  </button>

  {#if !collapsed}
    <div class="file-list">
      {#each docs as doc (doc.id)}
        <FileItem {doc} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .project-group {
    margin-bottom: 2px;
  }

  .project-header {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 4px 16px;
    font-size: 13px;
    font-weight: 500;
    text-align: left;
    color: var(--black);
  }

  .project-header:hover {
    color: var(--accent);
  }

  .arrow {
    font-size: 10px;
    transition: transform 0.15s;
    width: 12px;
    text-align: center;
  }

  .arrow.collapsed {
    transform: rotate(-90deg);
  }

  .project-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .shared-badge {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.05em;
    color: var(--accent);
    border: 1px solid var(--accent);
    padding: 0 4px;
    line-height: 16px;
  }

  .file-list {
    padding-left: 12px;
  }
</style>
