<script lang="ts">
  import { documentState } from '../../state/documents.svelte.js';
  import { closeQuickOpen } from '../../state/ui.svelte.js';
  import { navigateToDoc } from '../../navigation/workspace-router.svelte.js';
  import DsModal from '../../design-system/DsModal.svelte';

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

<DsModal
  closeLabel="close quick open"
  label="Quick open"
  onclose={closeQuickOpen}
  panelTestId="quick-open-panel"
>
  <div class="quick-open">
    <input
      aria-activedescendant={filtered[selectedIndex.value] ? `quick-open-option-${filtered[selectedIndex.value]!.id}` : undefined}
      aria-controls="quick-open-results"
      aria-label="Search notes"
      aria-autocomplete="list"
      aria-expanded="true"
      bind:this={inputEl}
      bind:value={query}
      class="search-input"
      placeholder="search notes..."
      role="combobox"
      type="text"
      onkeydown={handleKeydown}
    />

    <div class="results" id="quick-open-results" role="listbox">
      {#each filtered as doc, i (doc.id)}
        <button
          aria-selected={i === selectedIndex.value}
          class="result-item"
          class:selected={i === selectedIndex.value}
          id={`quick-open-option-${doc.id}`}
          onclick={() => void select(doc.id)}
          role="option"
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
</DsModal>

<style>
  .quick-open {
    display: flex;
    flex-direction: column;
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
