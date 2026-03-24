<script lang="ts">
  import { projectState } from '../../state/projects.svelte.js';
  import { documentState } from '../../state/documents.svelte.js';
  import ProjectGroup from './ProjectGroup.svelte';

  const localProjects = $derived(
    projectState.projects.filter((p) => !p.shared),
  );
  const sharedProjects = $derived(
    projectState.projects.filter((p) => p.shared),
  );
</script>

<aside class="sidebar">
  <div class="sidebar-scroll">
    {#if localProjects.length > 0}
      <div class="section">
        <h2 class="section-header">Local</h2>
        {#each localProjects as project (project.id)}
          <ProjectGroup {project} docs={documentState.docs.filter((d) => d.projectId === project.id)} />
        {/each}
      </div>
    {/if}

    {#if sharedProjects.length > 0}
      <div class="section">
        <h2 class="section-header">Shared</h2>
        {#each sharedProjects as project (project.id)}
          <ProjectGroup {project} docs={documentState.docs.filter((d) => d.projectId === project.id)} />
        {/each}
      </div>
    {/if}

    {#if projectState.projects.length === 0}
      <div class="empty">
        <p>No projects yet.</p>
        <p>Create a folder to get started.</p>
      </div>
    {/if}
  </div>

  <div class="sidebar-footer">
    <button class="new-btn">+ New Note</button>
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--white);
    overflow: hidden;
  }

  .sidebar-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 8px 0;
  }

  .section {
    margin-bottom: 8px;
  }

  .section-header {
    font-family: var(--font-display);
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--black);
    padding: 8px 16px 4px;
  }

  .empty {
    padding: 24px 16px;
    font-size: 13px;
    color: var(--black);
    text-align: center;
    line-height: 1.6;
  }

  .sidebar-footer {
    border-top: var(--border);
    padding: 8px 12px;
  }

  .new-btn {
    width: 100%;
    padding: 6px 12px;
    font-size: 13px;
    text-align: left;
    color: var(--accent);
    font-weight: 500;
  }

  .new-btn:hover {
    color: var(--black);
  }
</style>
