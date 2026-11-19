// Central UI state. Components read these fields directly; all mutations go
// through methods so autosave, list ordering and counts stay consistent.

import { api } from "../api";
import { prefs, type SortBy } from "./prefs.svelte";
import type { DocJSON, Folder, Note, NoteFilter, NoteSummary } from "../types";
import { sameFilter } from "../types";

const SAVE_DELAY_MS = 400;
const SEARCH_DELAY_MS = 150;

export type SettingsSection = "general" | "appearance" | "backup" | "shortcuts" | "about";

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel: string;
  destructive?: boolean;
  resolve: (ok: boolean) => void;
}

/** Note-list order: pinned first, then by the "Sort notes by" setting. */
export function compareNotes(a: NoteSummary, b: NoteSummary, sortBy: SortBy = prefs.sortBy): number {
  const pinned = Number(b.isPinned) - Number(a.isPinned);
  if (pinned) return pinned;
  switch (sortBy) {
    case "created":
      return b.createdAt - a.createdAt;
    case "title":
      return (
        (a.title || "New Note").localeCompare(b.title || "New Note", undefined, { sensitivity: "base", numeric: true }) ||
        b.updatedAt - a.updatedAt
      );
    default:
      return b.updatedAt - a.updatedAt;
  }
}

function toSummary(note: Note): NoteSummary {
  const { content: _content, ...rest } = note;
  return { ...rest, snippet: "" };
}

class AppState {
  folders = $state<Folder[]>([]);
  trashCount = $state(0);
  filter = $state<NoteFilter>({ kind: "all" });
  notes = $state<NoteSummary[]>([]);
  selectedId = $state<string | null>(null);
  /** The note open in the editor. Raw (not deeply reactive): reassign, don't mutate. */
  current = $state.raw<Note | null>(null);
  query = $state("");
  searchResults = $state<NoteSummary[] | null>(null);
  /** Bottom-of-window message. Errors stay longer than info messages. */
  toast = $state<{ kind: "error" | "info"; message: string } | null>(null);
  /** Section shown in the Settings window, or null when it's closed. */
  settingsSection = $state<SettingsSection | null>(null);
  confirmRequest = $state<ConfirmRequest | null>(null);
  /** Bumped when the editor should take keyboard focus (e.g. after "New Note"). */
  focusEditorTick = $state(0);
  /** Bumped when the search field should take focus (Ctrl+F). */
  focusSearchTick = $state(0);
  /** Show the inline "new folder" field in the sidebar. */
  creatingFolder = $state(false);

  private pending: { id: string; getContent: () => DocJSON } | null = null;
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  private loadToken = 0;
  private toastTimer: ReturnType<typeof setTimeout> | null = null;

  // ------------------------------------------------------------- derived

  get visibleNotes(): NoteSummary[] {
    return this.searchResults ?? this.notes;
  }

  get allCount(): number {
    return this.folders.reduce((sum, f) => sum + f.noteCount, 0);
  }

  get isTrash(): boolean {
    return this.filter.kind === "trash";
  }

  get listTitle(): string {
    if (this.searchResults) return "Search Results";
    switch (this.filter.kind) {
      case "all":
        return "All Notes";
      case "trash":
        return "Recently Deleted";
      case "folder": {
        const id = this.filter.folderId;
        return this.folders.find((f) => f.id === id)?.name ?? "Notes";
      }
    }
  }

  folderName(id: string): string {
    return this.folders.find((f) => f.id === id)?.name ?? "";
  }

  // ------------------------------------------------------------ plumbing

  showError(err: unknown) {
    this.setToast("error", err instanceof Error ? err.message : String(err), 8000);
    console.error(err);
  }

  showInfo(message: string) {
    this.setToast("info", message, 4000);
  }

