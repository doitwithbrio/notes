<!--
  UpdateBanner — a slim notification bar at the top of the editor panel.

  Shows only when there's update activity:
  - "Update v1.2.0 available" with Install button
  - Download progress bar
  - "Installing..." spinner
  - "Restarting..." confirmation
  - Error with retry button

  Matches the app appearance tokens so light/dark modes shift together.
-->
<script lang="ts">
  import {
    updateState,
    installUpdate,
    dismissUpdate,
    checkForUpdate,
  } from '../state/updates.svelte.js';
  import { Download, X, RefreshCw, CheckCircle, Loader } from 'lucide-svelte';

  const status = $derived(updateState.status);
  const info = $derived(updateState.info);
  const progress = $derived(updateState.progress);
  const error = $derived(updateState.error);
  const updaterEnabled = $derived(updateState.updaterEnabled);

  // Only render the banner when there's something to show
  const visible = $derived(
    updaterEnabled &&
      (status === 'available' ||
        status === 'downloading' ||
        status === 'installing' ||
        status === 'ready' ||
        status === 'error'),
  );

  function handleInstall() {
    void installUpdate();
  }

  function handleDismiss() {
    dismissUpdate();
  }

  function handleRetry() {
    void checkForUpdate(false);
  }
</script>

{#if visible}
  <div class="update-banner" class:error={status === 'error'} role="status" aria-live="polite">
    <div class="update-content">
      {#if status === 'available' && info}
        <!-- Update found: show version and install button -->
        <Download size={13} strokeWidth={1.8} />
        <span class="update-text">
          Update <strong>v{info.version}</strong> available
        </span>
        <button class="update-action" onclick={handleInstall}>Install & restart</button>
        <button class="update-dismiss" onclick={handleDismiss} aria-label="dismiss">
          <X size={12} strokeWidth={2} />
        </button>
      {:else if status === 'downloading'}
        <!-- Downloading: show spinner and progress bar -->
        <Loader size={13} strokeWidth={1.8} class="spinning" />
        <span class="update-text">
          Downloading{#if progress > 0}... {progress}%{/if}
        </span>
        {#if updateState.totalBytes > 0}
          <div
            class="progress-bar"
            role="progressbar"
            aria-label="update download progress"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={progress}
          >
            <div class="progress-fill" style="width: {progress}%"></div>
          </div>
        {/if}
      {:else if status === 'installing'}
        <!-- Installing: extracting archive and replacing .app -->
        <Loader size={13} strokeWidth={1.8} class="spinning" />
        <span class="update-text">Installing update...</span>
      {:else if status === 'ready'}
        <!-- Done: about to relaunch -->
        <CheckCircle size={13} strokeWidth={1.8} />
        <span class="update-text">Update installed. Restarting...</span>
      {:else if status === 'error'}
        <!-- Error: show message with retry -->
        <span class="update-text">Update failed: {error}</span>
        <button class="update-action" onclick={handleRetry}>
          <RefreshCw size={11} strokeWidth={2} />
          Retry
        </button>
        <button class="update-dismiss" onclick={handleDismiss} aria-label="dismiss">
          <X size={12} strokeWidth={2} />
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .update-banner {
    background: var(--accent);
    color: var(--accent-contrast);
    padding: 5px 14px;
    font-size: 12px;
    display: flex;
    align-items: center;
    flex-shrink: 0;
    z-index: 50;
  }

  .update-banner.error {
    background: var(--danger-solid);
  }

  .update-content {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }

  .update-text {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .update-text strong {
    font-weight: 600;
  }

  .update-action {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 10px;
    border-radius: 4px;
    background: var(--banner-action-bg);
    color: var(--accent-contrast);
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    transition: background 0.15s;
    flex-shrink: 0;
  }

  .update-action:hover {
    background: var(--banner-action-hover);
  }

  .update-dismiss {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 3px;
    color: var(--banner-dismiss-fg);
    flex-shrink: 0;
    transition: color 0.15s, background 0.15s;
  }

  .update-dismiss:hover {
    color: var(--accent-contrast);
    background: var(--banner-dismiss-hover);
  }

  .progress-bar {
    width: 80px;
    height: 4px;
    border-radius: 2px;
    background: var(--progress-track);
    overflow: hidden;
    flex-shrink: 0;
  }

  .progress-fill {
    height: 100%;
    background: var(--progress-fill);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  :global(.spinning) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>
