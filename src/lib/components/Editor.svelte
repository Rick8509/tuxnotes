<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { Editor, Extension, getMarkRange } from "@tiptap/core";
  import { Placeholder } from "@tiptap/extensions";
  import { EditorState, NodeSelection } from "@tiptap/pm/state";
  import type { EditorView } from "@tiptap/pm/view";
  import { ChevronDown, ChevronUp, Ellipsis, LockOpen, Maximize2, Minimize2, Pin, PinOff, RotateCcw, Search, SquarePen, Trash2 } from "@lucide/svelte";
  import LockedNote from "./LockedNote.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, isTauri } from "../api";
  import { NOTE_DRAG_TYPE } from "../dnd";
  import { nodeForAttachment } from "../editor/attachmentNodes";
  import { contentExtensions } from "../editor/content";
  import { ChecklistSort, checklistActions } from "../editor/checklist";
  import { SearchHighlight, getSearch, goToMatch, scrollToPos, setSearch } from "../editor/search";
  import { formatEditorDate } from "../format";
  import { exportMarkdown, exportPdf, importMarkdown, printNote } from "../importExport";
  import { displayUrl, isOpenableUrl } from "../links";
  import { tableMenuItems } from "../tableMenu";
  import { extractText } from "../text";
  import { app } from "../stores/app.svelte";
  import { prefs, TEXT_SIZE_PX } from "../stores/prefs.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { DocJSON } from "../types";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import EditorToolbar from "./EditorToolbar.svelte";
  import LinkForm from "./LinkForm.svelte";
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
  /** The note whose edits are saved (null while a locked note is hidden). */
  let loadedId: string | null = null;
  /** Note id + whether its content was hidden when loaded: unlocking reloads it. */
  let loadedKey: string | null = null;

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
        "Mod-k": () => {
          openLinkEditor();
          return true;
        },
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

  // ----------------------------------------------------- find & highlights

  let findOpen = $state(false);
  let findQuery = $state("");
  let findInput = $state<HTMLInputElement>();
  const findInfo = $derived.by(() => {
    void version;
    const s = editor ? getSearch(editor.state) : undefined;
    return s && s.mode === "find" ? { count: s.matches.length, current: s.current } : { count: 0, current: -1 };
  });

  /** Words being searched across all notes, highlighted in the open note. */
  const searchWords = () => (app.searchResults ? app.query.trim() : "");

  function applySearchHighlight(ed: Editor, scroll: boolean) {
    const words = searchWords();
    setSearch(ed.view, words, "terms");
    const first = getSearch(ed.state)?.matches[0];
    if (words && scroll && first) scrollToPos(ed.view, first.from);
  }

  async function openFind() {
    findOpen = true;
    await tick();
    findInput?.focus();
    findInput?.select();
    if (findQuery) runFind();
  }

  function runFind() {
    const ed = editor;
    if (!ed) return;
    setSearch(ed.view, findQuery, "find", 0);
    if (findQuery && getSearch(ed.state)?.matches.length) goToMatch(ed.view, 0);
  }

  function step(delta: number) {
    const ed = editor;
    if (ed && findInfo.count) goToMatch(ed.view, findInfo.current + delta);
  }

  function closeFind() {
    findOpen = false;
    const ed = editor;
    if (!ed) return;
    applySearchHighlight(ed, false); // back to search-word highlights (or none)
    ed.commands.focus();
  }

  function onFindKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      step(e.shiftKey ? -1 : 1);
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeFind();
    }
  }

  $effect(() => {
    if (app.findInNoteTick) untrack(() => void openFind());
  });

  // Search words changed while a note is open: update its highlights.
  $effect(() => {
    void app.query;
    void app.searchResults;
    const ed = editor;
    if (ed) untrack(() => !findOpen && applySearchHighlight(ed, true));
  });

  // ------------------------------------------------------------------ links

  /** Bubble under a link while the cursor is in it: address, Open, Edit, Remove. */
  let linkBubble = $state<{ href: string; x: number; y: number } | null>(null);
  /** The add/edit link box (Ctrl+K). */
  let linkForm = $state<{ x: number; y: number; href: string; withText: boolean; editing: boolean } | null>(null);

  /** Position just under document position `pos`, relative to the scrolling area. */
  function positionAt(ed: Editor, pos: number, width: number) {
    const c = ed.view.coordsAtPos(pos);
    const r = scroller.getBoundingClientRect();
    return {
      x: Math.max(8, Math.min(c.left - r.left, scroller.clientWidth - width - 8)),
      y: c.bottom - r.top + scroller.scrollTop + 6,
    };
  }

  function updateLinkBubble(ed: Editor) {
    const { selection } = ed.state;
    if (linkForm || !selection.empty || !ed.isActive("link")) {
      linkBubble = null;
      return;
    }
    const range = getMarkRange(selection.$from, ed.schema.marks.link);
    const href = ed.getAttributes("link").href as string | undefined;
    linkBubble = range && href ? { href, ...positionAt(ed, range.from, 320) } : null;
  }

  function openUrlSafely(href: string) {
    if (!isOpenableUrl(href)) return;
    if (isTauri) openUrl(href).catch((e) => app.showError(`Couldn't open the link: ${e}`));
    else window.open(href, "_blank", "noopener");
  }

  /** Ctrl+K: add a link to the selection, edit the link under the cursor, or insert a new one. */
  function openLinkEditor() {
    const ed = editor;
    if (!ed || !ed.isEditable) return;
    const inLink = ed.isActive("link");
    if (inLink) ed.commands.extendMarkRange("link");
    const { from, empty } = ed.state.selection;
    linkBubble = null;
    linkForm = {
      ...positionAt(ed, from, 340),
      href: inLink ? ((ed.getAttributes("link").href as string) ?? "") : "",
      withText: empty && !inLink,
      editing: inLink,
    };
  }

  function saveLink(href: string, text: string) {
    const ed = editor;
    const form = linkForm;
    linkForm = null;
    if (!ed || !form) return;
    if (form.withText) {
      const label = text || href.replace(/^mailto:/i, "");
      ed.chain().focus().insertContent({ type: "text", text: label, marks: [{ type: "link", attrs: { href } }] }).run();
    } else {
      ed.chain().focus().extendMarkRange("link").setLink({ href }).run();
    }
  }

  function removeLink() {
    linkForm = null;
    editor?.chain().focus().extendMarkRange("link").unsetLink().run();
  }

  function cancelLink() {
    linkForm = null;
    editor?.commands.focus();
  }

  /** Ctrl+click (or middle-click) a link to open it in the web browser. */
  function openLink(event: MouseEvent): boolean {
    const link = (event.target as HTMLElement).closest("a[href]") as HTMLAnchorElement | null;
    if (!link || !(event.ctrlKey || event.metaKey || event.button === 1)) return false;
    event.preventDefault();
    openUrlSafely(link.href);
    return true;
  }

  // ------------------------------------------------------------ attachments

  /** Upload files and insert them at `pos` (or at the cursor). */
  async function addFiles(files: File[], pos?: number) {
    const ed = editor;
    const noteId = loadedId;
    if (!ed || !noteId || !ed.isEditable || files.length === 0) return;
    const big = files.some((f) => f.size > 2 * 1024 * 1024);
    if (big) app.showInfo(files.length === 1 ? `Adding “${files[0].name}”…` : `Adding ${files.length} files…`);
    const nodes = [];
    for (const file of files) {
      try {
        nodes.push(nodeForAttachment(await api.addAttachment(noteId, file)));
      } catch (e) {
        app.showError(e);
      }
    }
    // If the user switched notes meanwhile, don't insert into the wrong one.
    if (nodes.length === 0 || loadedId !== noteId) return;
    insertAttachments(ed, nodes, pos);
    if (big) app.toast = null;
  }

  /**
   * Insert attachment nodes, then put the cursor on the line after them (adding an empty
   * line if needed), like Apple Notes. Otherwise the inserted item stays selected and the
   * next paste or keystroke would replace it.
   */
  function insertAttachments(ed: Editor, nodes: ReturnType<typeof nodeForAttachment>[], pos?: number) {
    const chain = ed.chain().focus();
    (pos != null ? chain.insertContentAt(pos, nodes) : chain.insertContent(nodes)).run();
    const sel = ed.state.selection;
    if (!(sel instanceof NodeSelection) && sel.$from.parent.isTextblock) return;
    const after = sel.to;
    const next = ed.state.doc.resolve(after).nodeAfter;
    if (next?.isTextblock) ed.commands.setTextSelection(after + 1);
    else ed.chain().insertContentAt(after, { type: "paragraph" }).setTextSelection(after + 1).run();
  }

  /** Toolbar paperclip: pick files with the system file picker. */
  async function attachFromPicker() {
    const ed = editor;
    const noteId = loadedId;
    if (!ed || !noteId || !ed.isEditable) return;
    try {
      const { attachments, errors } = await api.attachFiles(noteId);
      errors.forEach((e) => app.showError(e));
      if (attachments.length && loadedId === noteId) insertAttachments(ed, attachments.map(nodeForAttachment));
    } catch (e) {
      app.showError(e);
    }
  }

  function handlePaste(event: ClipboardEvent): boolean {
    const data = event.clipboardData;
    const files = [...(data?.files ?? [])];
    if (files.length === 0) return false;
    // Apps like LibreOffice put a picture of copied text on the clipboard as well;
    // when there's text, paste the text.
    if (data?.getData("text/plain").trim()) return false;
    event.preventDefault();
    void addFiles(files);
    return true;
  }

  function handleDrop(view: EditorView, event: DragEvent, moved: boolean): boolean {
    const dt = event.dataTransfer;
    // A note dragged from the note list isn't content for this note.
    if (dt?.types.includes(NOTE_DRAG_TYPE)) {
      event.preventDefault();
      return true;
    }
    if (moved || !dt?.files?.length) return false;
    event.preventDefault();
    const pos = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos;
    void addFiles([...dt.files], pos);
    return true;
  }

  function attachmentMenu(id: string): MenuItem[] {
    return [
      { label: "Open", action: () => api.openAttachment(id).catch((e) => app.showError(e)) },
      {
        label: "Save a Copy…",
        action: () =>
          api
            .saveAttachmentAs(id)
            .then((saved) => saved && app.showInfo("Saved"))
            .catch((e) => app.showError(e)),
      },
      { kind: "separator" },
      { label: "Delete", action: () => editor?.chain().focus().deleteSelection().run(), danger: true },
    ];
  }

  /** Right-click: actions for images/files and table cells instead of the browser menu. */
  function openContextMenu(view: EditorView, event: MouseEvent): boolean {
    const ed = editor;
    if (!ed) return false;
    const target = event.target as HTMLElement;
    const attachmentEl = target.closest<HTMLElement>("[data-attachment-id]");
    if (attachmentEl) {
      event.preventDefault();
      const id = attachmentEl.dataset.attachmentId!;
      const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
      if (at && at.inside >= 0) ed.commands.setNodeSelection(at.inside);
      const items = attachmentMenu(id);
      tableMenu = { x: event.clientX, y: event.clientY, items: ed.isEditable ? items : items.slice(0, 2) };
      return true;
    }
    if (target.closest('ul[data-type="taskList"]') && ed.isEditable) {
      const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
      if (!at) return false;
      event.preventDefault();
      // Inside a table cell, the table actions are still one click away.
      const inTable = !!target.closest("td, th");
      const { total, checked } = checklistActions.counts(ed, at.pos);
      const items: MenuItem[] = [
        { label: "Check All", action: () => checklistActions.setAll(ed, at.pos, true), disabled: checked === total },
        { label: "Uncheck All", action: () => checklistActions.setAll(ed, at.pos, false), disabled: checked === 0 },
        { label: "Move Checked to Bottom", action: () => checklistActions.moveCheckedToBottom(ed, at.pos), disabled: checked === 0 },
        { kind: "separator" },
        {
          label: checked ? `Delete ${checked} Checked Item${checked === 1 ? "" : "s"}` : "Delete Checked Items",
          action: () => checklistActions.deleteChecked(ed, at.pos),
          disabled: checked === 0,
          danger: true,
        },
      ];
      if (inTable) {
        ed.chain().setTextSelection(at.pos).run();
        items.push({ kind: "separator" }, { kind: "heading", label: "Table" }, ...tableMenuItems(ed));
      }
      tableMenu = { x: event.clientX, y: event.clientY, items };
      return true;
    }
    return openTableMenu(view, event);
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

  /** "⋯" button: export, print and import. */
  function openMoreMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const cur = app.current;
    // A locked note that's hidden has nothing to show, export or copy.
    const visible = !!cur && cur.content !== null;
    const active = !!cur && !cur.deletedAt;
    tableMenu = {
      x: r.right - 230,
      y: r.bottom + 4,
      items: [
        cur?.isLocked
          ? { label: "Remove Lock", action: () => app.removeLock(cur.id), disabled: !active }
          : { label: "Lock Note…", action: () => cur && app.lockNote(cur.id), disabled: !active },
        ...(app.lock.unlocked ? [{ label: "Lock Now", action: () => app.lockNow() }] : []),
        { kind: "separator" },
        { label: "Duplicate Note", action: () => cur && app.duplicateNote(cur.id), disabled: !active || !!cur?.isLocked },
        { label: "Note Info…", action: () => app.flush().then(() => (app.showNoteInfo = true)), disabled: !visible },
        { kind: "separator" },
        { label: "Export as Markdown…", action: exportMarkdown, disabled: !visible },
        { label: "Export as PDF…", action: exportPdf, disabled: !visible },
        { label: "Print…", action: printNote, disabled: !visible },
        { kind: "separator" },
        { label: "Import Markdown Files…", action: importMarkdown },
      ],
    };
  }

  /** Clicking the empty space below the text puts the cursor at the end, like Apple Notes. */
  function onScrollerClick(e: MouseEvent) {
    if (e.target === scroller && editor?.isEditable) editor.commands.focus("end");
  }

  onMount(() => {
    const ed = new Editor({
      element,
      extensions: [
        ...contentExtensions(),
        // Settings → General → "Move checked items to the bottom".
        ChecklistSort.configure({ enabled: () => prefs.sortChecked }),
        SearchHighlight,
        Placeholder.configure({ placeholder: "Start writing…" }),
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
          contextmenu: (view, event) => openContextMenu(view, event),
          // Double-click an image or file to open it in its usual app.
          dblclick: (_view, event) => {
            const el = (event.target as HTMLElement).closest<HTMLElement>("[data-attachment-id]");
            if (!el?.dataset.attachmentId) return false;
            event.preventDefault();
            api.openAttachment(el.dataset.attachmentId).catch((e) => app.showError(e));
            return true;
          },
        },
        handlePaste: (_view, event) => handlePaste(event),
        handleDrop: (view, event, _slice, moved) => handleDrop(view, event, moved),
      },
      onTransaction: ({ editor: e }) => {
        version++;
        updateLinkBubble(e);
      },
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
    const hiddenNow = !!note && note.content === null;
    const key = note ? `${note.id}${hiddenNow ? ":hidden" : ""}` : null;
    if (key !== loadedKey) {
      loadedKey = key;
      loadedId = note && !hiddenNow ? note.id : null;
      const json = note?.content ? $state.snapshot(note.content) : { type: "doc", content: [{ type: "paragraph" }] };
      let doc;
      unsupported = false;
      tableMenu = null;
      linkForm = null;
      linkBubble = null;
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
      // A fresh state has no highlights: re-run the find bar or search-word highlights.
      if (findOpen && findQuery) runFind();
      else applySearchHighlight(ed, true);
      version++;
    }
    const editable = !!note && !note.deletedAt && !unsupported && !hiddenNow;
    if (ed.isEditable !== editable) ed.setEditable(editable, false);
  }

  $effect(() => {
    const ed = editor;
    if (app.focusEditorTick && ed) untrack(() => ed.commands.focus("end"));
  });

  const note = $derived(app.current);
  /** A locked note whose content is hidden until the password is entered. */
  const hidden = $derived(!!note && note.content === null);
  const readOnly = $derived(!note || !!note.deletedAt || unsupported || hidden);
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
    <EditorToolbar {editor} {version} disabled={readOnly} onattach={attachFromPicker} onlink={openLinkEditor} />
    <div class="spacer" data-tauri-drag-region></div>
    <button class="icon-btn" title="Export, print or import" aria-label="More actions" onclick={openMoreMenu}>
      <Ellipsis size={17} />
    </button>
    {#if note?.isLocked && !hidden}
      <button class="icon-btn lock-now" title="Lock Now: hide all locked notes" aria-label="Lock Now" onclick={() => app.lockNow()}>
        <LockOpen size={17} />
      </button>
    {/if}
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

  {#if findOpen && note && !hidden}
    <div class="find-bar" role="search">
      <Search size={14} />
      <input
        bind:this={findInput}
        bind:value={findQuery}
        oninput={runFind}
        onkeydown={onFindKey}
        placeholder="Find in note"
        aria-label="Find in note"
        spellcheck="false"
      />
      <span class="find-count" aria-live="polite">
        {findQuery ? (findInfo.count ? `${findInfo.current + 1} of ${findInfo.count}` : "No results") : ""}
      </span>
      <button class="icon-btn" aria-label="Previous match" title="Previous (Shift+Enter)" disabled={!findInfo.count} onclick={() => step(-1)}>
        <ChevronUp size={16} />
      </button>
      <button class="icon-btn" aria-label="Next match" title="Next (Enter)" disabled={!findInfo.count} onclick={() => step(1)}>
        <ChevronDown size={16} />
      </button>
      <button class="find-done" onclick={closeFind}>Done</button>
    </div>
  {/if}

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
    class:hidden={!note || hidden}
    style:--editor-font-size="{TEXT_SIZE_PX[prefs.textSize]}px"
    bind:this={scroller}
    onclick={onScrollerClick}
  >
    <div class="date">{note ? formatEditorDate(note.updatedAt) : ""}</div>
    <!-- TipTap owns everything inside this element. -->
    <div bind:this={element}></div>

    {#if linkBubble && !linkForm}
      <!-- mousedown is prevented so the editor keeps its cursor while a button is clicked -->
      <div class="link-bubble" role="toolbar" aria-label="Link" tabindex="-1" style:left="{linkBubble.x}px" style:top="{linkBubble.y}px" onmousedown={(e) => e.preventDefault()}>
        <button class="url" title={linkBubble.href} onclick={() => openUrlSafely(linkBubble!.href)}>{displayUrl(linkBubble.href)}</button>
        <span class="sep"></span>
        <button onclick={() => openUrlSafely(linkBubble!.href)}>Open</button>
        {#if editor?.isEditable}
          <button onclick={openLinkEditor}>Edit</button>
          <button onclick={removeLink}>Remove</button>
        {/if}
      </div>
    {/if}
    {#if linkForm}
      <LinkForm {...linkForm} onsave={saveLink} onremove={removeLink} oncancel={cancelLink} />
    {/if}
  </div>

  {#if hidden}
    <LockedNote />
  {/if}

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
  .lock-now {
    color: var(--accent-strong);
  }
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
    /* The toolbar hides buttons based on this width (see EditorToolbar). */
    container: editor-header / inline-size;
  }
  .header > :global(*) {
    flex: none;
  }
  .header > :global(.toolbar) {
    flex: 0 1 auto;
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
    position: relative;
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

  /* ------------------------------------------------------- find & search */
  .find-bar {
    position: absolute;
    top: calc(var(--header-h) + 6px);
    right: 16px;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 4px;
    width: min(360px, calc(100% - 32px));
    padding: 4px 4px 4px 10px;
    border-radius: var(--radius-md);
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    color: var(--text-secondary);
  }
  .find-bar input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: none;
    background: none;
    outline: none;
    color: var(--text);
  }
  .find-count {
    flex: none;
    font-size: 12px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .find-done {
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    font-weight: 600;
    color: var(--accent-strong);
  }
  .find-done:hover {
    background: var(--bg-hover);
  }
  .scroll :global(.search-match) {
    border-radius: 2px;
    background: color-mix(in srgb, var(--accent) 38%, transparent);
  }
  .scroll :global(.search-match.current) {
    background: var(--accent);
    color: var(--accent-contrast);
  }

  /* ----------------------------------------------------------------- links */
  .link-bubble {
    position: absolute;
    z-index: 25;
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: 320px;
    padding: 4px;
    border-radius: var(--radius-md);
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    font-size: 12px;
    user-select: none;
    cursor: default;
  }
  .link-bubble button {
    padding: 4px 8px;
    border-radius: 5px;
    font-weight: 600;
    white-space: nowrap;
  }
  .link-bubble button:hover {
    background: var(--bg-hover);
  }
  .link-bubble .url {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--accent-strong);
    font-weight: 500;
  }
  .link-bubble .sep {
    width: 1px;
    height: 16px;
    margin: 0 2px;
    background: var(--separator-strong);
  }

  /* ----------------------------------------------------------- attachments */
  .scroll :global(.prose img.note-image) {
    display: block;
    max-width: 100%;
    max-height: 75vh;
    height: auto;
    margin: 0.6em 0;
    border-radius: var(--radius-md);
    cursor: default;
  }
  .scroll :global(.prose .note-file) {
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: 440px;
    margin: 0.5em 0;
    padding: 8px 12px 8px 8px;
    border: 1px solid var(--separator-strong);
    border-radius: var(--radius-lg);
    background: var(--bg-input);
    cursor: default;
    user-select: none;
  }
  .scroll :global(.prose .note-file:hover) {
    background: var(--bg-active);
  }
  .scroll :global(.note-file-icon) {
    flex: none;
    display: grid;
    place-items: end center;
    width: 32px;
    height: 40px;
    padding-bottom: 5px;
    border-radius: 4px 10px 4px 4px;
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: 9px;
    font-weight: 800;
    letter-spacing: 0.02em;
  }
  .scroll :global(.note-file-text) {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .scroll :global(.note-file-name) {
    font-weight: 600;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .scroll :global(.note-file-size) {
    font-size: 0.8em;
    color: var(--text-secondary);
  }
  .scroll :global(.prose .ProseMirror-selectednode.note-image),
  .scroll :global(.prose .ProseMirror-selectednode.note-file) {
    outline: 3px solid var(--accent);
    outline-offset: 2px;
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
