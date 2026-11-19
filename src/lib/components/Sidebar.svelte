<script lang="ts">
  import { Cloud, Folder as FolderIcon, FolderPlus, NotebookText, PanelLeftClose, Trash2 } from "@lucide/svelte";
  import { NOTE_DRAG_TYPE } from "../dnd";
  import { app } from "../stores/app.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { Folder, NoteFilter } from "../types";
  import { sameFilter } from "../types";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import Resizer from "./Resizer.svelte";

  let renamingId = $state<string | null>(null);
  let draftName = $state("");
  let newFolderName = $state("");

  // Reset the field whenever creation starts (also via Ctrl+Shift+N).
  $effect(() => {
    if (app.creatingFolder) newFolderName = "";
  });
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

  const isActive = (f: NoteFilter) => !app.searchResults && sameFilter(app.filter, f);

  function open(filter: NoteFilter) {
    ui.closeOverlay();
    void app.setFilter(filter);
  }

  // ------------------------------------------------ drag notes onto folders
  /** Folder id (or "trash") currently under a dragged note. */
  let dropTarget = $state<string | null>(null);

  function dragOver(e: DragEvent, target: string) {
    if (!e.dataTransfer?.types.includes(NOTE_DRAG_TYPE)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    dropTarget = target;
  }

  function dragLeave(e: DragEvent, target: string) {
    const to = e.relatedTarget as Node | null;
    if (dropTarget === target && !(to && (e.currentTarget as Node).contains(to))) dropTarget = null;
  }

  function drop(e: DragEvent, target: string) {
    const id = e.dataTransfer?.getData(NOTE_DRAG_TYPE);
    dropTarget = null;
    if (!id) return;
    e.preventDefault();
    if (target === "trash") void app.trashNote(id);
    else void app.moveNote(id, target);
  }

  function startRename(folder: Folder) {
    if (folder.isDefault) return;
    renamingId = folder.id;
    draftName = folder.name;
  }

  // Enter commits and removes the input, which then fires blur; `busy` stops a double commit.
  let busy = false;

  async function commitRename() {
    const id = renamingId;
    if (!id || busy) return;
    const original = app.folders.find((f) => f.id === id)?.name;
    if (draftName.trim() && draftName.trim() !== original) {
      busy = true;
      const ok = await app.renameFolder(id, draftName).finally(() => (busy = false));
      if (!ok) return; // keep editing so the user can fix the name
    }
    renamingId = null;
  }

  function startCreate() {
    app.creatingFolder = true;
  }

  async function commitCreate() {
    if (!app.creatingFolder || busy) return;
    if (!newFolderName.trim()) {
      app.creatingFolder = false;
      return;
    }
    busy = true;
    await app.createFolder(newFolderName).finally(() => (busy = false));
  }

  function nameKeys(e: KeyboardEvent, commit: () => void, cancel: () => void) {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    }
  }

  function folderMenu(e: MouseEvent, folder: Folder) {
    e.preventDefault();
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "New Note", action: () => app.setFilter({ kind: "folder", folderId: folder.id }).then(() => app.newNote()) },
        { kind: "separator" },
        { label: "Rename Folder", action: () => startRename(folder), disabled: folder.isDefault },
        { label: "Delete Folder", action: () => app.deleteFolder(folder.id), disabled: folder.isDefault, danger: true },
      ],
    };
  }

  function trashMenu(e: MouseEvent) {
    e.preventDefault();
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [{ label: "Empty Recently Deleted", action: () => app.emptyTrash(), disabled: !app.trashCount, danger: true }],
    };
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<aside
  class="sidebar"
  class:collapsed={!ui.sidebarShown}
  class:overlay={ui.sidebarOverlay}
  style:width={ui.sidebarOverlay ? `${ui.sidebarWidth}px` : null}
  inert={!ui.sidebarShown}
