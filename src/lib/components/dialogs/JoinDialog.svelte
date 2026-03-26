<script lang="ts">
  import { inviteState, acceptInvite, closeJoinDialog } from '../../state/invite.svelte.js';
  import { Loader2 } from 'lucide-svelte';
  import { navigateToProject } from '../../navigation/workspace-router.svelte.js';

  let passphrase = $state('');
  let ownerPeerId = $state('');
  let inputEl: HTMLInputElement;

  function handleSubmit() {
    if (!passphrase.trim() || !ownerPeerId.trim()) return;
    void acceptInvite(passphrase, ownerPeerId);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') closeJoinDialog();
    if (e.key === 'Enter' && passphrase.trim() && ownerPeerId.trim() && !inviteState.accepting) {
      e.preventDefault();
      handleSubmit();
    }
  }

  function handleOpenProject() {
    if (inviteState.acceptResult) {
      void navigateToProject(inviteState.acceptResult.projectName);
    }
    closeJoinDialog();
  }

  $effect(() => {
    inputEl?.focus();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
<div class="backdrop" onclick={closeJoinDialog}>
  <div
    class="panel"
    data-testid="join-dialog"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-labelledby="join-title"
    onclick={(e) => e.stopPropagation()}
  >
    {#if inviteState.acceptResult}
      <div class="panel-header">
        <h3 id="join-title">joined {inviteState.acceptResult.projectName}</h3>
      </div>
      <div class="panel-body">
        <p class="hint">you are now an {inviteState.acceptResult.role} on this project</p>
        <div class="actions">
          <button class="btn-primary" data-testid="join-open-project" onclick={handleOpenProject}>open project</button>
          <button class="btn-muted" onclick={closeJoinDialog}>close</button>
        </div>
      </div>
    {:else}
      <div class="panel-header">
        <h3 id="join-title">join a project</h3>
      </div>
      <div class="panel-body">
        <div class="field">
          <label class="field-label" for="passphrase-input">invite code</label>
          <input
            bind:this={inputEl}
            id="passphrase-input"
            data-testid="join-passphrase-input"
            class="text-input"
            type="text"
            placeholder="tiger-marble-ocean-violet-canyon-frost"
            bind:value={passphrase}
            disabled={inviteState.accepting}
          />
          <span class="field-hint">spaces or hyphens both work</span>
        </div>

        <div class="field">
          <label class="field-label" for="peerid-input">owner's peer ID</label>
          <input
            id="peerid-input"
            data-testid="join-peer-id-input"
            class="text-input mono"
            type="text"
            placeholder="paste the owner's peer ID"
            bind:value={ownerPeerId}
            disabled={inviteState.accepting}
          />
        </div>

        {#if inviteState.acceptError}
          <div class="error-row" data-testid="join-error">{inviteState.acceptError}</div>
        {/if}

        <div class="actions">
          {#if inviteState.accepting}
            <button class="btn-primary" disabled>
              <Loader2 size={13} strokeWidth={1.5} class="spin" />
              connecting...
            </button>
          {:else}
              <button
                class="btn-primary"
                data-testid="join-submit"
                onclick={handleSubmit}
                disabled={!passphrase.trim() || !ownerPeerId.trim()}
              >join</button>
          {/if}
          <button class="btn-muted" onclick={closeJoinDialog} disabled={inviteState.accepting}>cancel</button>
        </div>
      </div>
    {/if}
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
    padding: 16px 24px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .panel-header h3 {
    font-size: 16px;
    font-weight: 600;
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

  .field {
    margin-bottom: 14px;
  }

  .field-label {
    display: block;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-tertiary);
    margin-bottom: 6px;
  }

  .text-input {
    width: 100%;
    padding: 11px 14px;
    font-family: var(--font-body);
    font-size: 14px;
    color: var(--text-primary);
    background: var(--surface-hover);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    outline: none;
    transition: border-color var(--transition-fast);
  }

  .field-hint {
    display: block;
    font-size: 11px;
    color: var(--text-tertiary);
    margin-top: 4px;
  }

  .text-input:focus {
    border-color: var(--accent);
  }

  .text-input::placeholder {
    color: var(--text-tertiary);
  }

  .text-input.mono {
    font-size: 12px;
    font-family: var(--font-mono);
  }

  .text-input:disabled {
    opacity: 0.5;
  }

  .error-row {
    font-size: 14px;
    color: #a04130;
    padding: 11px 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 12px;
    border-top: 1px solid var(--border-subtle);
    margin-top: 8px;
  }

  .btn-primary {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
    background: var(--accent);
    color: white;
    padding: 7px 16px;
    border-radius: 8px;
    transition: opacity var(--transition-fast);
  }

  .btn-primary:hover { opacity: 0.85; }
  .btn-primary:disabled { opacity: 0.5; cursor: default; }

  .btn-primary :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .btn-muted {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    padding: 7px 14px;
    border-radius: 8px;
    transition: background var(--transition-fast);
  }

  .btn-muted:hover { background: var(--surface-hover); }
  .btn-muted:disabled { opacity: 0.5; }
</style>
