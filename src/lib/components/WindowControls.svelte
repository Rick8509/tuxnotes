<script lang="ts">
  // Minimize / maximize / close for the frameless window (GNOME-style, on the right).
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isTauri } from "../api";

  const win = isTauri ? getCurrentWindow() : null;
</script>

{#if win}
  <div class="controls">
    <button class="ctl" title="Minimize" aria-label="Minimize" onclick={() => win.minimize()}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 8h7" /></svg>
    </button>
    <button class="ctl" title="Maximize" aria-label="Maximize" onclick={() => win.toggleMaximize()}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="4.5" y="4.5" width="7" height="7" rx="1" /></svg>
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