>
  <header class="header" data-tauri-drag-region>
    <span class="brand" data-tauri-drag-region>TuxNotes</span>
    <button class="icon-btn" title="Hide sidebar (Ctrl+Alt+S)" aria-label="Hide sidebar" onclick={() => ui.toggleSidebar()}>
      <PanelLeftClose size={17} />
    </button>
  </header>

  <nav class="scroll" aria-label="Folders">
    <button class="row" class:active={isActive({ kind: "all" })} onclick={() => open({ kind: "all" })}>
      <NotebookText size={16} />
      <span class="name">All Notes</span>
      <span class="count">{app.allCount}</span>
    </button>

    <div class="section">Folders</div>

    {#each app.folders as folder (folder.id)}
      {#if renamingId === folder.id}
        <div class="row editing">
          <FolderIcon size={16} />
          <input
            class="name-input"
            bind:value={draftName}
            use:autofocus
            maxlength="100"
            aria-label="Folder name"
            onkeydown={(e) => nameKeys(e, commitRename, () => (renamingId = null))}
            onblur={commitRename}
          />
        </div>
      {:else}
        <button
          class="row"
          class:active={isActive({ kind: "folder", folderId: folder.id })}
          class:drop-target={dropTarget === folder.id}
          onclick={() => open({ kind: "folder", folderId: folder.id })}
          ondblclick={() => startRename(folder)}
          oncontextmenu={(e) => folderMenu(e, folder)}
          ondragover={(e) => dragOver(e, folder.id)}
          ondragleave={(e) => dragLeave(e, folder.id)}
          ondrop={(e) => drop(e, folder.id)}
        >
          <FolderIcon size={16} />
          <span class="name">{folder.name}</span>
          <span class="count">{folder.noteCount}</span>
        </button>
      {/if}
    {/each}

    {#if app.creatingFolder}
      <div class="row editing">
        <FolderIcon size={16} />
        <input
          class="name-input"
          bind:value={newFolderName}
          use:autofocus
          maxlength="100"
          placeholder="New Folder"
          aria-label="New folder name"
          onkeydown={(e) => nameKeys(e, commitCreate, () => (app.creatingFolder = false))}
          onblur={commitCreate}
        />
      </div>
    {/if}

    {#if app.trashCount > 0 || app.isTrash || dropTarget !== null}
      <button
        class="row"
        class:active={isActive({ kind: "trash" })}
        class:drop-target={dropTarget === "trash"}
        onclick={() => open({ kind: "trash" })}
        oncontextmenu={trashMenu}
        ondragover={(e) => dragOver(e, "trash")}
        ondragleave={(e) => dragLeave(e, "trash")}
        ondrop={(e) => drop(e, "trash")}
      >
        <Trash2 size={16} />
        <span class="name">Recently Deleted</span>
        <span class="count">{app.trashCount}</span>
      </button>
    {/if}
  </nav>

  <footer class="footer">
    <button class="new-folder" onclick={startCreate} title="New Folder (Ctrl+Shift+N)">
      <FolderPlus size={16} />
      <span>New Folder</span>
    </button>
    <button class="new-folder" onclick={() => (app.showBackup = true)} title="Backup & Restore">
      <Cloud size={16} />
      <span>Backup</span>
    </button>
  </footer>

  {#if ui.sidebarInline}
    <Resizer width={ui.sidebarWidth} onresize={(px) => ui.setSidebarWidth(px)} label="Resize sidebar" />
  {/if}
</aside>

{#if menu}
  <ContextMenu {...menu} onclose={() => (menu = null)} />
{/if}

<style>
  .sidebar {
    grid-column: 1;
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--separator);
  }
  /* Hidden: stay visible while the column animates closed, then hide. */
  .sidebar.collapsed {
    visibility: hidden;
    border-right-width: 0;
    transition: visibility 0s 0.22s;
  }
  /* Narrow windows: the sidebar floats over the note list. */
  .sidebar.overlay {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 50;
    border-right: 1px solid var(--separator-strong);
    box-shadow: 8px 0 30px rgba(0, 0, 0, 0.18);
    animation: slide-in 0.18s ease-out;
  }
  @keyframes slide-in {
    from {
      transform: translateX(-24px);
      opacity: 0;
    }
  }
  .header {
    height: var(--header-h);
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px 0 16px;
  }
  .brand {
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.01em;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0 10px 10px;
  }
  .section {
    margin: 14px 8px 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    text-align: left;
    color: var(--text);
  }
  .row :global(svg) {
    flex: none;
    color: var(--accent);
  }
  .row:hover:not(.editing) {
    background: var(--bg-hover);
  }
  .row.active {
    background: var(--sel-sidebar);
  }
  .row.drop-target {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .row.drop-target :global(svg),
  .row.drop-target .count {
    color: inherit;
  }
  .name {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .count {
    color: var(--text-secondary);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .name-input {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--accent);
    border-radius: 5px;
    background: var(--bg-list);
    outline: none;
  }
  .footer {
    flex: none;
    display: flex;
    justify-content: space-between;
    padding: 8px 10px 10px;
  }
  .new-folder {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }
  .new-folder:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
</style>