  private setToast(kind: "error" | "info", message: string, ms: number) {
    this.toast = { kind, message };
    if (this.toastTimer) clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => (this.toast = null), ms);
  }

  private async guard<T>(fn: () => Promise<T>): Promise<T | undefined> {
    try {
      return await fn();
    } catch (err) {
      this.showError(err);
      return undefined;
    }
  }

  confirm(opts: Omit<ConfirmRequest, "resolve">): Promise<boolean> {
    this.confirmRequest?.resolve(false);
    return new Promise((resolve) => {
      this.confirmRequest = {
        ...opts,
        resolve: (ok) => {
          this.confirmRequest = null;
          resolve(ok);
        },
      };
    });
  }

  private replaceSummary(updated: NoteSummary) {
    const swap = (list: NoteSummary[]) => list.map((n) => (n.id === updated.id ? updated : n));
    this.notes = swap(this.notes);
    if (!this.isTrash) this.notes.sort((a, b) => compareNotes(a, b));
    if (this.searchResults) this.searchResults = swap(this.searchResults);
    if (this.current?.id === updated.id) {
      this.current = {
        ...this.current,
        title: updated.title,
        isPinned: updated.isPinned,
        folderId: updated.folderId,
        updatedAt: updated.updatedAt,
        deletedAt: updated.deletedAt,
      };
    }
  }

  private removeFromLists(id: string): string | null {
    // Pick the neighbour so the selection doesn't jump to the top.
    const list = this.visibleNotes;
    const idx = list.findIndex((n) => n.id === id);
    const next = idx >= 0 ? (list[idx + 1] ?? list[idx - 1] ?? null) : null;
    this.notes = this.notes.filter((n) => n.id !== id);
    if (this.searchResults) this.searchResults = this.searchResults.filter((n) => n.id !== id);
    return next?.id ?? null;
  }

  async refreshCounts() {
    await this.guard(async () => {
      const [folders, trashCount] = await Promise.all([api.listFolders(), api.trashCount()]);
      this.folders = folders;
      this.trashCount = trashCount;
    });
  }

  // ------------------------------------------------------------ lifecycle

  async init() {
    await this.refreshCounts();
    await this.loadNotes();
    await this.selectNote(this.notes[0]?.id ?? null);
  }

  /** Reload everything from the backend, e.g. after restoring a backup. */
  async reloadAll() {
    this.pending = null;
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.clearSearch();
    this.filter = { kind: "all" };
    this.loadToken++;
    this.selectedId = null;
    this.current = null;
    await this.init();
  }

  private async loadNotes() {
    const notes = await this.guard(() => api.listNotes(this.filter));
    if (!notes) return;
    // Recently Deleted keeps the backend's order (most recently deleted first).
    this.notes = this.isTrash ? notes : notes.sort((a, b) => compareNotes(a, b));
  }

  /** Re-apply the "Sort notes by" setting after it changes. */
  resortNotes() {
    if (!this.isTrash) this.notes = [...this.notes].sort((a, b) => compareNotes(a, b));
  }

  // ------------------------------------------------------------- autosave

  /** Called by the editor on every change; the actual save is debounced. */
  markDirty(id: string, getContent: () => DocJSON) {
    this.pending = { id, getContent };
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => void this.flush(), SAVE_DELAY_MS);
  }

  /** Save any pending edit immediately. Safe to call at any time. */
  async flush() {
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    const pending = this.pending;
    if (!pending) return;
    this.pending = null;
    // Read the content synchronously, before anything else can change the editor.
    const content = pending.getContent();
    const summary = await this.guard(() => api.updateNoteContent(pending.id, content));
    if (summary) this.replaceSummary(summary);
  }

  // ----------------------------------------------------------- navigation

  async setFilter(filter: NoteFilter) {
    await this.flush();
    if (sameFilter(filter, this.filter) && !this.searchResults) return;
    await this.discardIfLeavingBlank(null);
    this.filter = filter;
    this.clearSearch();
    await this.loadNotes();
    await this.selectNote(this.notes[0]?.id ?? null);
  }

  async selectNote(id: string | null) {
    await this.flush();
    await this.discardIfLeavingBlank(id);
    const token = ++this.loadToken;
    this.selectedId = id;
    if (!id) {
      this.current = null;
      return;
    }
    const note = await this.guard(() => api.getNote(id));
    // Ignore stale responses when the user clicks quickly through the list.
    if (token === this.loadToken && note) this.current = note;
  }

  // ---------------------------------------------------------------- search

  setQuery(query: string) {
    this.query = query;
    if (this.searchTimer) clearTimeout(this.searchTimer);
    if (!query.trim()) {
      this.searchResults = null;
      return;
    }
    this.searchTimer = setTimeout(async () => {
      await this.flush();
      const results = await this.guard(() => api.searchNotes(query));
      if (results && this.query === query) this.searchResults = results;
    }, SEARCH_DELAY_MS);
  }

  clearSearch() {
    if (this.searchTimer) clearTimeout(this.searchTimer);
    this.query = "";
    this.searchResults = null;
  }

  // ----------------------------------------------------------------- notes

  /**
   * Like Apple Notes: a note that was created but left without any content is
   * deleted when you move away from it, instead of cluttering the list.
   */
  private async discardIfLeavingBlank(nextId: string | null) {
    const prev = this.current;
    // Cheap check first; the backend does the strict "is it really empty" test.
    if (!prev || prev.id === nextId || prev.deletedAt || prev.title !== "") return;
    const removed = await this.guard(() => api.discardNoteIfBlank(prev.id));
    if (!removed) return;
    this.notes = this.notes.filter((n) => n.id !== prev.id);
    if (this.searchResults) this.searchResults = this.searchResults.filter((n) => n.id !== prev.id);
    if (this.current?.id === prev.id) {
      this.current = null;
      this.selectedId = null;
    }
    await this.refreshCounts();
  }

  async newNote() {
    await this.flush();
    await this.discardIfLeavingBlank(null);
    if (this.isTrash) {
      this.filter = { kind: "all" };
    }
    this.clearSearch();
    const folderId = this.filter.kind === "folder" ? this.filter.folderId : null;
    const note = await this.guard(() => api.createNote(folderId));
    if (!note) return;
    await this.loadNotes();
    if (!this.notes.some((n) => n.id === note.id)) this.notes.unshift(toSummary(note));
    this.loadToken++;
    this.selectedId = note.id;
    this.current = note;
    this.focusEditorTick++;
    await this.refreshCounts();
  }

  async togglePin(id: string) {
    await this.flush();
    const note = this.visibleNotes.find((n) => n.id === id);
    if (!note) return;
    const updated = await this.guard(() => api.setNotePinned(id, !note.isPinned));
    if (updated) this.replaceSummary(updated);
  }

  async moveNote(id: string, folderId: string) {
    await this.flush();
    const updated = await this.guard(() => api.moveNote(id, folderId));
    if (!updated) return;
    if (this.filter.kind === "folder" && this.filter.folderId !== folderId) {
      const next = this.removeFromLists(id);
      if (this.selectedId === id) await this.selectNote(next);
    } else {
      this.replaceSummary(updated);
    }
    await this.refreshCounts();
  }

  async trashNote(id: string) {
    await this.flush();
    const ok = await this.guard(() => api.trashNote(id));
    if (!ok) return;
    const next = this.removeFromLists(id);
    if (this.selectedId === id) await this.selectNote(next);
    await this.refreshCounts();
  }

  async restoreNote(id: string) {
    const restored = await this.guard(() => api.restoreNote(id));
    if (!restored) return;
    const next = this.removeFromLists(id);
    if (this.selectedId === id) await this.selectNote(next);
    await this.refreshCounts();
  }

  async deleteNoteForever(id: string) {
    const ok = await this.confirm({
      title: "Delete this note permanently?",
      message: "You can't undo this action.",
      confirmLabel: "Delete",
      destructive: true,
    });
    if (!ok) return;
    const done = await this.guard(() => api.deleteNotePermanently(id).then(() => true));
    if (!done) return;
    const next = this.removeFromLists(id);
    if (this.selectedId === id) await this.selectNote(next);
    await this.refreshCounts();
  }

  /** Delete from the current view: to trash normally, permanently when already in trash. */
  async deleteNote(id: string) {
    if (this.isTrash) await this.deleteNoteForever(id);
    else await this.trashNote(id);
  }

  async emptyTrash() {
    const count = this.trashCount;
    if (!count) return;
    const ok = await this.confirm({
      title: "Empty Recently Deleted?",
      message: `${count} note${count === 1 ? "" : "s"} will be deleted permanently. You can't undo this action.`,
      confirmLabel: "Delete All",
      destructive: true,
    });
    if (!ok) return;
    await this.guard(() => api.emptyTrash());
    await this.loadNotes();
    await this.selectNote(this.notes[0]?.id ?? null);
    await this.refreshCounts();
  }

  // --------------------------------------------------------------- folders

  async createFolder(name: string): Promise<boolean> {
    const folder = await this.guard(() => api.createFolder(name));
    if (!folder) return false;
    this.creatingFolder = false;
    await this.refreshCounts();
    await this.setFilter({ kind: "folder", folderId: folder.id });
    return true;
  }

  async renameFolder(id: string, name: string): Promise<boolean> {
    const folder = await this.guard(() => api.renameFolder(id, name));
    if (!folder) return false;
    await this.refreshCounts();
    return true;
  }

  async deleteFolder(id: string) {
    const folder = this.folders.find((f) => f.id === id);
    if (!folder) return;
    const n = folder.noteCount;
    const ok = await this.confirm({
      title: `Delete “${folder.name}”?`,
      message:
        n === 0
          ? "This folder is empty."
          : `${n} note${n === 1 ? "" : "s"} in this folder will be moved to Recently Deleted.`,
      confirmLabel: "Delete Folder",
      destructive: true,
    });
    if (!ok) return;
    await this.flush();
    const moved = await this.guard(() => api.deleteFolder(id));
    if (moved === undefined) return;
    await this.refreshCounts();
    if (this.filter.kind === "folder" && this.filter.folderId === id) {
      await this.setFilter({ kind: "all" });
    } else {
      await this.loadNotes();
      if (this.selectedId && !this.notes.some((x) => x.id === this.selectedId)) {
        await this.selectNote(this.notes[0]?.id ?? null);
      }
    }
  }
}

export const app = new AppState();
