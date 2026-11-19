<script lang="ts">
  // Minimize / maximize / close for the frameless window (GNOME-style, on the right).
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isTauri } from "../api";
  import { ui } from "../stores/ui.svelte";

  const win = isTauri ? getCurrentWindow() : null;

  /** In fullscreen this button leaves fullscreen; otherwise it maximizes/restores. */
  function toggleSize() {
    if (!win) return;
    if (ui.fullscreen) void win.setFullscreen(false);
    else void win.toggleMaximize();
  }
  const restoreLabel = $derived(ui.fullscreen ? "Exit full screen" : ui.maximized ? "Restore" : "Maximize");
</script>

{#if win}
  <div class="controls">
    <button class="ctl" title="Minimize" aria-label="Minimize" onclick={() => win.minimize()}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 8h7" /></svg>
    </button>
    <button class="ctl" title={restoreLabel} aria-label={restoreLabel} onclick={toggleSize}>
      {#if ui.maximized || ui.fullscreen}
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <rect x="4" y="6" width="6" height="6" rx="1" /><path d="M6.5 6V5a1 1 0 0 1 1-1H11a1 1 0 0 1 1 1v3.5a1 1 0 0 1-1 1h-1" />
        </svg>
      {:else}
        <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="4.5" y="4.5" width="7" height="7" rx="1" /></svg>
      {/if}
    </button>
    <button class="ctl" title="Close" aria-label="Close" onclick={() => win.close()}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M5 5l6 6M11 5l-6 6" /></svg>
    </button>
  </div>
{/if}

<style>
  .controls {
    display: flex;
    gap: 10px;
    margin-left: 10px;
  }
  .ctl {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--bg-input);
    color: var(--text);
  }
  .ctl:hover {
    background: var(--bg-active);
  }
  svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
  }
</style>
