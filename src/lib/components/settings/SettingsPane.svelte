<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { Download, LoaderCircle, RefreshCw, X } from 'lucide-svelte';
  import { settingsState, loadSettings, saveSettings } from '../../state/settings.svelte.js';
  import { getEffectiveThemeLabel, syncAppearancePreference } from '../../state/appearance.svelte.js';
  import {
    checkForUpdate,
    ensureCurrentVersion,
    installUpdate,
    updateState,
  } from '../../state/updates.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import { ACCENT_COLORS, type AppSettings, type ThemeMode } from '../../types/index.js';

  let relayInput = $state('');
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let pendingSettings = $state<AppSettings | null>(null);
  const MODE_OPTIONS: ThemeMode[] = ['system', 'light', 'dark'];

  onDestroy(() => {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    if (pendingSettings) {
      void saveSettings(pendingSettings);
      pendingSettings = null;
    }
  });

  // Load settings on mount
  $effect(() => {
    if (!settingsState.settings && !settingsState.loading) {
      void loadSettings();
    }
    void ensureCurrentVersion();
  });

  const updateStatus = $derived(updateState.status);
  const updateInfo = $derived(updateState.info);
  const currentVersion = $derived(updateState.currentVersion);
  const updateProgress = $derived(updateState.progress);
  const lastCheckResult = $derived(updateState.lastCheckResult);

  function formatCheckedAt(timestamp: number | null) {
    if (!timestamp) return 'never';
    return new Date(timestamp).toLocaleString([], {
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
    });
  }

  function handleManualUpdateCheck() {
    void checkForUpdate(false);
  }

  function handleInstallUpdate() {
    void installUpdate();
  }

  function update(patch: Partial<AppSettings>) {
    if (!settingsState.settings) return;
    const next = { ...settingsState.settings, ...patch };
    settingsState.settings = next;
    pendingSettings = next;
    syncAppearancePreference(next.appearance, false);

    // Debounced save
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void saveSettings(next);
      pendingSettings = null;
      saveTimer = null;
    }, 400);
  }

  function updateAppearance(patch: Partial<AppSettings['appearance']>) {
    if (!settingsState.settings) return;
    update({
      appearance: {
        ...settingsState.settings.appearance,
        ...patch,
      },
    });
  }

  function setThemeMode(mode: ThemeMode) {
    updateAppearance({ mode });
  }

  function moveThemeMode(step: number) {
    if (!settingsState.settings) return;
    const currentIndex = MODE_OPTIONS.indexOf(settingsState.settings.appearance.mode);
    const nextIndex = (currentIndex + step + MODE_OPTIONS.length) % MODE_OPTIONS.length;
    setThemeMode(MODE_OPTIONS[nextIndex]!);
  }

  async function handleModeKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
      event.preventDefault();
      moveThemeMode(1);
      await tick();
      (event.currentTarget as HTMLButtonElement)
        .parentElement
        ?.querySelector<HTMLButtonElement>('[role="radio"][aria-checked="true"]')
        ?.focus();
    }
    if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
      event.preventDefault();
      moveThemeMode(-1);
      await tick();
      (event.currentTarget as HTMLButtonElement)
        .parentElement
        ?.querySelector<HTMLButtonElement>('[role="radio"][aria-checked="true"]')
        ?.focus();
    }
  }

  function moveAccent(step: number) {
    if (!settingsState.settings) return;
    const currentIndex = ACCENT_COLORS.findIndex((accent) => accent.id === settingsState.settings?.appearance.accent);
    const nextIndex = (currentIndex + step + ACCENT_COLORS.length) % ACCENT_COLORS.length;
    updateAppearance({ accent: ACCENT_COLORS[nextIndex]!.id });
  }

  async function handleAccentKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
      event.preventDefault();
      moveAccent(1);
      await tick();
      (event.currentTarget as HTMLButtonElement)
        .parentElement
        ?.querySelector<HTMLButtonElement>('[role="radio"][aria-checked="true"]')
        ?.focus();
    }
    if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
      event.preventDefault();
      moveAccent(-1);
      await tick();
      (event.currentTarget as HTMLButtonElement)
        .parentElement
        ?.querySelector<HTMLButtonElement>('[role="radio"][aria-checked="true"]')
        ?.focus();
    }
  }

  function addRelay() {
    const url = relayInput.trim();
    if (!url || !settingsState.settings) return;
    if (settingsState.settings.customRelays.includes(url)) {
      relayInput = '';
      return;
    }
    update({ customRelays: [...settingsState.settings.customRelays, url] });
    relayInput = '';
  }

  function removeRelay(url: string) {
    if (!settingsState.settings) return;
    update({ customRelays: settingsState.settings.customRelays.filter((r) => r !== url) });
  }

  function goBack() {
    uiState.view = 'editor';
  }
