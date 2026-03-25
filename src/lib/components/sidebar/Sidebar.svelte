<script lang="ts">
  import { PanelLeftClose, PanelLeftOpen, PenLine, Check } from 'lucide-svelte';

  import { sortable } from '../../actions/sortable.js';
  import { isMac, modKey } from '../../utils/platform.js';
  import { documentState, loadProjectDocs, reorderDocs } from '../../state/documents.svelte.js';
  import { createProject, projectState, reorderProject } from '../../state/projects.svelte.js';
  import { uiState, openProjectOverview, toggleSidebar } from '../../state/ui.svelte.js';
  import { openEditorSession } from '../../session/editor-session.svelte.js';
  import { tauriApi } from '../../api/tauri.js';
  import ProjectGroup from './ProjectGroup.svelte';

  let creatingProject = $state(false);
  let creatingProjectName = $state('');
  let creatingNoteProjectId = $state<string | null>(null);
  let creatingNoteTitle = $state('');
  let showProjectPicker = $state(false);
  let editMode = $state(false);

  function focusInput(el: HTMLInputElement) {
    requestAnimationFrame(() => el.focus());
  }

  async function commitNewProject(name: string) {
    const trimmed = name.trim();
    if (!trimmed) {
      creatingProject = false;
      creatingProjectName = '';
      return;
    }
    try {
      const project = await createProject(trimmed);
      if (project) {
        await loadProjectDocs(project.id);
        openProjectOverview(project.id);
      }
    } catch (err) {
      console.error('Failed to create project:', err);
    } finally {
      creatingProject = false;
      creatingProjectName = '';
    }
  }

  async function commitNewNote(title: string) {
    const projectId = creatingNoteProjectId;
    const trimmed = title.trim();
    if (!projectId || !trimmed) {
      creatingNoteProjectId = null;
      creatingNoteTitle = '';
      return;
    }
    try {
      const leaf = trimmed.endsWith('.md') ? trimmed : `${trimmed}.md`;
      const docId = await tauriApi.createNote(projectId, leaf);
      await loadProjectDocs(projectId);
      await openEditorSession(projectId, docId);
    } catch (err) {
      console.error('Failed to create note:', err);
    } finally {
      creatingNoteProjectId = null;
      creatingNoteTitle = '';
    }
  }

  function startNewProject() {
    creatingProject = true;
    creatingProjectName = '';
  }

  function startNewNote(projectId: string) {
    showProjectPicker = false;
    creatingNoteProjectId = projectId;
    creatingNoteTitle = '';
  }

  function cancelNewProject() {
    creatingProject = false;
    creatingProjectName = '';
  }

  function cancelNewNote() {
    creatingNoteProjectId = null;
    creatingNoteTitle = '';
  }

  function handleKeydown(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    if (!mod) return;

    if (e.key === 'n') {
      e.preventDefault();
      startNewProject();
    }

    if (e.key === 't') {
      e.preventDefault();
      // New file in the active project, or the first project if none active
      const projectId =
        uiState.activeProjectId
        ?? projectState.projects[0]?.id
        ?? null;
      if (projectId) {
        startNewNote(projectId);
      }
    }
  }

  function handleWindowClick(e: MouseEvent) {
    if (!showProjectPicker) return;
    const target = e.target as HTMLElement;
    if (!target.closest('.project-picker') && !target.closest('.footer-new-project')) {
      showProjectPicker = false;
    }
  }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={handleKeydown} />

