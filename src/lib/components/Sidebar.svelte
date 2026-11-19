<script lang="ts">
  import { ChevronRight, Folder as FolderIcon, FolderPlus, NotebookText, PanelLeftClose, Settings, Trash2 } from "@lucide/svelte";
  import { FOLDER_DRAG_TYPE, NOTE_DRAG_TYPE } from "../dnd";
  import { childrenOf, dropPlacement, visibleRows, type DropZone } from "../folders";
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
  /** The folder tree as rows (collapsed branches hidden). */
  const rows = $derived(visibleRows(app.folders, ui.collapsedFolders));

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

  // ---------------------------------------------- drag folders to nest/reorder
  let draggingFolder: string | null = null;
  /** Where the dragged folder would land: before/after a row, or inside it. "root" = the Folders label. */
  let folderDrop = $state<{ id: string; zone: DropZone } | null>(null);

  function folderDragStart(e: DragEvent, folder: Folder) {
    if (folder.isDefault || !e.dataTransfer) return;
    draggingFolder = folder.id;
    e.dataTransfer.setData(FOLDER_DRAG_TYPE, folder.id);
    e.dataTransfer.setData("text/plain", folder.name);
    e.dataTransfer.effectAllowed = "move";
  }

  function folderDragEnd() {
    draggingFolder = null;
    folderDrop = null;
  }

  function rowDragOver(e: DragEvent, folder: Folder) {
    if (e.dataTransfer?.types.includes(NOTE_DRAG_TYPE)) return dragOver(e, folder.id);
    if (!draggingFolder || !e.dataTransfer?.types.includes(FOLDER_DRAG_TYPE)) return;
    // Top quarter: before, bottom quarter: after, middle: inside.
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const y = (e.clientY - r.top) / r.height;
    const zone: DropZone = y < 0.28 ? "before" : y > 0.72 ? "after" : "inside";
    if (!dropPlacement(app.folders, draggingFolder, folder, zone)) {
      folderDrop = null;
      return;
    }
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    folderDrop = { id: folder.id, zone };
  }

  function rowDrop(e: DragEvent, folder: Folder) {
    if (e.dataTransfer?.types.includes(NOTE_DRAG_TYPE)) return drop(e, folder.id);
    const dragged = draggingFolder;
    const zone = folderDrop?.id === folder.id ? folderDrop.zone : null;
    folderDragEnd();
    if (!dragged || !zone) return;
    const place = dropPlacement(app.folders, dragged, folder, zone);
    if (!place) return;
    e.preventDefault();
    if (zone === "inside") ui.expandFolder(folder.id);
    void app.moveFolder(dragged, place.parentId, place.index);
  }

  /** Dropping a folder on the "Folders" label moves it to the top level, at the end. */
  function rootDragOver(e: DragEvent) {
    if (!draggingFolder || !e.dataTransfer?.types.includes(FOLDER_DRAG_TYPE)) return;
    e.preventDefault();
    folderDrop = { id: "root", zone: "inside" };
  }

  function rootDrop(e: DragEvent) {
    const dragged = draggingFolder;
    folderDragEnd();
    if (!dragged) return;
    e.preventDefault();
    const top = childrenOf(app.folders, null).filter((f) => !f.isDefault && f.id !== dragged);
    void app.moveFolder(dragged, null, top.length);
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

  function startCreate(parentId: string | null = null) {
    if (parentId) ui.expandFolder(parentId);
    app.startNewFolder(parentId);
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
        { label: "New Subfolder…", action: () => startCreate(folder.id) },
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
    <button class="row plain" class:active={isActive({ kind: "all" })} onclick={() => open({ kind: "all" })}>
      <NotebookText size={16} />
      <span class="name">All Notes</span>
      <span class="count">{app.allCount}</span>
    </button>

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="section" class:drop-target={folderDrop?.id === "root"} ondragover={rootDragOver} ondragleave={() => folderDrop?.id === "root" && (folderDrop = null)} ondrop={rootDrop}>Folders</div>

    {#snippet newFolderRow(depth: number)}
      <div class="tree-row" style:--depth={depth}>
        <span class="twisty-space"></span>
        <div class="row editing">
          <FolderIcon size={16} />
          <input
            class="name-input"
            bind:value={newFolderName}
            use:autofocus
            maxlength="100"
            placeholder={app.newFolderParent ? "New Subfolder" : "New Folder"}
            aria-label="New folder name"
            onkeydown={(e) => nameKeys(e, commitCreate, () => (app.creatingFolder = false))}
            onblur={commitCreate}
          />
        </div>
      </div>
    {/snippet}

    {#each rows as { folder, depth, hasChildren } (folder.id)}
      {@const collapsed = ui.collapsedFolders.has(folder.id)}
      <div
        class="tree-row"
        style:--depth={depth}
        class:drop-before={folderDrop?.id === folder.id && folderDrop.zone === "before"}
        class:drop-after={folderDrop?.id === folder.id && folderDrop.zone === "after"}
      >
        {#if hasChildren}
          <button class="twisty" class:open={!collapsed} aria-label={collapsed ? `Expand ${folder.name}` : `Collapse ${folder.name}`} onclick={() => ui.toggleFolder(folder.id)}>
            <ChevronRight size={13} />
          </button>
        {:else}
          <span class="twisty-space"></span>
        {/if}
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
            class:drop-target={dropTarget === folder.id || (folderDrop?.id === folder.id && folderDrop.zone === "inside")}
            draggable={!folder.isDefault}
            data-folder-id={folder.id}
            onclick={() => open({ kind: "folder", folderId: folder.id })}
            ondblclick={() => startRename(folder)}
            oncontextmenu={(e) => folderMenu(e, folder)}
            ondragstart={(e) => folderDragStart(e, folder)}
            ondragend={folderDragEnd}
            ondragover={(e) => rowDragOver(e, folder)}
            ondragleave={(e) => {
              dragLeave(e, folder.id);
              if (folderDrop?.id === folder.id && !(e.currentTarget as Node).contains(e.relatedTarget as Node | null)) folderDrop = null;
            }}
            ondrop={(e) => rowDrop(e, folder)}
          >
            <FolderIcon size={16} />
            <span class="name">{folder.name}</span>
            <span class="count">{folder.noteCount}</span>
          </button>
        {/if}
      </div>
      {#if app.creatingFolder && app.newFolderParent === folder.id}
        {@render newFolderRow(depth + 1)}
      {/if}
    {/each}

    {#if app.creatingFolder && !app.newFolderParent}
      {@render newFolderRow(0)}
    {/if}

    {#if app.trashCount > 0 || app.isTrash || dropTarget !== null}
      <button
        class="row plain"
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
    <button class="new-folder" onclick={() => startCreate(null)} title="New Folder (Ctrl+Shift+N)">
      <FolderPlus size={16} />
      <span>New Folder</span>
    </button>
    <button class="icon-btn" onclick={() => (app.settingsSection = "general")} title="Settings (Ctrl+,)" aria-label="Settings">
      <Settings size={17} />
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
  /* Folder tree */
  .tree-row {
    position: relative;
    display: flex;
    align-items: center;
    padding-left: calc(var(--depth, 0) * 14px);
  }
  .tree-row .row {
    flex: 1;
    min-width: 0;
    padding-left: 4px;
  }
  .twisty,
  .twisty-space {
    flex: none;
    width: 16px;
    height: 24px;
  }
  .twisty {
    display: grid;
    place-items: center;
    border-radius: 4px;
    color: var(--text-tertiary);
  }
  .twisty:hover {
    color: var(--text);
  }
  .twisty :global(svg) {
    transition: transform 0.15s;
  }
  .twisty.open :global(svg) {
    transform: rotate(90deg);
  }
  .row.plain {
    padding-left: 20px;
  }
  /* Where a dragged folder will land between rows. */
  .tree-row.drop-before::before,
  .tree-row.drop-after::after {
    content: "";
    position: absolute;
    left: calc(var(--depth, 0) * 14px + 18px);
    right: 4px;
    height: 2px;
    border-radius: 1px;
    background: var(--accent);
  }
  .tree-row.drop-before::before {
    top: -1px;
  }
  .tree-row.drop-after::after {
    bottom: -1px;
  }
  .section.drop-target {
    color: var(--accent-strong);
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
    align-items: center;
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
