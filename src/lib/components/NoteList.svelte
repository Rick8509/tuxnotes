<script lang="ts">
  import { Folder as FolderIcon, LockKeyhole, PanelLeftOpen, Search, SquarePen, X } from "@lucide/svelte";
  import { NOTE_DRAG_TYPE } from "../dnd";
  import { formatListDate, groupByDate, purgeLabel } from "../format";
  import { visibleRows } from "../folders";
  import { app } from "../stores/app.svelte";
  import { prefs } from "../stores/prefs.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { NoteSummary } from "../types";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import Resizer from "./Resizer.svelte";

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

  // Apple-style sections: Pinned, then Today / Yesterday / Previous 7 Days / … by last edit.
  // Search results and Recently Deleted are shown as one flat list.
  const showSections = $derived(!app.searchResults && !app.isTrash);
  // Settings → General: date sections follow the sort field; sorting by title has none.
  const dateOf = (n: NoteSummary) => (prefs.sortBy === "created" ? n.createdAt : n.updatedAt);
  const sections = $derived.by(() => {
    if (!showSections) return [{ label: "", notes: app.visibleNotes }];
    const pinned = app.visibleNotes.filter((n) => n.isPinned);
    const rest = app.visibleNotes.filter((n) => !n.isPinned);
    const grouped =
      prefs.groupByDate && prefs.sortBy !== "title"
        ? groupByDate(rest, new Date(), undefined, dateOf)
        : [{ label: pinned.length ? "Notes" : "", notes: rest }];
    return [...(pinned.length ? [{ label: "Pinned", notes: pinned }] : []), ...grouped.filter((g) => g.notes.length)];
  });
  const ordered = $derived(sections.flatMap((s) => s.notes));
  const showFolderName = $derived(!!app.searchResults || app.filter.kind === "all");

  function onListKey(e: KeyboardEvent) {
    const idx = ordered.findIndex((n) => n.id === app.selectedId);
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp": {
        e.preventDefault();
        const next = ordered[e.key === "ArrowDown" ? idx + 1 : Math.max(0, idx - 1)];
        if (next) void app.selectNote(next.id);
        break;
      }
      case "Home":
      case "End": {
        e.preventDefault();
        const next = e.key === "Home" ? ordered[0] : ordered[ordered.length - 1];
        if (next) void app.selectNote(next.id);
        break;
      }
      // Enter or Tab: start typing in the selected note.
      case "Enter":
      case "Tab":
        if (e.shiftKey || !app.current || app.current.deletedAt) return;
        e.preventDefault();
        app.focusEditorTick++;
        break;
      case "Delete":
      case "Backspace":
        if (!app.selectedId) return;
        e.preventDefault();
        void app.deleteNote(app.selectedId);
        break;
    }
  }

  function onDragStart(e: DragEvent, note: NoteSummary) {
    if (!e.dataTransfer || note.deletedAt) return;
    e.dataTransfer.setData(NOTE_DRAG_TYPE, note.id);
    e.dataTransfer.setData("text/plain", note.title || "New Note");
    e.dataTransfer.effectAllowed = "move";
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
          note.isLocked
            ? { label: "Remove Lock", action: () => app.removeLock(note.id) }
            : { label: "Lock Note…", action: () => app.lockNote(note.id) },
          { label: "Duplicate", action: () => app.duplicateNote(note.id), disabled: note.isLocked },
          { kind: "separator" },
          { kind: "heading", label: "Move to" },
          // All folders in tree order; subfolders indented under their parent.
          ...visibleRows(app.folders).map(({ folder: f, depth }) => ({
            label: `${"\u2003".repeat(depth)}${f.name}`,
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
  <!-- Keyboard navigation lives on the listbox container (onListKey), per the ARIA listbox pattern. -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="note"
    class:selected={note.id === app.selectedId}
    role="option"
    aria-selected={note.id === app.selectedId}
    tabindex="-1"
    data-id={note.id}
    draggable={!note.deletedAt}
    ondragstart={(e) => onDragStart(e, note)}
    onclick={() => app.selectNote(note.id)}
    oncontextmenu={(e) => noteMenu(e, note)}
  >
    <span class="title">
      {#if note.isLocked}<LockKeyhole size={12} class="lock" aria-label="Locked" />{/if}{note.title || "New Note"}
    </span>
    <span class="meta">
      <span class="date">
        {note.deletedAt ? purgeLabel(note.deletedAt) : formatListDate(dateOf(note))}
      </span>
      <span class="snippet">{note.isLocked ? "Locked" : note.snippet || "No additional text"}</span>
    </span>
    {#if showFolderName}
      <span class="folder"><FolderIcon size={12} />{app.folderName(note.folderId)}</span>
    {/if}
  </div>
{/snippet}

<section class="list-pane" class:collapsed={!ui.listVisible} inert={!ui.listVisible}>
  <header class="header" data-tauri-drag-region>
    {#if !ui.sidebarShown}
      <button class="icon-btn show-sidebar" title="Show sidebar (Ctrl+Alt+S)" aria-label="Show sidebar" onclick={() => ui.toggleSidebar()}>
        <PanelLeftOpen size={17} />
      </button>
    {/if}
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
    {#each sections as section (section.label)}
      {#if section.label}<div class="section">{section.label}</div>{/if}
      {#each section.notes as note (note.id)}{@render row(note)}{/each}
    {/each}

    {#if !app.visibleNotes.length}
      <p class="empty">
        {#if app.searchResults}No results{:else if app.isTrash}No deleted notes{:else}No notes{/if}
      </p>
    {/if}
  </div>

  {#if ui.listVisible}
    <Resizer width={ui.listWidth} onresize={(px) => ui.setListWidth(px)} label="Resize note list" />
  {/if}
</section>

{#if menu}
  <ContextMenu {...menu} onclose={() => (menu = null)} />
{/if}

<style>
  .list-pane {
    grid-column: 2;
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-list);
    border-right: 1px solid var(--separator);
  }
  .list-pane.collapsed {
    visibility: hidden;
    border-right-width: 0;
    transition: visibility 0s 0.22s;
  }
  .header {
    height: var(--header-h);
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    padding: 0 10px 0 16px;
  }
  .show-sidebar {
    margin-left: -8px;
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
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
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
    cursor: default;
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
  /* Like Apple Notes: yellow while the list has focus, grey while you're typing elsewhere. */
  .notes:not(:focus-within) .note.selected {
    background: var(--sel-list-inactive);
    color: var(--text);
  }
  .note:focus {
    outline: none;
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
  .title :global(.lock) {
    margin-right: 4px;
    vertical-align: -1px;
    color: var(--text-secondary);
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
