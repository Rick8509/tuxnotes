<script lang="ts">
  import { Folder as FolderIcon, Search, SquarePen, X } from "@lucide/svelte";
  import { formatListDate, purgeLabel } from "../format";
  import { app } from "../stores/app.svelte";
  import type { NoteSummary } from "../types";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";

  let searchInput = $state<HTMLInputElement>();
  let listEl = $state<HTMLDivElement>();
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

  $effect(() => {
    if (app.focusSearchTick) {
      searchInput?.focus();
      searchInput?.select();
    }
  });

  // Keep the selected row visible when navigating with the keyboard.
  $effect(() => {
    const id = app.selectedId;
    if (id) listEl?.querySelector(`[data-id="${CSS.escape(id)}"]`)?.scrollIntoView({ block: "nearest" });
  });

  const showSections = $derived(!app.searchResults && !app.isTrash);
  const pinned = $derived(showSections ? app.visibleNotes.filter((n) => n.isPinned) : []);
  const others = $derived(showSections ? app.visibleNotes.filter((n) => !n.isPinned) : app.visibleNotes);
  const showFolderName = $derived(!!app.searchResults || app.filter.kind === "all");

  function onListKey(e: KeyboardEvent) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const order = [...pinned, ...others];
    const idx = order.findIndex((n) => n.id === app.selectedId);
    const next = order[e.key === "ArrowDown" ? idx + 1 : Math.max(0, idx - 1)];
    if (next) app.selectNote(next.id);
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      app.clearSearch();
      searchInput?.blur();
    } else if (e.key === "ArrowDown" || e.key === "Enter") {
      e.preventDefault();
      const first = app.visibleNotes[0];
      if (first) {
        app.selectNote(first.id);
        listEl?.focus();
      }
    }
  }

  function noteMenu(e: MouseEvent, note: NoteSummary) {
    e.preventDefault();
    if (app.selectedId !== note.id) app.selectNote(note.id);
    const items: MenuItem[] = note.deletedAt
      ? [
          { label: "Restore", action: () => app.restoreNote(note.id) },
          { kind: "separator" },
          { label: "Delete Permanently…", action: () => app.deleteNoteForever(note.id), danger: true },
        ]
      : [
          { label: note.isPinned ? "Unpin Note" : "Pin Note", action: () => app.togglePin(note.id) },
          { kind: "separator" },
          { kind: "heading", label: "Move to" },
          ...app.folders.map((f) => ({
            label: f.name,
            action: () => app.moveNote(note.id, f.id),
            disabled: f.id === note.folderId,
          })),
          { kind: "separator" },
          { label: "Delete", action: () => app.trashNote(note.id), danger: true },
        ];
    menu = { x: e.clientX, y: e.clientY, items };
  }
</script>

{#snippet row(note: NoteSummary)}
  <button
    class="note"
    class:selected={note.id === app.selectedId}
    data-id={note.id}
    onclick={() => app.selectNote(note.id)}
    oncontextmenu={(e) => noteMenu(e, note)}
  >
    <span class="title">{note.title || "New Note"}</span>
    <span class="meta">
      <span class="date">
        {note.deletedAt ? purgeLabel(note.deletedAt) : formatListDate(note.updatedAt)}
      </span>
      <span class="snippet">{note.snippet || "No additional text"}</span>
    </span>
    {#if showFolderName}
      <span class="folder"><FolderIcon size={12} />{app.folderName(note.folderId)}</span>
    {/if}
  </button>
{/snippet}

<section class="list-pane">
  <header class="header" data-tauri-drag-region>
    <div class="heading" data-tauri-drag-region>
      <h1 data-tauri-drag-region>{app.listTitle}</h1>
      <span class="count" data-tauri-drag-region>
        {app.visibleNotes.length} note{app.visibleNotes.length === 1 ? "" : "s"}
      </span>
    </div>
    <button class="icon-btn" title="New Note (Ctrl+N)" aria-label="New note" onclick={() => app.newNote()}>
      <SquarePen size={18} />
    </button>
  </header>

  <div class="search">
    <Search size={14} />
    <input
      bind:this={searchInput}
      type="search"
      placeholder="Search all notes"
      aria-label="Search all notes"
      value={app.query}
      oninput={(e) => app.setQuery(e.currentTarget.value)}
      onkeydown={onSearchKey}
    />
    {#if app.query}
      <button class="clear" aria-label="Clear search" onclick={() => app.clearSearch()}><X size={12} /></button>
    {/if}
  </div>

  {#if app.isTrash && app.visibleNotes.length && !app.searchResults}
    <div class="trash-banner">
      Notes are deleted permanently after 30 days.
      <button onclick={() => app.emptyTrash()}>Empty</button>
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="notes" role="listbox" aria-label="Notes" tabindex="0" bind:this={listEl} onkeydown={onListKey}>
    {#if pinned.length}
      <div class="section">Pinned</div>
      {#each pinned as note (note.id)}{@render row(note)}{/each}
      {#if others.length}<div class="section">Notes</div>{/if}
    {/if}
    {#each others as note (note.id)}{@render row(note)}{/each}

    {#if !app.visibleNotes.length}
      <p class="empty">
        {#if app.searchResults}No results{:else if app.isTrash}No deleted notes{:else}No notes{/if}
      </p>
    {/if}
  </div>
</section>

{#if menu}
  <ContextMenu {...menu} onclose={() => (menu = null)} />
{/if}

<style>
  .list-pane {
    display: flex;
    flex-direction: column;
    background: var(--bg-list);
    border-right: 1px solid var(--separator);
    min-width: 0;
  }
  .header {
    height: var(--header-h);
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 10px 0 16px;
  }
  .heading {
    min-width: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .count {
    font-size: 11px;
    color: var(--text-secondary);
  }
  .search {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 12px 8px;
    padding: 0 8px;
    height: 28px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-secondary);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    outline: none;
    color: var(--text);
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  .search:focus-within {
    box-shadow: 0 0 0 2px var(--accent);
  }
  .clear {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-tertiary);
    color: var(--bg-list);
  }
  .trash-banner {
    flex: none;
    margin: 0 12px 8px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    font-size: 12px;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .trash-banner button {
    color: var(--accent-strong);
    font-weight: 600;
  }
  .notes {
    flex: 1;
    overflow-y: auto;
    padding: 0 8px 12px;
    outline: none;
  }
  .section {
    margin: 10px 8px 4px;
    font-size: 11px;
    font-weight: 700;
    color: var(--text-secondary);
  }
  .note {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 9px 10px;
    border-radius: var(--radius-md);
    text-align: left;
    position: relative;
  }
  /* Thin separators between rows, hidden around the selected row. */
  .note + .note::before {
    content: "";
    position: absolute;
    top: 0;
    left: 10px;
    right: 10px;
    height: 1px;
    background: var(--separator);
  }
  .note.selected::before,
  .note.selected + .note::before {
    display: none;
  }
  .note.selected {
    background: var(--sel-list);
    color: var(--sel-list-text);
  }
  .notes:not(:focus-within) .note.selected {
    filter: saturate(0.6);
  }
  .title {
    font-weight: 700;
    font-size: 13px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .meta {
    display: flex;
    gap: 8px;
    font-size: 12px;
    min-width: 0;
  }
  .date {
    flex: none;
    font-weight: 500;
  }
  .snippet {
    color: var(--text-secondary);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .selected .snippet,
  .selected .folder {
    color: inherit;
    opacity: 0.75;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-secondary);
  }
  .empty {
    margin-top: 40px;
    text-align: center;
    color: var(--text-tertiary);
  }
</style>
