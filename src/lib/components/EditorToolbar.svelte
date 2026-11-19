<script lang="ts">
  import type { Editor } from "@tiptap/core";
  import {
    Bold,
    CaseSensitive,
    Italic,
    List,
    ListChecks,
    ListOrdered,
    Paperclip,
    Strikethrough,
    Table,
    Underline,
  } from "@lucide/svelte";
  import { insertTable, isInTable, tableMenuItems } from "../tableMenu";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";

  interface Props {
    editor: Editor | null;
    /** Changes on every editor transaction so active states re-render. */
    version: number;
    disabled: boolean;
    /** Paperclip button: attach files from the file picker. */
    onattach: () => void;
    /** Add or edit a link (same as Ctrl+K). */
    onlink: () => void;
  }
  let { editor, version, disabled, onattach, onlink }: Props = $props();

  let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

  // Re-evaluated whenever `version` changes.
  const active = $derived.by(() => {
    void version;
    const is = (name: string, attrs?: Record<string, unknown>) => !!editor?.isActive(name, attrs);
    return {
      bold: is("bold"),
      italic: is("italic"),
      underline: is("underline"),
      strike: is("strike"),
      taskList: is("taskList"),
      bulletList: is("bulletList"),
      orderedList: is("orderedList"),
      table: isInTable(editor),
    };
  });

  /** Outside a table: insert one. Inside: show the table actions. */
  function tableButton(e: MouseEvent) {
    if (!editor) return;
    if (!isInTable(editor)) {
      insertTable(editor);
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: r.left, y: r.bottom + 4, items: tableMenuItems(editor) };
  }

  const chain = () => editor!.chain().focus();

  function openStyles(e: MouseEvent) {
    if (!editor) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const mark = (on: boolean, label: string) => (on ? `✓  ${label}` : `     ${label}`);
    menu = {
      x: r.left,
      y: r.bottom + 4,
      items: [
        { label: mark(editor.isActive("heading", { level: 1 }), "Title"), action: () => chain().setHeading({ level: 1 }).run() },
        { label: mark(editor.isActive("heading", { level: 2 }), "Heading"), action: () => chain().setHeading({ level: 2 }).run() },
        { label: mark(editor.isActive("heading", { level: 3 }), "Subheading"), action: () => chain().setHeading({ level: 3 }).run() },
        { label: mark(editor.isActive("paragraph"), "Body"), action: () => chain().setParagraph().run() },
        { label: mark(editor.isActive("codeBlock"), "Monospaced"), action: () => chain().toggleCodeBlock().run() },
        { kind: "separator" },
        { label: mark(editor.isActive("bulletList"), "Bulleted List"), action: () => chain().toggleBulletList().run() },
        { label: mark(editor.isActive("orderedList"), "Numbered List"), action: () => chain().toggleOrderedList().run() },
        { label: mark(editor.isActive("blockquote"), "Block Quote"), action: () => chain().toggleBlockquote().run() },
        { kind: "separator" },
        { label: mark(editor.isActive("bold"), "Bold"), action: () => chain().toggleBold().run() },
        { label: mark(editor.isActive("italic"), "Italic"), action: () => chain().toggleItalic().run() },
        { label: mark(editor.isActive("underline"), "Underline"), action: () => chain().toggleUnderline().run() },
        { label: mark(editor.isActive("strike"), "Strikethrough"), action: () => chain().toggleStrike().run() },
        { kind: "separator" },
        { label: editor.isActive("link") ? "     Edit Link…   Ctrl+K" : "     Link…   Ctrl+K", action: onlink },
      ],
    };
  }
</script>

<div class="toolbar" role="toolbar" aria-label="Formatting">
  <button class="icon-btn" title="Text styles" aria-label="Text styles" {disabled} onclick={openStyles}>
    <CaseSensitive size={20} />
  </button>
  <span class="divider"></span>
  <!-- Hidden when the editor is narrow; the same actions are in the Aa menu. -->
  <span class="group marks">
  <button class="icon-btn" class:active={active.bold} title="Bold (Ctrl+B)" aria-label="Bold" {disabled} onclick={() => chain().toggleBold().run()}>
    <Bold size={16} />
  </button>
  <button class="icon-btn" class:active={active.italic} title="Italic (Ctrl+I)" aria-label="Italic" {disabled} onclick={() => chain().toggleItalic().run()}>
    <Italic size={16} />
  </button>
  <button class="icon-btn" class:active={active.underline} title="Underline (Ctrl+U)" aria-label="Underline" {disabled} onclick={() => chain().toggleUnderline().run()}>
    <Underline size={16} />
  </button>
  <button class="icon-btn" class:active={active.strike} title="Strikethrough (Ctrl+Shift+S)" aria-label="Strikethrough" {disabled} onclick={() => chain().toggleStrike().run()}>
    <Strikethrough size={16} />
  </button>
  <span class="divider"></span>
  </span>
  <button class="icon-btn" class:active={active.taskList} title="Checklist (Ctrl+Shift+L)" aria-label="Checklist" {disabled} onclick={() => chain().toggleTaskList().run()}>
    <ListChecks size={18} />
  </button>
  <span class="group lists">
  <button class="icon-btn" class:active={active.bulletList} title="Bulleted list" aria-label="Bulleted list" {disabled} onclick={() => chain().toggleBulletList().run()}>
    <List size={18} />
  </button>
  <button class="icon-btn" class:active={active.orderedList} title="Numbered list" aria-label="Numbered list" {disabled} onclick={() => chain().toggleOrderedList().run()}>
    <ListOrdered size={18} />
  </button>
  </span>
  <span class="divider"></span>
  <button
    class="icon-btn"
    class:active={active.table}
    title={active.table ? "Table options" : "Insert table"}
    aria-label={active.table ? "Table options" : "Insert table"}
    {disabled}
    onclick={tableButton}
  >
    <Table size={17} />
  </button>
  <button class="icon-btn" title="Attach images or files (or paste / drag them in)" aria-label="Attach files" {disabled} onclick={onattach}>
    <Paperclip size={17} />
  </button>
</div>

{#if menu}
  <ContextMenu {...menu} onclose={() => (menu = null)} />
{/if}

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    /* Last resort on very narrow windows: scroll instead of overlapping other controls. */
    overflow-x: auto;
    scrollbar-width: none;
  }
  .toolbar::-webkit-scrollbar {
    display: none;
  }
  .toolbar :global(.icon-btn),
  .divider {
    flex: none;
  }
  .group {
    display: contents;
  }
  /* Narrow editor: fewer buttons (bold/italic/… and lists are also in the Aa menu). */
  @container editor-header (max-width: 540px) {
    .marks {
      display: none;
    }
  }
  @container editor-header (max-width: 430px) {
    .lists {
      display: none;
    }
  }
  .divider {
    width: 1px;
    height: 18px;
    margin: 0 6px;
    background: var(--separator-strong);
  }
</style>
