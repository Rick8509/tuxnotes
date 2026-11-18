<script lang="ts">
  import { Folder as FolderIcon, FolderPlus, NotebookText, Trash2 } from "@lucide/svelte";
  import { app } from "../stores/app.svelte";
  import type { Folder, NoteFilter } from "../types";
  import { sameFilter } from "../types";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";

  let renamingId = $state<string | null>(null);
  let draftName = $state("");
  let newFolderName = $state("");

  // Reset the field whenever creation starts (also via Ctrl+Shift+N).
  $effect(() => {
    if (app.creatingFolder) newFolderName = "";
  });
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

  const isActive = (f: NoteFilter) => !app.searchResults && sameFilter(app.filter, f);

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

<aside class="sidebar">
  <header class="header" data-tauri-drag-region>
    <span class="brand" data-tauri-drag-region>TuxNotes</span>
  </header>

  <nav class="scroll" aria-label="Folders">
    <button class="row" class:active={isActive({ kind: "all" })} onclick={() => app.setFilter({ kind: "all" })}>
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
          onclick={() => app.setFilter({ kind: "folder", folderId: folder.id })}
          ondblclick={() => startRename(folder)}
          oncontextmenu={(e) => folderMenu(e, folder)}
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

    {#if app.trashCount > 0 || app.isTrash}
      <button
        class="row"
        class:active={isActive({ kind: "trash" })}
        onclick={() => app.setFilter({ kind: "trash" })}
        oncontextmenu={trashMenu}
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
  </footer>
</aside>

{#if menu}
  <ContextMenu {...menu} onclose={() => (menu = null)} />
{/if}

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--separator);
    min-width: 0;
  }
  .header {
    height: var(--header-h);
    flex: none;
    display: flex;
    align-items: center;
    padding: 0 16px;
  }
  .brand {
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.01em;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
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
