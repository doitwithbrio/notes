<script lang="ts">
  import { inviteState, closeShareDialog, generateInvite } from '../../state/invite.svelte.js';
  import { getProject } from '../../state/projects.svelte.js';
  import { Copy, Check, Loader2 } from 'lucide-svelte';

  const project = $derived(getProject(inviteState.shareProjectId));
  const invite = $derived(inviteState.activeInvite);

  let copiedPassphrase = $state(false);
  let copiedPeerId = $state(false);

  // Countdown timer
  let remainingSeconds = $state(0);
  let timerInterval: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    if (invite?.expiresAt) {
      const updateTimer = () => {
        const remaining = Math.max(0, Math.floor((new Date(invite.expiresAt).getTime() - Date.now()) / 1000));
        remainingSeconds = remaining;
        if (remaining <= 0 && timerInterval) {
          clearInterval(timerInterval);
          timerInterval = null;
        }
      };
      updateTimer();
      timerInterval = setInterval(updateTimer, 1000);
      return () => {
        if (timerInterval) clearInterval(timerInterval);
      };
    }
  });

  const timerLabel = $derived(() => {
    const mins = Math.floor(remainingSeconds / 60);
    const secs = remainingSeconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  });

  const expired = $derived(invite != null && remainingSeconds <= 0);

  async function copyToClipboard(text: string, which: 'passphrase' | 'peerId') {
    try {
      await navigator.clipboard.writeText(text);
      if (which === 'passphrase') {
        copiedPassphrase = true;
        setTimeout(() => (copiedPassphrase = false), 2000);
      } else {
        copiedPeerId = true;
        setTimeout(() => (copiedPeerId = false), 2000);
      }
    } catch {
      // Clipboard API may not be available
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') closeShareDialog();
  }

  function handleGenerate() {
    if (inviteState.shareProjectId) {
      void generateInvite(inviteState.shareProjectId, inviteState.inviteRole);
    }
  }

  function setRole(role: 'editor' | 'viewer') {
    inviteState.inviteRole = role;
    inviteState.activeInvite = null;
    inviteState.generateError = null;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
<div class="backdrop" onclick={closeShareDialog}>
  <div
    class="panel"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="share-title"
    onclick={(e) => e.stopPropagation()}
  >
    <div class="panel-header">
      <h3 id="share-title">share {project?.name ?? 'project'}</h3>
      <div class="role-switcher">
        <button class="role-btn" class:active={inviteState.inviteRole === 'editor'} onclick={() => setRole('editor')}>editor</button>
        <button class="role-btn" class:active={inviteState.inviteRole === 'viewer'} onclick={() => setRole('viewer')}>viewer</button>
      </div>
    </div>

    <div class="panel-body">
      {#if inviteState.generating}
        <div class="status-row">
          <Loader2 size={14} strokeWidth={1.5} class="spin" />
          <span>generating invite code...</span>
        </div>
      {:else if inviteState.generateError}
        <div class="error-row">{inviteState.generateError}</div>
        <button class="action-row" onclick={handleGenerate}>try again</button>
      {:else if invite}
        <p class="hint">share this code and your peer ID with the person you want to invite</p>
        <p class="hint-subtle">this invite is single-use, expires in 10 minutes, and stops working if you close the app</p>

        <div class="field-row">
          <span class="field-label">invite code</span>
          <div class="code-box">
            <span class="passphrase">{invite.passphrase}</span>
            <button class="copy-btn" onclick={() => copyToClipboard(invite.passphrase, 'passphrase')}>
              {#if copiedPassphrase}
                <Check size={13} strokeWidth={1.5} />
              {:else}
                <Copy size={13} strokeWidth={1.5} />
              {/if}
            </button>
          </div>
        </div>

        <div class="field-row">
          <span class="field-label">your peer ID</span>
          <div class="code-box small">
            <span class="peer-id">{inviteState.localPeerId ?? invite.peerId}</span>
            <button class="copy-btn" onclick={() => copyToClipboard(inviteState.localPeerId ?? invite.peerId, 'peerId')}>
              {#if copiedPeerId}
                <Check size={13} strokeWidth={1.5} />
              {:else}
                <Copy size={13} strokeWidth={1.5} />
              {/if}
            </button>
          </div>
        </div>

        <div class="footer-row">
          {#if expired}
            <span class="expired">invite expired</span>
          {:else}
            <span class="timer">expires in {timerLabel()}</span>
          {/if}
          <div class="footer-actions">
            <button class="btn-accent" onclick={handleGenerate}>new code</button>
            <button class="btn-muted" onclick={closeShareDialog}>done</button>
          </div>
        </div>
      {:else}
        <p class="hint">choose a role, then generate a one-time invite code</p>
        <button class="action-row" onclick={handleGenerate}>generate invite</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    justify-content: center;
    padding-top: 100px;
    background: var(--overlay-backdrop);
    backdrop-filter: blur(2px);
    -webkit-backdrop-filter: blur(2px);
    animation: fadeIn 150ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .panel {
    width: 560px;
    max-height: 440px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 16px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: panelIn 200ms var(--ease-out-expo);
  }

  @keyframes panelIn {
    from { opacity: 0; transform: scale(0.97) translateY(-4px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .panel-header h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .role-switcher {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    background: var(--surface-hover);
    border-radius: 8px;
  }

  .role-btn {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-tertiary);
    padding: 4px 10px;
    border-radius: 6px;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .role-btn.active {
    background: var(--surface);
    color: var(--text-primary);
  }

  .panel-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px 24px;
  }

  .hint {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.5;
    margin-bottom: 16px;
  }

  .hint-subtle {
    font-size: 11px;
    color: var(--text-tertiary);
    line-height: 1.4;
    margin-top: -10px;
    margin-bottom: 14px;
  }

  .status-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 20px 0;
    font-size: 14px;
    color: var(--text-secondary);
  }

  .status-row :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .error-row {
    font-size: 14px;
    color: #a04130;
    padding: 11px 0;
  }

  .action-row {
    display: block;
    width: 100%;
    padding: 11px 0;
    font-size: 14px;
    font-weight: 500;
    color: var(--accent);
    text-align: left;
    transition: opacity var(--transition-fast);
  }

  .action-row:hover {
    opacity: 0.7;
  }

  .field-row {
    margin-bottom: 14px;
  }

  .field-label {
    display: block;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-tertiary);
    margin-bottom: 6px;
  }

  .code-box {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--surface-hover);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    padding: 11px 14px;
  }

  .code-box.small {
    padding: 8px 14px;
  }

  .passphrase {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 15px;
    font-weight: 500;
    color: var(--text-primary);
    letter-spacing: 0.02em;
    word-break: break-all;
  }

  .peer-id {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
    word-break: break-all;
    line-height: 1.4;
  }

  .copy-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    color: var(--text-tertiary);
    border-radius: 6px;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .copy-btn:hover {
    color: var(--text-primary);
    background: var(--surface-active);
  }

  .footer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-top: 12px;
    border-top: 1px solid var(--border-subtle);
    margin-top: 8px;
  }

  .footer-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .timer {
    font-size: 12px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }

  .expired {
    font-size: 12px;
    color: #a04130;
    font-weight: 500;
  }

  .btn-accent {
    font-size: 13px;
    font-weight: 500;
    color: var(--accent);
    padding: 6px 12px;
    border-radius: 8px;
    transition: background var(--transition-fast);
  }

  .btn-accent:hover {
    background: var(--surface-hover);
  }

  .btn-muted {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    padding: 6px 12px;
    border-radius: 8px;
    transition: background var(--transition-fast);
  }

  .btn-muted:hover {
    background: var(--surface-hover);
  }
</style>
