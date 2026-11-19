<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { Editor, Extension } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import { TaskItem, TaskList } from "@tiptap/extension-list";
  import { Placeholder } from "@tiptap/extensions";
  import { TableKit } from "@tiptap/extension-table";
  import { EditorState } from "@tiptap/pm/state";
  import type { EditorView } from "@tiptap/pm/view";
  import { Maximize2, Minimize2, Pin, PinOff, RotateCcw, SquarePen, Trash2 } from "@lucide/svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { isTauri } from "../api";
  import { formatEditorDate } from "../format";
  import { tableMenuItems } from "../tableMenu";
  import { extractText } from "../text";
  import { app } from "../stores/app.svelte";
  import { prefs, TEXT_SIZE_PX } from "../stores/prefs.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { DocJSON } from "../types";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import EditorToolbar from "./EditorToolbar.svelte";
  import WindowControls from "./WindowControls.svelte";

  let element: HTMLDivElement;
  let scroller: HTMLDivElement;
  let editor = $state.raw<Editor | null>(null);
  /** Right-click menu inside a table. */
  let tableMenu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
  /**
   * The open note contains something this version can't display (e.g. made by a newer
   * TuxNotes). It's shown as plain text and kept read-only so it can never be overwritten.
   */
  let unsupported = $state(false);
  let version = $state(0);
  /** Id of the note currently loaded into the editor. */
  let loadedId: string | null = null;

  // Apple-Notes-like extra shortcuts.
  const Shortcuts = Extension.create({
    name: "tuxnotesShortcuts",
    addKeyboardShortcuts() {
      return {
        "Mod-Shift-l": () => this.editor.commands.toggleTaskList(),
        "Mod-Shift-s": () => this.editor.commands.toggleStrike(),
        "Mod-Shift-t": () => this.editor.commands.toggleHeading({ level: 1 }),
        "Mod-Shift-h": () => this.editor.commands.toggleHeading({ level: 2 }),
        "Mod-Shift-j": () => this.editor.commands.toggleHeading({ level: 3 }),
        "Mod-Shift-b": () => this.editor.commands.setParagraph(),
      };
    },
  });

  // Tab inserts a tab character instead of moving focus out of the note.
  // Low priority so list items still indent/outdent with Tab first.
  const TabKey = Extension.create({
    name: "tuxnotesTab",
    priority: 50,
    addKeyboardShortcuts() {
      return { Tab: () => this.editor.commands.insertContent("\t") };
    },
  });

  /** Ctrl+click (or middle-click) a link to open it in the web browser. */
  function openLink(event: MouseEvent): boolean {
    const link = (event.target as HTMLElement).closest("a[href]") as HTMLAnchorElement | null;
    if (!link || !(event.ctrlKey || event.metaKey || event.button === 1)) return false;
    event.preventDefault();
    const href = link.href;
    if (!/^(https?|mailto):/i.test(href)) return true;
    if (isTauri) openUrl(href).catch((e) => app.showError(`Couldn't open the link: ${e}`));
    else window.open(href, "_blank", "noopener");
    return true;
  }

  /** Right-click inside a table cell: table actions instead of the browser menu. */
  function openTableMenu(view: EditorView, event: MouseEvent): boolean {
    const ed = editor;
    const cell = (event.target as HTMLElement).closest("td, th");
    if (!ed || !cell || !ed.isEditable) return false;
    event.preventDefault();
    // Act on the right-clicked cell, unless it's part of a multi-cell selection (keep that for "Merge").
    if (!cell.classList.contains("selectedCell")) {
      const pos = view.posAtCoords({ left: event.clientX, top: event.clientY });
      if (pos) ed.chain().focus().setTextSelection(pos.pos).run();
    }
    tableMenu = { x: event.clientX, y: event.clientY, items: tableMenuItems(ed) };
    return true;
  }

  /** Clicking the empty space below the text puts the cursor at the end, like Apple Notes. */
  function onScrollerClick(e: MouseEvent) {
    if (e.target === scroller && editor?.isEditable) editor.commands.focus("end");
  }

  onMount(() => {
    const ed = new Editor({
      element,
      extensions: [
        StarterKit.configure({
          heading: { levels: [1, 2, 3] },
          link: { openOnClick: false, autolink: true, defaultProtocol: "https" },
        }),
        TaskList,
        TaskItem.configure({ nested: true }),
        Placeholder.configure({ placeholder: "Start writing…" }),
        // Tab / Shift+Tab move between cells; Tab in the last cell adds a row.
        TableKit.configure({ table: { resizable: true, cellMinWidth: 60 } }),
        Shortcuts,
        TabKey,
      ],
      // Created editable so editable-only plugins (like table column resizing) are installed.
      // Nothing can be typed until a note loads: the editor is hidden and loadNote sets this per note.
      editable: true,
      editorProps: {
        attributes: { class: "prose", spellcheck: "true", "aria-label": "Note editor" },
        handleDOMEvents: {
          click: (_view, event) => openLink(event),
          auxclick: (_view, event) => openLink(event),
          contextmenu: (view, event) => openTableMenu(view, event),
        },
      },
      onTransaction: () => version++,
      onUpdate: ({ editor: e }) => {
        const id = loadedId;
        if (id) app.markDirty(id, () => e.getJSON() as DocJSON);
      },
    });
    editor = ed;
    return () => {
      void app.flush();
      ed.destroy();
      editor = null;
    };
  });

  // Load the selected note. A fresh EditorState per note keeps undo history separate,
  // so Ctrl+Z can never bring back another note's text.
  // Depends only on `app.current` and `editor`; everything else is untracked so
  // editor transactions can't re-trigger this effect (that would loop forever).
  $effect(() => {
    const note = app.current;
    const ed = editor;
    if (!ed) return;
    untrack(() => loadNote(ed, note));
  });

  function loadNote(ed: Editor, note: typeof app.current) {
    const id = note?.id ?? null;
    if (id !== loadedId) {
      loadedId = id;
      const json = note ? $state.snapshot(note.content) : { type: "doc", content: [{ type: "paragraph" }] };
      let doc;
      unsupported = false;
      tableMenu = null;
      try {
        doc = ed.schema.nodeFromJSON(json);
        doc.check();
      } catch (err) {
        console.warn("note content not supported by this version:", err);
        unsupported = true;
        const lines = extractText(json).plain.split("\n");
        doc = ed.schema.nodeFromJSON({
          type: "doc",
          content: lines.map((l) => (l ? { type: "paragraph", content: [{ type: "text", text: l }] } : { type: "paragraph" })),
        });
      }
      ed.view.updateState(EditorState.create({ doc, plugins: ed.state.plugins }));
      scroller.scrollTop = 0;
      version++;
    }
    const editable = !!note && !note.deletedAt && !unsupported;
    if (ed.isEditable !== editable) ed.setEditable(editable, false);
  }

  $effect(() => {
    const ed = editor;
    if (app.focusEditorTick && ed) untrack(() => ed.commands.focus("end"));
  });

  const note = $derived(app.current);
  const readOnly = $derived(!note || !!note.deletedAt || unsupported);
