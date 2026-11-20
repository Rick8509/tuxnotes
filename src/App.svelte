<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api, isTauri } from "./lib/api";
  import { app } from "./lib/stores/app.svelte";
  import { ui } from "./lib/stores/ui.svelte";
  import { printNote } from "./lib/importExport";
  import ConfirmDialog from "./lib/components/ConfirmDialog.svelte";
  import LockDialog from "./lib/components/LockDialog.svelte";
  import { prefs } from "./lib/stores/prefs.svelte";
  import Editor from "./lib/components/Editor.svelte";
  import NoteList from "./lib/components/NoteList.svelte";
  import NoteInfoDialog from "./lib/components/NoteInfoDialog.svelte";
  import SettingsDialog from "./lib/components/SettingsDialog.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";

  /** How long closing waits for a backup before quitting anyway. */
  const CLOSE_BACKUP_LIMIT_MS = 90_000;

  onMount(() => {
    void app.init();
    const cleanups: (() => void)[] = [];

    if (isTauri) {
      const win = getCurrentWindow();
      // Closing (title-bar button, Alt+F4, etc.): save pending edits and lock locked notes.
      // Then, if "Back up when closing" is on and something changed, the window hides
      // right away while the backup finishes (for at most CLOSE_BACKUP_LIMIT_MS).
      win.onCloseRequested(async (event) => {
        await app.flush();
        await app.lockNow();
        const needed = await api.backupOnCloseNeeded().catch(() => false);
        if (!needed) return;
        await win.hide();
        await Promise.race([
          api.backupBeforeExit().catch((e) => console.warn("backup before closing failed:", e)),
          new Promise((resolve) => setTimeout(resolve, CLOSE_BACKUP_LIMIT_MS)),
        ]);
        // Opened again while the backup ran (single-instance shows the window): stay open.
        if (await win.isVisible()) event.preventDefault();
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

    // Lock locked notes again after a while without using TuxNotes (Settings → General).
    let lastActivity = Date.now();
    const onActivity = () => (lastActivity = Date.now());
    const activityEvents = ["keydown", "pointerdown", "pointermove", "wheel"] as const;
    for (const ev of activityEvents) window.addEventListener(ev, onActivity, { passive: true, capture: true });
    const lockTimer = setInterval(() => {
      const minutes = prefs.autoLockMinutes;
      if (app.lock.unlocked && minutes > 0 && Date.now() - lastActivity >= minutes * 60_000) void app.lockNow();
    }, 15_000);
    cleanups.push(() => {
      clearInterval(lockTimer);
      for (const ev of activityEvents) window.removeEventListener(ev, onActivity, { capture: true });
    });

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
    if (app.confirmRequest || app.settingsSection || app.showNoteInfo || app.lockPrompt) return;

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

    // Ctrl+Z outside the editor undoes the last delete (while its toast is showing).
    if (key === "z" && !e.shiftKey && app.toast?.action) {
      if (!isTyping() && app.runToastAction()) {
        e.preventDefault();
        return;
      }
    }

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

    if (key === "p" && !e.shiftKey) {
      e.preventDefault();
      if (app.current) void printNote();
    } else if (e.key === ",") {
      e.preventDefault();
      app.settingsSection = "general";
    } else if (key === "n" && !e.shiftKey) {
      e.preventDefault();
      void app.newNote();
    } else if (key === "n" && e.shiftKey) {
      e.preventDefault();
      if (!ui.sidebarShown) ui.toggleSidebar();
      app.startNewFolder(null);
    } else if (key === "f") {
      e.preventDefault();
      // Like Apple Notes: Ctrl+F in a note finds in that note; Ctrl+Shift+F (or Ctrl+F
      // anywhere else) searches all notes.
      const focused = document.activeElement as HTMLElement | null;
      if (!e.shiftKey && app.current && focused?.closest(".prose, .find-bar")) {
        app.findInNoteTick++;
      } else {
        if (!ui.listVisible) ui.toggleFocus();
        app.focusSearchTick++;
      }
    } else if (e.key === "Delete" && app.selectedId) {
      // While typing, Ctrl+Delete deletes the next word. The note list handles Delete
      // itself (and marks the event as handled).
      if (e.defaultPrevented || isTyping()) return;
      e.preventDefault();
      void app.deleteNote(app.selectedId);
    }
  }

  /** Focus is in the note or another text field, where keys edit text. */
  function isTyping(): boolean {
    const focused = document.activeElement as HTMLElement | null;
    return !!focused?.closest(".prose, input, textarea, select, [contenteditable='true']");
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

  {#if app.settingsSection}
    <SettingsDialog />
  {/if}
  {#if app.showNoteInfo && app.current}
    <NoteInfoDialog />
  {/if}
  <ConfirmDialog />
  {#if app.lockPrompt}
    <LockDialog prompt={app.lockPrompt} />
  {/if}

  {#if app.toast}
    <div class="toast" class:info={app.toast.kind === "info"} role={app.toast.kind === "error" ? "alert" : "status"}>
      <span>{app.toast.message}</span>
      {#if app.toast.action}
        <button class="toast-action" onclick={() => app.runToastAction()}>{app.toast.action.label}</button>
      {/if}
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
  .toast .toast-action {
    opacity: 1;
    padding: 2px 10px;
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 700;
  }
</style>
