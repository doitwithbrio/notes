<script lang="ts">
  import { editorSessionState } from '../../session/editor-session.svelte.js';
  import { createVersion, hideSavePrompt, versionState } from '../../state/versions.svelte.js';

  let inputEl = $state<HTMLInputElement | null>(null);
  let saving = $state(false);
  let savedName = $state<string | null>(null);

  // Auto-focus the input when visible
  $effect(() => {
    if (versionState.savePromptVisible && inputEl) {
      // Small delay to ensure DOM is ready
      setTimeout(() => inputEl?.focus(), 50);
    }
  });

  async function handleSave() {
    if (!editorSessionState.projectId || !editorSessionState.docId) return;
    saving = true;
    const label = inputEl?.value.trim() || undefined;

    try {
      const version = await createVersion(
        editorSessionState.projectId,
        editorSessionState.docId,
        label ?? 'Checkpoint',
      );
      if (version) {
        savedName = version.name;
        // Show confirmation briefly
        setTimeout(() => {
          hideSavePrompt();
          savedName = null;
        }, 1500);
      } else {
        hideSavePrompt();
      }
    } catch {
      hideSavePrompt();
    } finally {
      saving = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleSave();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      hideSavePrompt();
    }
  }

  function handleCancel() {
    hideSavePrompt();
  }
</script>

{#if versionState.savePromptVisible}
  <div class="save-bar">
    {#if savedName}
      <span class="saved-msg">saved as ★ {savedName}</span>
    {:else}
      <span class="save-label">save version:</span>
      <input
        bind:this={inputEl}
        type="text"
        class="save-input"
        placeholder="name (optional)"
        onkeydown={handleKeydown}
        disabled={saving}
      />
      <button class="btn-save" onclick={handleSave} disabled={saving}>
        {saving ? 'saving...' : 'save'}
      </button>
      <button class="btn-cancel" onclick={handleCancel}>✕</button>
    {/if}
  </div>
{/if}

<style>
  .save-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 20px;
    height: 36px;
    background: color-mix(in srgb, var(--accent) 6%, var(--surface));
    border-bottom: 1px solid var(--border-default);
    flex-shrink: 0;
  }

  .save-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
  }

  .save-input {
    flex: 1;
    max-width: 260px;
    padding: 4px 8px;
    font-size: 12px;
    border: 1px solid var(--border-default);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text-primary);
    outline: none;
    transition: border-color var(--transition-fast);
  }

  .save-input:focus {
    border-color: var(--accent);
  }

  .save-input::placeholder {
    color: var(--text-tertiary);
  }

  .btn-save {
    font-size: 11px;
    font-weight: 600;
    background: var(--accent);
    color: var(--white);
    padding: 4px 10px;
    border-radius: 8px;
    transition: opacity var(--transition-fast);
    white-space: nowrap;
  }

  .btn-save:hover:not(:disabled) {
    opacity: 0.85;
  }

  .btn-save:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn-cancel {
    font-size: 14px;
    color: var(--text-tertiary);
    padding: 2px 4px;
    transition: color var(--transition-fast);
  }

  .btn-cancel:hover {
    color: var(--text-primary);
  }

  .saved-msg {
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
  }
</style>
