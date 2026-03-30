<script lang="ts">
  import { documentState } from '../../state/documents.svelte.js';
  import { closeQuickOpen } from '../../state/ui.svelte.js';
  import { navigateToDoc } from '../../navigation/workspace-router.svelte.js';

  let query = $state('');
  let inputEl = $state<HTMLInputElement | null>(null);
  let opening = $state(false);

  const searchableDocs = $derived(
    documentState.docs.map((doc) => ({
      doc,
      titleLower: doc.title.toLowerCase(),
    })),
  );

  const filtered = $derived.by(() => {
    const normalized = query.trim().toLowerCase();
    const matches = normalized
      ? searchableDocs.filter(({ titleLower }) => titleLower.includes(normalized))
      : searchableDocs;
    return matches.slice(0, 100).map(({ doc }) => doc);
  });

  const selectedIndex = $state({ value: 0 });
  const loading = $derived(
    documentState.loading || documentState.loadingProjectIds.length > 0,
  );

  async function select(docId: string) {
    if (opening) return;
    const doc = searchableDocs.find((entry) => entry.doc.id === docId)?.doc;
    if (!doc) return;
    opening = true;
    try {
      await navigateToDoc(doc.projectId, doc.id);
      closeQuickOpen();
      query = '';
    } catch (error) {
      console.error('Failed to open note from quick open:', error);
    } finally {
      opening = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIndex.value = Math.min(selectedIndex.value + 1, filtered.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIndex.value = Math.max(selectedIndex.value - 1, 0);
    } else if (e.key === 'Enter' && filtered[selectedIndex.value]) {
      e.preventDefault();
      void select(filtered[selectedIndex.value]!.id);
    }
  }

  $effect(() => {
    query;
    selectedIndex.value = 0;
  });

  $effect(() => {
    inputEl?.focus();
  });
</script>

<div class="quick-open-shell">
  <button
    aria-label="close quick open"
    class="quick-open-backdrop"
    onclick={closeQuickOpen}
    tabindex="-1"
    type="button"
  ></button>
  <div class="quick-open" data-testid="quick-open-panel">
    <input
      bind:this={inputEl}
      bind:value={query}
      class="search-input"
      placeholder="search notes..."
      type="text"
      onkeydown={handleKeydown}
    />

    <div class="results">
      {#each filtered as doc, i (doc.id)}
        <button
          class="result-item"
          class:selected={i === selectedIndex.value}
          onclick={() => void select(doc.id)}
        >
          <span class="result-title">{doc.title.toLowerCase()}</span>
          <span class="result-path">{doc.path}</span>
        </button>
      {/each}

      {#if filtered.length === 0}
        <div class="no-results">
          {loading && !query.trim() ? 'loading notes...' : 'no matching notes'}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .quick-open-shell {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    justify-content: center;
    padding-top: 100px;
  }

  .quick-open-backdrop {
    position: absolute;
    inset: 0;
    border: none;
    padding: 0;
    background: var(--overlay-backdrop);
    backdrop-filter: blur(2px);
    -webkit-backdrop-filter: blur(2px);
    animation: fadeIn 150ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .quick-open {
    position: relative;
    z-index: 1;
    width: 560px;
    max-height: 440px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 16px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: quickOpenIn 200ms var(--ease-out-expo);
  }

  @keyframes quickOpenIn {
    from {
      opacity: 0;
      transform: scale(0.97) translateY(-4px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }

  .search-input {
    padding: 16px 24px;
    font-size: 16px;
    font-weight: 400;
    border: none;
    outline: none;
    background: transparent;
  }

  .search-input::placeholder {
    color: var(--text-tertiary);
  }

  .results {
    flex: 1;
    overflow-y: auto;
    border-top: 1px solid var(--border-subtle);
  }

  .result-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 11px 24px;
    text-align: left;
    font-size: 14px;
    color: var(--text-primary);
    background: transparent;
    transition: background var(--transition-fast);
  }

  .result-item:hover {
    background: var(--surface-hover);
  }

  .result-item.selected {
    background: var(--surface-active);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .result-title {
    font-weight: 450;
  }

  .result-path {
    font-size: 12px;
    font-weight: 400;
    color: var(--text-tertiary);
  }

  .no-results {
    padding: 24px;
    text-align: center;
    font-size: 14px;
    font-weight: 400;
    color: var(--text-tertiary);
  }
</style>