</script>

<main class="editor-pane">
  <header class="header" data-tauri-drag-region>
    {#if !ui.listVisible && !ui.sidebarShown}
      <!-- Focus mode: this is the only way back, so it sits where the panes were. -->
      <button class="icon-btn" title="Show notes list (Ctrl+Alt+F)" aria-label="Exit focus mode" onclick={() => ui.toggleFocus()}>
        <Minimize2 size={16} />
      </button>
    {:else}
      <button class="icon-btn" title="Focus mode: hide sidebar and notes list (Ctrl+Alt+F)" aria-label="Focus mode" onclick={() => ui.toggleFocus()}>
        <Maximize2 size={16} />
      </button>
    {/if}
    <span class="divider"></span>
    <EditorToolbar {editor} {version} disabled={readOnly} />
    <div class="spacer" data-tauri-drag-region></div>
    {#if note && !note.deletedAt}
      <button
        class="icon-btn"
        class:active={note.isPinned}
        title={note.isPinned ? "Unpin note" : "Pin note"}
        aria-label={note.isPinned ? "Unpin note" : "Pin note"}
        onclick={() => app.togglePin(note.id)}
      >
        {#if note.isPinned}<PinOff size={17} />{:else}<Pin size={17} />{/if}
      </button>
    {/if}
    <button
      class="icon-btn"
      title={app.isTrash ? "Delete permanently" : "Delete note (Ctrl+Delete)"}
      aria-label="Delete note"
      disabled={!note}
      onclick={() => note && app.deleteNote(note.id)}
    >
      <Trash2 size={17} />
    </button>
    <WindowControls />
  </header>

  {#if note?.deletedAt}
    <div class="trash-banner">
      <span>This note is in Recently Deleted.</span>
      <button onclick={() => app.restoreNote(note.id)}><RotateCcw size={13} /> Restore</button>
    </div>
  {/if}
  {#if note && unsupported}
    <div class="trash-banner">
      <span>This note uses something this version of TuxNotes can't show, so it's read-only here to keep it safe.</span>
    </div>
  {/if}

  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="scroll first-{prefs.firstLine}"
    class:hidden={!note}
    style:--editor-font-size="{TEXT_SIZE_PX[prefs.textSize]}px"
    bind:this={scroller}
    onclick={onScrollerClick}
  >
    <div class="date">{note ? formatEditorDate(note.updatedAt) : ""}</div>
    <!-- TipTap owns everything inside this element. -->
    <div bind:this={element}></div>
  </div>

  {#if !note}
    <div class="empty">
      <p>No note selected</p>
      {#if !app.isTrash}
        <button class="new" onclick={() => app.newNote()}><SquarePen size={15} /> New Note</button>
      {/if}
    </div>
  {/if}
</main>

{#if tableMenu}
  <ContextMenu {...tableMenu} onclose={() => (tableMenu = null)} />
{/if}

<style>
  .editor-pane {
    grid-column: 3;
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-editor);
  }
  .divider {
    width: 1px;
    height: 18px;
    margin: 0 6px;
    background: var(--separator-strong);
  }
  .header {
    height: var(--header-h);
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 12px;
  }
  .spacer {
    flex: 1;
    align-self: stretch;
  }
  .trash-banner {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 8px;
    background: var(--bg-input);
    color: var(--text-secondary);
    font-size: 12px;
  }
  .trash-banner button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--accent-strong);
    font-weight: 600;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0 max(32px, calc((100% - 760px) / 2)) 80px;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }
  .scroll.hidden {
    display: none;
  }
  .date {
    padding: 6px 0 14px;
    text-align: center;
    font-size: 12px;
    color: var(--text-tertiary);
    user-select: none;
    -webkit-user-select: none;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-tertiary);
    font-size: 15px;
  }
  .empty p {
    margin: 0;
  }
  .new {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text);
    font-size: 13px;
  }
  .new:hover {
    background: var(--bg-active);
  }

  /* ---------------------------------------------------- note typography */
  .scroll :global(.prose) {
    outline: none;
    tab-size: 4;
    /* Set from Settings → Appearance → Text size; everything below scales with it. */
    font-size: var(--editor-font-size, 15px);
    line-height: 1.55;
    color: var(--text);
    min-height: 60vh;
    caret-color: var(--accent);
  }
  .scroll :global(.prose > :first-child) {
    margin-top: 0;
  }
  .scroll :global(.prose p) {
    margin: 0;
  }
  .scroll :global(.prose h1) {
    font-size: 1.733em;
    font-weight: 700;
    line-height: 1.25;
    margin: 0.6em 0 0.3em;
  }
  .scroll :global(.prose h2) {
    font-size: 1.333em;
    font-weight: 700;
    margin: 0.8em 0 0.25em;
  }
  .scroll :global(.prose h3) {
    font-size: 1.067em;
    font-weight: 600;
    margin: 0.8em 0 0.2em;
  }
  /* "New notes start with" (Settings → General): like Apple Notes, the first line
     is shown as the note's title (or heading); "Body" leaves it as normal text. */
  .scroll.first-title :global(.prose > p:first-child),
  .scroll.first-title :global(.prose > h2:first-child),
  .scroll.first-title :global(.prose > h3:first-child) {
    font-size: 1.733em;
    font-weight: 700;
    line-height: 1.25;
    margin-bottom: 0.3em;
  }
  .scroll.first-heading :global(.prose > p:first-child),
  .scroll.first-heading :global(.prose > h3:first-child) {
    font-size: 1.333em;
    font-weight: 700;
    margin-bottom: 0.25em;
  }
  .scroll :global(.prose ul),
  .scroll :global(.prose ol) {
    margin: 0.25em 0;
    padding-left: 1.6em;
  }
  .scroll :global(.prose li > p) {
    margin: 0;
  }
  .scroll :global(.prose blockquote) {
    margin: 0.5em 0;
    padding-left: 12px;
    border-left: 3px solid var(--accent);
    color: var(--text-secondary);
  }
  .scroll :global(.prose code) {
    font-family: var(--font-mono);
    font-size: 0.9em;
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--bg-input);
  }
  .scroll :global(.prose pre) {
    font-family: var(--font-mono);
    font-size: 0.867em;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: var(--bg-input);
    overflow-x: auto;
  }
  .scroll :global(.prose pre code) {
    padding: 0;
    background: none;
  }
  .scroll :global(.prose a) {
    color: var(--accent-strong);
    cursor: text;
  }
  .scroll :global(.prose hr) {
    border: none;
    border-top: 1px solid var(--separator-strong);
    margin: 1em 0;
  }
  .scroll :global(.prose p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    height: 0;
    pointer-events: none;
    color: var(--text-tertiary);
  }

  /* ---------------------------------------------------------------- tables */
  .scroll :global(.prose .tableWrapper) {
    overflow-x: auto;
    margin: 0.6em 0;
  }
  .scroll :global(.prose table) {
    border-collapse: collapse;
    table-layout: fixed;
    width: 100%;
    margin: 0.6em 0;
    overflow: hidden;
  }
  .scroll :global(.prose .tableWrapper table) {
    margin: 0;
  }
  .scroll :global(.prose td),
  .scroll :global(.prose th) {
    position: relative;
    box-sizing: border-box;
    min-width: 60px;
    padding: 6px 10px;
    border: 1px solid var(--separator-strong);
    vertical-align: top;
    text-align: left;
  }
  .scroll :global(.prose th) {
    background: var(--bg-input);
    font-weight: 600;
  }
  .scroll :global(.prose td > p),
  .scroll :global(.prose th > p) {
    margin: 0;
  }
  /* The first line of a note is styled as its title, but never inside a table. */
  .scroll :global(.prose td p:first-child),
  .scroll :global(.prose th p:first-child) {
    font-size: inherit;
  }
  .scroll :global(.prose .selectedCell::after) {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 2;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    pointer-events: none;
  }
  .scroll :global(.prose .column-resize-handle) {
    position: absolute;
    top: 0;
    right: -2px;
    bottom: 0;
    width: 4px;
    z-index: 3;
    background: var(--accent);
    pointer-events: none;
  }
  .scroll :global(.prose.resize-cursor) {
    cursor: col-resize;
  }

  /* Checklists: round yellow checkboxes like Apple Notes. */
  .scroll :global(ul[data-type="taskList"]) {
    list-style: none;
    padding-left: 0.2em;
  }
  .scroll :global(ul[data-type="taskList"] > li) {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }
  .scroll :global(ul[data-type="taskList"] > li > label) {
    flex: none;
    margin-top: 3px;
    user-select: none;
  }
  .scroll :global(ul[data-type="taskList"] > li > div) {
    flex: 1;
    min-width: 0;
  }
  .scroll :global(ul[data-type="taskList"] > li input[type="checkbox"]) {
    appearance: none;
    -webkit-appearance: none;
    width: 18px;
    height: 18px;
    margin: 0;
    border-radius: 50%;
    border: 1.5px solid var(--text-tertiary);
    display: grid;
    place-items: center;
    cursor: pointer;
  }
  .scroll :global(ul[data-type="taskList"] > li input[type="checkbox"]:checked) {
    background: var(--accent);
    border-color: var(--accent);
  }
  .scroll :global(ul[data-type="taskList"] > li input[type="checkbox"]:checked::after) {
    content: "";
    width: 4px;
    height: 8px;
    margin-top: -2px;
    border: solid #fff;
    border-width: 0 2px 2px 0;
    transform: rotate(45deg);
  }
  .scroll :global(ul[data-type="taskList"] > li[data-checked="true"] > div) {
    color: var(--text-tertiary);
  }
</style>
