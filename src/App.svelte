<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { isTauri } from "./lib/api";
  import { app } from "./lib/stores/app.svelte";
  import BackupDialog from "./lib/components/BackupDialog.svelte";
  import ConfirmDialog from "./lib/components/ConfirmDialog.svelte";
  import Editor from "./lib/components/Editor.svelte";
  import NoteList from "./lib/components/NoteList.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";

  onMount(() => {
    void app.init();

    // Save pending edits before the window closes (title-bar button, Alt+F4, etc.).
    let unlisten: (() => void) | undefined;
    if (isTauri) {
      getCurrentWindow()
        .onCloseRequested(async () => {
          await app.flush();
        })
        .then((fn) => (unlisten = fn));
    }
    const onHide = () => {
      if (document.visibilityState === "hidden") void app.flush();
    };
    document.addEventListener("visibilitychange", onHide);
    window.addEventListener("blur", onHide);
    return () => {
      unlisten?.();
      document.removeEventListener("visibilitychange", onHide);
      window.removeEventListener("blur", onHide);
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (app.confirmRequest || app.showBackup) return;
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) return;
    const key = e.key.toLowerCase();

    if (key === "n" && !e.shiftKey) {
      e.preventDefault();
      void app.newNote();
    } else if (key === "n" && e.shiftKey) {
      e.preventDefault();
      app.creatingFolder = true;
    } else if (key === "f" && !e.shiftKey) {
      e.preventDefault();
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

<svelte:window {onkeydown} {oncontextmenu} />

<div class="app">
  <Sidebar />
  <NoteList />
  <Editor />
</div>

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

<style>
  .app {
    display: grid;
    grid-template-columns: var(--sidebar-w) var(--list-w) minmax(0, 1fr);
    height: 100%;
  }
  @media (max-width: 860px) {
    .app {
      grid-template-columns: 180px 240px minmax(0, 1fr);
    }
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
