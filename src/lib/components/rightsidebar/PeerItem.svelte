<script lang="ts">
  import type { Peer } from '../../types/index.js';
  import { X } from 'lucide-svelte';

  let {
    peer,
    isOwner = false,
    onremove,
  }: {
    peer: Peer;
    isOwner?: boolean;
    onremove?: () => void;
  } = $props();

  let confirmingRemove = $state(false);
</script>

<div class="peer-row" data-testid={`peer-row-${peer.id}`} data-state={peer.online ? 'online' : 'offline'} class:offline={!peer.online} title="{peer.alias} · {peer.online ? 'online' : 'offline'}{peer.role ? ` · ${peer.role}` : ''}">
  <span class="peer-dot" class:offline-dot={!peer.online} style={peer.online ? `background: ${peer.cursorColor}` : ''}></span>
  <span class="peer-name">{peer.alias}</span>
  {#if peer.role}
    <span class="peer-role">{peer.role}</span>
  {/if}
  {#if isOwner && onremove && peer.role !== 'owner'}
    {#if confirmingRemove}
      <button class="confirm-remove" data-testid={`peer-remove-confirm-${peer.id}`} onclick={() => { onremove(); confirmingRemove = false; }}>remove?</button>
      <button class="cancel-remove" data-testid={`peer-remove-cancel-${peer.id}`} onclick={() => (confirmingRemove = false)}>no</button>
    {:else}
      <button class="remove-btn" data-testid={`peer-remove-trigger-${peer.id}`} onclick={() => (confirmingRemove = true)} aria-label="remove peer">
        <X size={11} strokeWidth={1.5} />
      </button>
    {/if}
  {/if}
</div>

<style>
  .peer-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    border-radius: 6px;
    transition: background var(--transition-fast);
    cursor: default;
  }

  .peer-row:hover {
    background: var(--surface-hover);
  }

  .peer-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .offline-dot {
    background: var(--text-tertiary);
  }

  .peer-name {
    font-size: 13px;
    font-weight: 450;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .peer-row.offline .peer-name {
    color: var(--text-tertiary);
  }

  .peer-role {
    font-size: 10px;
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  .remove-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    color: var(--text-tertiary);
    border-radius: 4px;
    opacity: 0;
    transition: opacity var(--transition-fast), color var(--transition-fast);
  }

  .peer-row:hover .remove-btn {
    opacity: 1;
  }

  .remove-btn:hover {
    color: var(--text-primary);
  }

  .confirm-remove {
    font-size: 10px;
    font-weight: 600;
    color: var(--danger-fg);
    padding: 1px 6px;
    border-radius: 4px;
    flex-shrink: 0;
  }

  .confirm-remove:hover {
    background: var(--danger-bg);
  }

  .cancel-remove {
    font-size: 10px;
    color: var(--text-tertiary);
    padding: 1px 4px;
    flex-shrink: 0;
  }
</style>
