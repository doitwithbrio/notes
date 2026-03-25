<script lang="ts">
  import type { BackendHistorySession } from '../../types/index.js';
  import { historyState } from '../../state/history.svelte.js';
  import { CURSOR_COLORS } from '../../types/index.js';
  import { formatShortTime, formatDuration } from '../../utils/time.js';

  let {
    session,
    selected = false,
    localActorId = '',
    onclick,
  }: {
    session: BackendHistorySession;
    selected?: boolean;
    localActorId?: string;
    onclick?: () => void;
  } = $props();

  function colorForActor(actor: string): string {
    let hash = 0;
    for (let i = 0; i < actor.length; i++) {
      hash = (hash * 31 + actor.charCodeAt(i)) | 0;
    }
    return CURSOR_COLORS[Math.abs(hash) % CURSOR_COLORS.length]!;
  }

  const isSelf = $derived(session.actor === localActorId);
  const dotColor = $derived(isSelf ? 'var(--accent)' : colorForActor(session.actor));

  const displayName = $derived.by(() => {
    if (isSelf) return 'you';
    const alias = historyState.actorAliases[session.actor];
    if (alias) return alias;
    return session.actor.slice(0, 4) + '...';
  });

  const timeStr = $derived(session.startedAt > 0 ? formatShortTime(session.startedAt) : '');
  const durationStr = $derived(
    session.startedAt > 0 && session.endedAt > 0
      ? formatDuration(session.startedAt, session.endedAt)
      : '',
  );
</script>

<button class="session-item" class:selected onclick={onclick} type="button">
  <div class="row-1">
    <span class="dot" style="background: {dotColor}"></span>
    <span class="author">{displayName} edited</span>
    {#if timeStr}<span class="time">{timeStr}</span>{/if}
  </div>
  <div class="row-2">
    {session.changeCount} change{session.changeCount !== 1 ? 's' : ''}{#if durationStr} · {durationStr}{/if}
  </div>
</button>

<style>
  .session-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: pointer;
    transition: background var(--transition-fast);
    text-align: left;
  }

  .session-item:hover {
    background: rgba(182, 141, 94, 0.06);
  }

  .session-item.selected {
    background: var(--surface-active);
    border-left: 2px solid var(--accent);
    padding-left: 8px;
  }

  .row-1 {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .author {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .time {
    font-size: 11px;
    color: rgba(0, 0, 0, 0.30);
    flex-shrink: 0;
  }

  .row-2 {
    font-size: 11px;
    color: rgba(0, 0, 0, 0.35);
    padding-left: 13px;
  }
</style>
