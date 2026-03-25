<script lang="ts">
  import { uiState, toggleRightSidebar } from '../../state/ui.svelte.js';
  import { PanelRightClose, PanelRightOpen, Users, CheckSquare, Clock } from 'lucide-svelte';
  import PeersSection from './PeersSection.svelte';
  import TodosSection from './TodosSection.svelte';
  import HistorySection from './HistorySection.svelte';

  const collapsed = $derived(!uiState.rightSidebarOpen);
</script>

<aside class="right-sidebar" class:collapsed>
  <div class="right-sidebar-header">
    <button
      class="collapse-toggle"
      onclick={toggleRightSidebar}
      aria-label={collapsed ? 'expand panel' : 'collapse panel'}
    >
      {#if collapsed}
        <PanelRightOpen size={14} strokeWidth={1.5} />
      {:else}
        <PanelRightClose size={14} strokeWidth={1.5} />
      {/if}
    </button>
  </div>

  {#if collapsed}
    <div class="collapsed-icons">
      <button class="collapsed-icon-btn" onclick={toggleRightSidebar} aria-label="peers" title="peers">
        <Users size={15} strokeWidth={1.5} />
      </button>
      <button class="collapsed-icon-btn" onclick={toggleRightSidebar} aria-label="todos" title="todos">
        <CheckSquare size={15} strokeWidth={1.5} />
      </button>
      <button class="collapsed-icon-btn" onclick={toggleRightSidebar} aria-label="history" title="history">
        <Clock size={15} strokeWidth={1.5} />
      </button>
    </div>
  {:else}
    <div class="right-sidebar-scroll">
      <PeersSection />
      <TodosSection />
      <HistorySection />
    </div>
  {/if}
</aside>

<style>
  .right-sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .right-sidebar:not(.collapsed) {
    min-width: var(--right-sidebar-width);
  }

  .right-sidebar-header {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    height: 44px;
    flex-shrink: 0;
    padding: 0 14px;
  }

  .collapsed .right-sidebar-header {
    justify-content: center;
    padding: 0 10px;
  }

  .collapse-toggle {
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

  .collapse-toggle:hover {
    color: var(--text-primary);
    background: var(--surface-hover);
  }

  .collapsed-icons {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 4px 0;
  }

  .collapsed-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    color: var(--text-primary);
    border-radius: 6px;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .collapsed-icon-btn:hover {
    color: var(--text-primary);
    background: var(--surface-hover);
  }

  .right-sidebar-scroll {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
