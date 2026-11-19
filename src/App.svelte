<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isTauri } from "./lib/api";
  import { app } from "./lib/stores/app.svelte";
  import { ui } from "./lib/stores/ui.svelte";
  import BackupDialog from "./lib/components/BackupDialog.svelte";
  import ConfirmDialog from "./lib/components/ConfirmDialog.svelte";
  import Editor from "./lib/components/Editor.svelte";
  import NoteList from "./lib/components/NoteList.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";

  onMount(() => {
    void app.init();
    const cleanups: (() => void)[] = [];

    if (isTauri) {
      const win = getCurrentWindow();
      // Save pending edits before the window closes (title-bar button, Alt+F4, etc.).
      win.onCloseRequested(async () => {
        await app.flush();
      }).then((fn) => cleanups.push(fn));

      // Track maximized/fullscreen so corners can go square when the window fills the screen.
      let timer: ReturnType<typeof setTimeout> | undefined;
      const syncWindowMode = async () => {
        ui.maximized = await win.isMaximized();
        ui.fullscreen = await win.isFullscreen();
      };
      void syncWindowMode();
      win.onResized(() => {
        clearTimeout(timer);
        timer = setTimeout(syncWindowMode, 80);
      }).then((fn) => cleanups.push(fn));
    }

    const onHide = () => {
      if (document.visibilityState === "hidden") void app.flush();
    };
    document.addEventListener("visibilitychange", onHide);
    window.addEventListener("blur", onHide);
    cleanups.push(() => {
      document.removeEventListener("visibilitychange", onHide);
      window.removeEventListener("blur", onHide);
    });
    return () => cleanups.forEach((fn) => fn());
  });

  async function toggleFullscreen() {
    if (!isTauri) return;
    const win = getCurrentWindow();
    await win.setFullscreen(!(await win.isFullscreen()));
  }

  function onkeydown(e: KeyboardEvent) {
    if (app.confirmRequest || app.showBackup) return;

    if (e.key === "F11") {
      e.preventDefault();
      void toggleFullscreen();
      return;
    }
    if (e.key === "Escape" && ui.sidebarOverlay) {
      ui.closeOverlay();
      return;
    }

    const mod = e.ctrlKey || e.metaKey;
    if (!mod || e.getModifierState("AltGraph")) return;
    const key = e.key.toLowerCase();

    // Ctrl+Alt: layout shortcuts (like Apple Notes' ⌥⌘S for the sidebar).
    if (e.altKey) {
      if (e.code === "KeyS") {
        e.preventDefault();
        ui.toggleSidebar();
      } else if (e.code === "KeyF") {
        e.preventDefault();
        ui.toggleFocus();
      }
      return;
    }

    if (key === "n" && !e.shiftKey) {
      e.preventDefault();
      void app.newNote();
    } else if (key === "n" && e.shiftKey) {
      e.preventDefault();
      if (!ui.sidebarShown) ui.toggleSidebar();
      app.creatingFolder = true;
    } else if (key === "f" && !e.shiftKey) {
      e.preventDefault();
      if (!ui.listVisible) ui.toggleFocus();
      app.focusSearchTick++;
    } else if (e.key === "Delete" && app.selectedId) {
      e.preventDefault();
      void app.deleteNote(app.selectedId);
    }
  }

  // Suppress the browser's own context menu except inside the editor (spellcheck, copy/paste).
  function oncontextmenu(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest(".prose, input")) e.preventDefault();
  }
</script>

<svelte:window {onkeydown} {oncontextmenu} bind:innerWidth={ui.windowWidth} />

<div class="frame" class:edge-to-edge={ui.edgeToEdge}>
  <div class="app" class:animate={!ui.resizing} style:grid-template-columns={ui.columns}>
    <Sidebar />
    <NoteList />
    <Editor />
  </div>

  {#if ui.sidebarOverlay}
    <!-- Click outside the floating sidebar to close it. -->
    <div class="overlay-scrim" role="presentation" onmousedown={() => ui.closeOverlay()}></div>
  {/if}

  {#if app.showBackup}
    <BackupDialog />
  {/if}
  <ConfirmDialog />

  {#if app.toast}
    <div class="toast" class:info={app.toast.kind === "info"} role={app.toast.kind === "error" ? "alert" : "status"}>
      <span>{app.toast.message}</span>
      <button aria-label="Dismiss" onclick={() => (app.toast = null)}>✕</button>
    </div>
  {/if}
</div>

<style>
  /* The window itself is transparent; this frame draws the rounded app window.
     `contain: paint` makes it the containing block for fixed-position children
     (dialogs, menus, toasts) so they're clipped to the rounded corners too. */
  .frame {
    position: relative;
    height: 100%;
    overflow: hidden;
    contain: paint;
    border-radius: var(--window-radius);
    background: var(--bg-editor);
  }
  /* Hairline window border drawn above everything. */
  .frame::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    border: 1px solid var(--window-border);
    pointer-events: none;
    z-index: 1000;
  }
  .frame.edge-to-edge {
    border-radius: 0;
  }
  .frame.edge-to-edge::after {
    display: none;
  }

  .app {
    display: grid;
    /* One row exactly as tall as the window, so each pane scrolls internally. */
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
  }
  .app.animate {
    transition: grid-template-columns 0.22s ease;
  }
  @media (prefers-reduced-motion: reduce) {
    .app.animate {
      transition: none;
    }
  }

  .overlay-scrim {
    position: absolute;
    inset: 0;
    z-index: 40;
  }

  .toast {
    position: fixed;
    left: 50%;
    bottom: 20px;
    transform: translateX(-50%);
    z-index: 110;
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: min(520px, 90vw);
    padding: 10px 14px;
    border-radius: var(--radius-md);
    background: var(--text);
    color: var(--bg-editor);
    box-shadow: var(--shadow-pop);
  }
  .toast:not(.info)::before {
    content: "";
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--danger);
  }
  .toast button {
    opacity: 0.7;
  }
  .toast button:hover {
    opacity: 1;
  }
</style>
