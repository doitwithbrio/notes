<script lang="ts">
  import { inviteState, acceptInvite, closeJoinDialog } from '../../state/invite.svelte.js';
  import { Loader2 } from 'lucide-svelte';
  import { navigateToProject } from '../../navigation/workspace-router.svelte.js';
  import DsButton from '../../design-system/DsButton.svelte';
  import DsInput from '../../design-system/DsInput.svelte';
  import DsModal from '../../design-system/DsModal.svelte';

  let passphrase = $state('');
  let ownerPeerId = $state('');
  let inputEl = $state<HTMLInputElement | null>(null);

  function handleSubmit() {
    if (!passphrase.trim() || !ownerPeerId.trim()) return;
    void acceptInvite(passphrase, ownerPeerId);
  }

  function handleSubmitEvent(event: SubmitEvent) {
    event.preventDefault();
    if (inviteState.accepting) return;
    handleSubmit();
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

<DsModal
  closeLabel="close join dialog"
  labelledBy="join-title"
  onclose={closeJoinDialog}
  panelTestId="join-dialog"
>
    {#if inviteState.acceptResult}
      <div class="panel-header">
        <h3 id="join-title">joined {inviteState.acceptResult.projectName}</h3>
      </div>
      <div class="panel-body">
        <p class="hint">you are now an {inviteState.acceptResult.role} on this project</p>
        <div class="actions">
          <DsButton testId="join-open-project" onclick={handleOpenProject} variant="primary">open project</DsButton>
          <DsButton onclick={closeJoinDialog} variant="secondary">close</DsButton>
        </div>
      </div>
    {:else}
      <div class="panel-header">
        <h3 id="join-title">join a project</h3>
      </div>
      <form class="panel-body" onsubmit={handleSubmitEvent}>
        <DsInput
          bind:inputRef={inputEl}
          bind:value={passphrase}
          aria-invalid={inviteState.acceptError ? 'true' : undefined}
          describedBy={inviteState.acceptError ? 'join-error' : undefined}
          testId="join-passphrase-input"
          disabled={inviteState.accepting}
          hint="spaces or hyphens both work"
          id="passphrase-input"
          label="invite code"
          placeholder="tiger-marble-ocean-violet-canyon-frost"
          type="text"
        />

        <DsInput
          bind:value={ownerPeerId}
          aria-invalid={inviteState.acceptError ? 'true' : undefined}
          describedBy={inviteState.acceptError ? 'join-error' : undefined}
          testId="join-peer-id-input"
          disabled={inviteState.accepting}
          id="peerid-input"
          label="owner's peer ID"
          mono
          placeholder="paste the owner's peer ID"
          type="text"
        />

        {#if inviteState.acceptError}
          <div class="error-row" data-testid="join-error" id="join-error" role="alert">{inviteState.acceptError}</div>
        {/if}

        <div class="actions">
          {#if inviteState.accepting}
            <DsButton disabled variant="primary">
              <Loader2 size={13} strokeWidth={1.5} class="spin" />
              connecting...
            </DsButton>
          {:else}
              <DsButton
                testId="join-submit"
                disabled={!passphrase.trim() || !ownerPeerId.trim()}
                type="submit"
                variant="primary"
              >join</DsButton>
          {/if}
          <DsButton disabled={inviteState.accepting} onclick={closeJoinDialog} variant="secondary">cancel</DsButton>
        </div>
      </form>
    {/if}
</DsModal>

<style>
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

  .error-row {
    font-size: 14px;
    color: var(--status-danger-fg);
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

  .actions :global(.ds-button .spin) {
    animation: spin var(--motion-spin);
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

</style>
