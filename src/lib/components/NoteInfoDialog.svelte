<script lang="ts">
  // ⋯ → Note Info…: word count, dates, folder and attachments of the open note.
  import { onMount } from "svelte";
  import { X } from "@lucide/svelte";
  import { api } from "../api";
  import { folderPath } from "../folders";
  import { formatEditorDate } from "../format";
  import { app } from "../stores/app.svelte";
  import { extractText } from "../text";
  import type { Note } from "../types";

  let note = $state<Note | null>(null);
  let dialog = $state<HTMLDivElement>();

  onMount(async () => {
    dialog?.focus();
    const id = app.current?.id;
    if (!id) return;
    try {
      note = await api.getNote(id); // the saved version, including the latest edits
    } catch (e) {
      app.showError(e);
      app.showNoteInfo = false;
    }
  });

  type Walkable = { type?: string; attrs?: Record<string, unknown>; content?: Walkable[] };

  const stats = $derived.by(() => {
    if (!note) return null;
    const plain = extractText(note.content).plain;
    const words = plain.match(/[\p{L}\p{N}]+(?:['’-][\p{L}\p{N}]+)*/gu)?.length ?? 0;
    const characters = [...plain.replace(/\n/g, "")].length;
    let tasks = 0;
    let done = 0;
    const attachments = new Set<string>();
    const walk = (n: Walkable) => {
      if (n.type === "taskItem") {
        tasks++;
        if (n.attrs?.checked) done++;
      }
      const id = n.attrs?.attachmentId;
      if (typeof id === "string") attachments.add(id);
      n.content?.forEach(walk);
    };
    walk(note.content as Walkable);
    return { words, characters, tasks, done, attachments: attachments.size };
  });

  function close() {
    app.showNoteInfo = false;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Note info" tabindex="-1" bind:this={dialog}>
    <header>
      <h2>Note Info</h2>
      <button class="icon-btn" aria-label="Close" onclick={close}><X size={16} /></button>
    </header>
    {#if note && stats}
      <p class="title">{note.title || "New Note"}</p>
      <dl>
        <dt>Words</dt>
        <dd>{stats.words.toLocaleString()}</dd>
        <dt>Characters</dt>
        <dd>{stats.characters.toLocaleString()}</dd>
        {#if stats.tasks}
          <dt>Checklist</dt>
          <dd>{stats.done} of {stats.tasks} done</dd>
        {/if}
        {#if stats.attachments}
          <dt>Attachments</dt>
          <dd>{stats.attachments}</dd>
        {/if}
        <dt>Folder</dt>
        <dd>{note.deletedAt ? "Recently Deleted" : folderPath(app.folders, note.folderId)}</dd>
        <dt>Created</dt>
        <dd>{formatEditorDate(note.createdAt)}</dd>
        <dt>Modified</dt>
        <dd>{formatEditorDate(note.updatedAt)}</dd>
      </dl>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 96;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    width: min(360px, calc(100vw - 40px));
    padding: 16px 20px 18px;
    border-radius: var(--radius-lg);
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    outline: none;
    user-select: text;
    -webkit-user-select: text;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
  }
  header .icon-btn {
    margin-right: -8px;
  }
  .title {
    margin: 4px 0 12px;
    color: var(--text-secondary);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px 16px;
    margin: 0;
    padding: 12px 14px;
    border-radius: var(--radius-md);
    background: var(--bg-input);
  }
  dt {
    color: var(--text-secondary);
  }
  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
