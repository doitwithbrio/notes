<script lang="ts">
  import { X } from 'lucide-svelte';
  import { settingsState, loadSettings, saveSettings } from '../../state/settings.svelte.js';
  import { uiState } from '../../state/ui.svelte.js';
  import type { AppSettings } from '../../types/index.js';

  let relayInput = $state('');
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  // Load settings on mount
  $effect(() => {
    if (!settingsState.settings && !settingsState.loading) {
      void loadSettings();
    }
  });

  function update(patch: Partial<AppSettings>) {
    if (!settingsState.settings) return;
    const next = { ...settingsState.settings, ...patch };
    settingsState.settings = next;

    // Debounced save
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void saveSettings(next);
    }, 400);
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
          <h2 class="section-title">theme</h2>
          <div class="theme-picker">
            {#each ['system', 'light', 'dark'] as theme (theme)}
              <button
                class="theme-option"
                class:active={s.theme === theme}
                onclick={() => update({ theme })}
              >
                {theme}
              </button>
            {/each}
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
    color: #c0392b;
    background: rgba(192, 57, 43, 0.08);
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
    background: white;
    transition: transform var(--transition-fast);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
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
</style>