</script>

<div class="settings-pane">
  <div class="settings-drag" data-tauri-drag-region>
    <div class="drag-spacer" data-tauri-drag-region></div>
  </div>

  {#if settingsState.loading}
    <div class="settings-loading">loading settings...</div>
  {:else if settingsState.settings}
    {@const s = settingsState.settings}
    <div class="settings-scroll">
      <div class="settings-content">
        <h1 class="settings-title">settings</h1>

        {#if settingsState.error}
          <div class="settings-error" role="status">{settingsState.error}</div>
        {/if}

        <section class="section">
          <h2 class="section-title">profile</h2>
          <label class="field">
            <span class="field-label">display name</span>
            <input
              class="field-input"
              type="text"
              value={s.displayName}
              oninput={(e) => update({ displayName: (e.target as HTMLInputElement).value })}
            />
          </label>
        </section>

        <section class="section">
          <h2 class="section-title">sync</h2>
          <div class="field">
            <span class="field-label">custom relays</span>
            <div class="relay-input-row">
              <input
                class="field-input"
                type="url"
                placeholder="https://relay.example.com"
                bind:value={relayInput}
                onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addRelay(); } }}
              />
              <button class="relay-add-btn" onclick={addRelay}>add</button>
            </div>
            {#if s.customRelays.length > 0}
              <div class="relay-list">
                {#each s.customRelays as relay (relay)}
                  <div class="relay-chip">
                    <span class="relay-url">{relay}</span>
                    <button class="relay-remove" onclick={() => removeRelay(relay)} aria-label="remove relay">
                      <X size={12} strokeWidth={2} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        </section>

        <section class="section">
          <h2 class="section-title">editor</h2>
          <label class="field">
            <span class="field-label">font size</span>
            <input
              class="field-input small"
              type="number"
              min="10"
              max="28"
              value={s.fontSize}
              oninput={(e) => update({ fontSize: Number((e.target as HTMLInputElement).value) || 16 })}
            />
          </label>
          <label class="field row">
            <span class="field-label">auto-save</span>
            <button
              class="toggle"
              class:on={s.autoSave}
              onclick={() => update({ autoSave: !s.autoSave })}
              role="switch"
              aria-checked={s.autoSave}
              aria-label="toggle auto-save"
            >
              <span class="toggle-thumb"></span>
            </button>
          </label>
          {#if s.autoSave}
            <label class="field">
              <span class="field-label">save interval (seconds)</span>
              <input
                class="field-input small"
                type="number"
                min="1"
                max="60"
                value={s.saveIntervalSecs}
                oninput={(e) => update({ saveIntervalSecs: Number((e.target as HTMLInputElement).value) || 5 })}
              />
            </label>
          {/if}
        </section>

        <section class="section">
          <h2 class="section-title">appearance</h2>
          <div class="field">
            <span class="field-label">mode</span>
            <div class="theme-picker" role="radiogroup" aria-label="theme mode">
              {#each [
                { id: 'system', label: 'system' },
                { id: 'light', label: 'light' },
                { id: 'dark', label: 'dark' },
              ] as option (option.id)}
                <button
                  class="theme-option"
                  class:active={s.appearance.mode === option.id}
                  type="button"
                  role="radio"
                  aria-checked={s.appearance.mode === option.id}
                  tabindex={s.appearance.mode === option.id ? 0 : -1}
                  onkeydown={handleModeKeydown}
                  onclick={() => setThemeMode(option.id as ThemeMode)}
                >
                  {option.label}
                </button>
              {/each}
            </div>
            {#if s.appearance.mode === 'system'}
              <span class="field-hint">currently using {getEffectiveThemeLabel()} from your system</span>
            {/if}
          </div>

          <div class="field">
            <span class="field-label">accent</span>
            <div class="accent-picker" role="radiogroup" aria-label="accent color">
              {#each ACCENT_COLORS as accent (accent.id)}
                <button
                  class="accent-option"
                  class:active={s.appearance.accent === accent.id}
                  type="button"
                  role="radio"
                  aria-checked={s.appearance.accent === accent.id}
                  aria-label={accent.label}
                  title={accent.label}
                  tabindex={s.appearance.accent === accent.id ? 0 : -1}
                  onkeydown={handleAccentKeydown}
                  onclick={() => updateAppearance({ accent: accent.id })}
                >
                  <span class="accent-swatch" style="--swatch: {accent.hex}"></span>
                  <span class="accent-name">{accent.label}</span>
                </button>
              {/each}
            </div>
          </div>
        </section>

        <section class="section">
          <h2 class="section-title">updates</h2>
          <div class="update-card">
            <div class="update-row">
              <div>
                <div class="field-label">current version</div>
                <div class="update-meta">{currentVersion || 'loading...'}</div>
              </div>
              <button
                class="update-check-btn"
                type="button"
                disabled={updateStatus === 'checking' || updateStatus === 'downloading' || updateStatus === 'installing'}
                onclick={handleManualUpdateCheck}
              >
                {#if updateStatus === 'checking'}
                  <LoaderCircle size={13} strokeWidth={1.8} class="spinning" />
                  checking...
                {:else}
                  <RefreshCw size={13} strokeWidth={1.8} />
                  check for updates
                {/if}
              </button>
            </div>

            <div class="update-subtle">last checked: {formatCheckedAt(updateState.lastCheckedAt)}</div>

            {#if updateStatus === 'available' && updateInfo}
              <div class="update-panel success" role="status" aria-live="polite">
                <div class="update-panel-head">
                  <div>
                    <div class="update-panel-title">update v{updateInfo.version} is available</div>
                    <div class="update-subtle">you are on v{updateInfo.currentVersion}</div>
                  </div>
                  <button class="update-install-btn" type="button" onclick={handleInstallUpdate}>
                    <Download size={13} strokeWidth={1.8} />
                    install & restart
                  </button>
                </div>
                {#if updateInfo.body}
                  <div class="update-notes">{updateInfo.body}</div>
                {/if}
              </div>
            {:else if updateStatus === 'downloading'}
              <div class="update-panel" role="status" aria-live="polite">
                <div class="update-panel-title">downloading update... {updateProgress}%</div>
                <div
                  class="settings-progress-bar"
                  role="progressbar"
                  aria-label="settings update download progress"
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-valuenow={updateProgress}
                >
                  <div class="settings-progress-fill" style={`width: ${updateProgress}%`}></div>
                </div>
              </div>
            {:else if updateStatus === 'installing'}
              <div class="update-panel" role="status" aria-live="polite">
                <div class="update-panel-title">installing update...</div>
                <div class="update-subtle">the app will relaunch when finished</div>
              </div>
            {:else if updateStatus === 'ready'}
              <div class="update-panel success" role="status" aria-live="polite">
                <div class="update-panel-title">update installed</div>
                <div class="update-subtle">relaunching now...</div>
              </div>
            {:else if updateStatus === 'error' && updateState.error}
              <div class="update-panel error" role="status" aria-live="polite">
                <div class="update-panel-title">could not update the app</div>
                <div class="update-notes">{updateState.error}</div>
              </div>
            {:else if lastCheckResult === 'up-to-date'}
              <div class="update-panel success" role="status" aria-live="polite">
                <div class="update-panel-title">you’re up to date</div>
                <div class="update-subtle">no newer release was found on GitHub</div>
              </div>
            {/if}
          </div>
        </section>

        <section class="section">
          <h2 class="section-title">advanced</h2>
          <label class="field">
            <span class="field-label">large document warning (words)</span>
            <input
              class="field-input small"
              type="number"
              min="1000"
              max="100000"
              step="1000"
              value={s.largeDocWarningWords}
              oninput={(e) => update({ largeDocWarningWords: Number((e.target as HTMLInputElement).value) || 10000 })}
            />
          </label>
          <label class="field">
            <span class="field-label">idle document timeout (seconds, 0 = off)</span>
            <input
              class="field-input small"
              type="number"
              min="0"
              max="3600"
              value={s.idleDocTimeoutSecs}
              oninput={(e) => update({ idleDocTimeoutSecs: Number((e.target as HTMLInputElement).value) || 0 })}
            />
          </label>
        </section>
      </div>
    </div>
  {:else if settingsState.error}
    <div class="settings-loading">{settingsState.error}</div>
  {/if}
</div>

<style>
  .settings-pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .settings-drag {
    height: 44px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    padding: 0 20px;
    -webkit-app-region: drag;
  }

  .drag-spacer {
    flex: 1;
  }

  .settings-loading {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-tertiary);
    font-size: 14px;
  }

  .settings-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 48px 80px;
  }

  .settings-content {
    max-width: 520px;
    margin: 0 auto;
  }

  .settings-title {
    font-family: var(--font-body);
    font-size: 34px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text-primary);
    margin-bottom: 40px;
    line-height: 1.15;
  }

  .settings-error {
    margin-bottom: 20px;
    padding: 10px 12px;
    border: 1px solid var(--danger-border);
    border-radius: 12px;
    background: var(--danger-bg);
    color: var(--danger-fg);
    font-size: 13px;
  }

  .section {
    margin-bottom: 36px;
  }

  .section-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-tertiary);
    margin-bottom: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 16px;
  }

  .field.row {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
  }

  .field-label {
    font-size: 13px;
    font-weight: 450;
    color: var(--text-primary);
  }

  .field-hint {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .field-input {
    width: 100%;
    padding: 9px 12px;
    font-family: var(--font-body);
    font-size: 13px;
    color: var(--text-primary);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    outline: none;
    transition: border-color var(--transition-fast);
  }

  .field-input:focus {
    border-color: var(--accent);
  }

  .field-input.small {
    width: 100px;
  }

  /* Relay input */
  .relay-input-row {
    display: flex;
    gap: 8px;
  }

  .relay-input-row .field-input {
    flex: 1;
  }

  .relay-add-btn {
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 500;
    color: var(--accent);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    transition: background var(--transition-fast), border-color var(--transition-fast);
    white-space: nowrap;
  }

  .relay-add-btn:hover {
    background: var(--surface-hover);
    border-color: var(--accent);
  }

  .relay-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 8px;
  }

  .relay-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
  }

  .relay-url {
    flex: 1;
    font-size: 13px;
    font-family: var(--font-mono);
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .relay-remove {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    color: var(--text-tertiary);
    border-radius: 4px;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .relay-remove:hover {
    color: var(--danger-fg);
    background: var(--danger-bg);
  }

  .update-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: 16px;
  }

  .update-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .update-meta,
  .update-subtle {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .update-check-btn,
  .update-install-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 9px 14px;
    border-radius: 10px;
    border: 1px solid var(--border-subtle);
    background: var(--surface-sidebar);
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast), color var(--transition-fast);
  }

  .update-check-btn:hover,
  .update-install-btn:hover {
    background: var(--surface-hover);
    border-color: var(--accent);
  }

  .update-check-btn:disabled,
  .update-install-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .update-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px;
    border-radius: 12px;
    background: var(--surface-sidebar);
    border: 1px solid var(--border-subtle);
  }

  .update-panel.success {
    border-color: color-mix(in srgb, var(--accent) 35%, var(--border-subtle));
    background: color-mix(in srgb, var(--accent) 10%, var(--surface-sidebar));
  }

  .update-panel.error {
    border-color: var(--danger-border);
    background: var(--danger-bg);
  }

  .update-panel-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .update-panel-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .update-notes {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
    white-space: pre-wrap;
  }

  .settings-progress-bar {
    width: 100%;
    height: 6px;
    border-radius: 999px;
    background: var(--border-subtle);
    overflow: hidden;
  }

  .settings-progress-fill {
    height: 100%;
    background: var(--accent);
    border-radius: inherit;
    transition: width 0.25s ease;
  }

  /* Toggle switch */
  .toggle {
    position: relative;
    width: 36px;
    height: 20px;
    border-radius: 10px;
    background: var(--border-subtle);
    transition: background var(--transition-fast);
    flex-shrink: 0;
  }

  .toggle.on {
    background: var(--accent);
  }

  .toggle-thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--accent-contrast);
    transition: transform var(--transition-fast);
    box-shadow: var(--shadow-sm);
  }

  .toggle.on .toggle-thumb {
    transform: translateX(16px);
  }

  /* Theme picker */
  .theme-picker {
    display: flex;
    gap: 0;
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    overflow: hidden;
    width: fit-content;
  }

  .theme-option {
    padding: 8px 20px;
    font-size: 13px;
    font-weight: 450;
    color: var(--text-secondary);
    background: var(--surface);
    border-right: 1px solid var(--border-subtle);
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .theme-option:last-child {
    border-right: none;
  }

  .theme-option:hover {
    background: var(--surface-hover);
  }

  .theme-option.active {
    background: var(--surface-active);
    color: var(--accent);
    font-weight: 500;
  }

  .accent-picker {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .accent-option {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    background: var(--surface);
    transition: border-color var(--transition-fast), background var(--transition-fast), transform var(--transition-fast);
  }

  .accent-option:hover {
    background: var(--surface-hover);
  }

  .accent-option.active {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, var(--surface));
  }

  .accent-swatch {
    width: 18px;
    height: 18px;
    border-radius: 999px;
    background: var(--swatch);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--surface) 72%, transparent);
    flex-shrink: 0;
  }

  .accent-name {
    font-size: 13px;
    color: var(--text-primary);
  }

  @media (max-width: 720px) {
    .settings-scroll {
      padding: 0 20px 64px;
    }

    .update-row,
    .update-panel-head {
      flex-direction: column;
      align-items: stretch;
    }

    .accent-picker {
      grid-template-columns: 1fr;
    }
  }
</style>