<aside class="sidebar" class:collapsed={!uiState.sidebarOpen}>
  <div class="sidebar-header" data-tauri-drag-region>
    {#if uiState.sidebarOpen}
      <span class="app-name" style="-webkit-app-region: no-drag">notes</span>
    {/if}
    <button
      class="collapse-toggle"
      style="-webkit-app-region: no-drag"
      onclick={toggleSidebar}
      aria-label={uiState.sidebarOpen ? 'collapse sidebar' : 'expand sidebar'}
    >
      {#if uiState.sidebarOpen}
        <PanelLeftClose size={14} strokeWidth={1.5} />
      {:else}
        <PanelLeftOpen size={14} strokeWidth={1.5} />
      {/if}
    </button>
  </div>

  {#if uiState.sidebarOpen}
    <button class="search-trigger" onclick={() => (uiState.quickOpenVisible = true)}>
      <span class="search-label">search...</span>
      <span class="search-hint">{modKey}+F</span>
    </button>
  {/if}

  <div
    class="sidebar-scroll"
    use:sortable={{
      onReorder: ({ fromIndex, toIndex }) => reorderProject(fromIndex, toIndex),
      itemSelector: ':scope > .project-group-wrapper',
      enabled: uiState.sidebarOpen && editMode,
    }}
  >
    {#if creatingProject}
      <div class="inline-create">
        <input
          bind:value={creatingProjectName}
          class="inline-input"
          placeholder="project name"
          use:focusInput
          onkeydown={(event) => {
            if (event.key === 'Enter') { event.preventDefault(); void commitNewProject(creatingProjectName); }
            if (event.key === 'Escape') { event.preventDefault(); cancelNewProject(); }
          }}
          onblur={() => void commitNewProject(creatingProjectName)}
        />
        <button
          class="inline-accept"
          onclick={() => void commitNewProject(creatingProjectName)}
          aria-label="create project"
        >
          <Check size={13} strokeWidth={2} />
        </button>
      </div>
    {/if}

    {#each projectState.projects as project (project.id)}
      <div class="project-group-wrapper">
        <ProjectGroup
          {project}
          docs={documentState.docs.filter((doc) => doc.projectId === project.id)}
          collapsed={!uiState.sidebarOpen}
          editing={false}
          editingDocId={creatingNoteProjectId ? '__creating__' : null}
          {editMode}
          oncancel={cancelNewProject}
          onnewnote={() => startNewNote(project.id)}
          onprojectclick={() => openProjectOverview(project.id)}
          ondocopen={(docId) => openEditorSession(project.id, docId)}
          ondoccommit={(title) => void commitNewNote(title)}
          ondoccancel={cancelNewNote}
          onreorderdocs={({ fromIndex, toIndex }) => reorderDocs(project.id, fromIndex, toIndex)}
        />

        {#if creatingNoteProjectId === project.id}
          <div class="inline-create note">
            <input
              bind:value={creatingNoteTitle}
              class="inline-input"
              placeholder="note title"
              use:focusInput
              onkeydown={(event) => {
                if (event.key === 'Enter') { event.preventDefault(); void commitNewNote(creatingNoteTitle); }
                if (event.key === 'Escape') { event.preventDefault(); cancelNewNote(); }
              }}
              onblur={() => void commitNewNote(creatingNoteTitle)}
            />
            <button
              class="inline-accept"
              onclick={() => void commitNewNote(creatingNoteTitle)}
              aria-label="create note"
            >
              <Check size={13} strokeWidth={2} />
            </button>
          </div>
        {/if}
      </div>
    {/each}

    {#if projectState.projects.length === 0 && !creatingProject}
      <div class="empty">
        {#if uiState.sidebarOpen}
          <p>no projects yet</p>
        {/if}
      </div>
    {/if}
  </div>

  <div class="sidebar-footer">
    {#if uiState.sidebarOpen}
      {#if showProjectPicker}
        <div class="project-picker">
          {#each projectState.projects as project (project.id)}
            <button class="picker-item" onclick={() => startNewNote(project.id)}>
              {project.name}
            </button>
          {/each}
        </div>
      {/if}
      <div class="footer-row">
        {#if editMode}
          <span class="edit-mode-label">drag to reorder</span>
          <button class="footer-done-btn" onclick={() => (editMode = false)}>done</button>
        {:else}
          <button class="footer-new-project" onclick={startNewProject}>+ new project</button>
          <button class="footer-edit-toggle" onclick={() => (editMode = true)} aria-label="edit order">
            <PenLine size={13} strokeWidth={1.5} />
          </button>
        {/if}
      </div>
    {:else}
      <button class="footer-new-project collapsed-btn" onclick={startNewProject}>+</button>
    {/if}
  </div>
</aside>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    transition: width var(--transition-slow);
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px 20px;
    gap: 8px;
    flex-shrink: 0;
    -webkit-app-region: drag;
  }

  .collapsed .sidebar-header {
    justify-content: center;
    padding-left: 10px;
    padding-right: 10px;
  }

  .app-name {
    font-family: var(--font-body);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: var(--text-primary);
  }

  .collapse-toggle,
  .footer-edit-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    color: var(--text-primary);
    flex-shrink: 0;
    border-radius: 6px;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .collapse-toggle:hover,
  .footer-edit-toggle:hover {
    color: var(--text-primary);
    background: var(--surface-hover);
  }

  .footer-done-btn {
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
    padding: 2px 8px;
    border-radius: 4px;
    transition: background var(--transition-fast);
  }

  .footer-done-btn:hover {
    background: var(--surface-hover);
  }

  .search-trigger {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 0 12px 12px;
    padding: 9px 12px;
    border-radius: 10px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
  }

  .search-label {
    color: var(--text-primary);
  }

  .search-hint {
    font-size: 12px;
    color: var(--text-primary);
  }

  .sidebar-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 8px 12px;
  }

  .project-group-wrapper {
    margin-bottom: 6px;
  }

  .inline-create {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 4px 4px;
    padding: 9px 12px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    transition: border-color var(--transition-fast);
  }

  .inline-create:focus-within {
    border-color: var(--accent);
  }

  .inline-create.note {
    margin-left: 10px;
  }

  .inline-input {
    flex: 1;
    min-width: 0;
    font-family: var(--font-body);
    font-size: 13px;
    color: var(--text-primary);
    padding: 0;
    border: none;
    outline: none;
    background: transparent;
  }

  .inline-accept {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    color: var(--accent);
    border-radius: 6px;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .inline-accept:hover {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .empty {
    display: flex;
    justify-content: center;
    padding: 20px 0;
    color: var(--text-primary);
  }

  .sidebar-footer {
    padding: 12px 16px 16px;
    flex-shrink: 0;
    position: relative;
  }

  .collapsed .sidebar-footer {
    padding: 12px 10px 16px;
  }

  .footer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .footer-new-project {
    padding: 0;
    font-size: 13px;
    font-weight: 500;
    text-align: left;
    color: var(--text-primary);
    transition: color var(--transition-fast);
  }

  .footer-new-project:hover {
    color: var(--text-primary);
  }

  .collapsed-btn {
    width: 100%;
    font-size: 16px;
    text-align: center;
  }

  .project-picker {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: calc(100% + 8px);
    display: flex;
    flex-direction: column;
    padding: 6px;
    border-radius: 12px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    box-shadow: var(--shadow-overlay);
  }

  .picker-item {
    text-align: left;
    padding: 8px 10px;
    border-radius: 8px;
  }

  .picker-item:hover {
    background: var(--surface-hover);
  }

  .edit-mode-label {
    font-size: 12px;
    color: var(--text-primary);
    flex: 1;
    padding-left: 2px;
  }
</style>
