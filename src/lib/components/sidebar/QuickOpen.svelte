<script lang="ts">
  import { documentState, setActiveDoc } from '../../state/documents.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import { modKey } from '../../utils/platform.js';

  let query = $state('');
  let inputEl: HTMLInputElement;

  const filtered = $derived(
    query.trim()
      ? documentState.docs.filter((d) =>
          d.title.toLowerCase().includes(query.toLowerCase()),
        )
      : documentState.docs,
  );

  const selectedIndex = $state({ value: 0 });

  function select(docId: string) {
    setActiveDoc(docId);
    uiState.quickOpenVisible = false;
    query = '';
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
      select(filtered[selectedIndex.value]!.id);
    }
  }

  $effect(() => {
    // Reset selection when query changes
    query;
    selectedIndex.value = 0;
  });

  $effect(() => {
    inputEl?.focus();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="quick-open-backdrop" onclick={() => (uiState.quickOpenVisible = false)} onkeydown={handleKeydown}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="quick-open" onclick={(e) => e.stopPropagation()}>
    <div class="search-row">
      <input
        bind:this={inputEl}
        bind:value={query}
        class="search-input"
        placeholder="Search notes... ({modKey}+P)"
        type="text"
        onkeydown={handleKeydown}
      />
    </div>

    <div class="results">
      {#each filtered as doc, i (doc.id)}
        <button
          class="result-item"
          class:selected={i === selectedIndex.value}
          onclick={() => select(doc.id)}
        >
          <span class="result-title">{doc.title}</span>
          <span class="result-path">{doc.path}</span>
        </button>
      {/each}

      {#if filtered.length === 0}
        <div class="no-results">No matching notes</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .quick-open-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    justify-content: center;
    padding-top: 80px;
    background: none;
  }

  .quick-open {
    width: 480px;
    max-height: 360px;
    background: var(--white);
    border: 2px solid var(--black);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .search-row {
    padding: 8px;
    border-bottom: var(--border);
  }

  .search-input {
    width: 100%;
    padding: 8px 12px;
    font-size: 15px;
    border: none;
    outline: none;
  }

  .results {
    flex: 1;
    overflow-y: auto;
  }

  .result-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 8px 16px;
    text-align: left;
    font-size: 13px;
    color: var(--black);
    background: var(--white);
  }

  .result-item:hover,
  .result-item.selected {
    background: var(--accent);
    color: var(--white);
  }

  .result-title {
    font-weight: 500;
  }

  .result-path {
    font-size: 11px;
  }

  .result-item:hover .result-path,
  .result-item.selected .result-path {
    color: var(--white);
  }

  .no-results {
    padding: 16px;
    text-align: center;
    font-size: 13px;
    color: var(--black);
  }
</style>
