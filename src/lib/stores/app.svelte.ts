// Central UI state. Components read these fields directly; all mutations go
// through methods so autosave, list ordering and counts stay consistent.

import { api, errorText } from "../api";
import { prefs, type SortBy } from "./prefs.svelte";
import type { DocJSON, Folder, LockStatus, Note, NoteFilter, NoteSummary } from "../types";
import { sameFilter } from "../types";
import { descendantIds } from "../folders";

const SAVE_DELAY_MS = 400;
const SEARCH_DELAY_MS = 150;

/** A button on a toast, e.g. "Undo". */
export interface ToastAction {
  label: string;
  run: () => void;
}

export type SettingsSection = "general" | "appearance" | "backup" | "shortcuts" | "about";

/** The password window: set the first password, unlock, or change it. */
export type LockPromptMode = "set" | "unlock" | "change";
export interface LockPrompt {
  mode: LockPromptMode;
  resolve: (ok: boolean) => void;
}

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
  toast = $state<{ kind: "error" | "info"; message: string; action?: ToastAction } | null>(null);
  /** The "Note Info" window is open. */
  showNoteInfo = $state(false);
  /** Section shown in the Settings window, or null when it's closed. */
  settingsSection = $state<SettingsSection | null>(null);
  confirmRequest = $state<ConfirmRequest | null>(null);
  /** Locked notes: is a password set, are they unlocked right now. */
  lock = $state<LockStatus>({ passwordSet: false, unlocked: false, hint: "", lockedNotes: 0 });
  lockPrompt = $state<LockPrompt | null>(null);
  /** Bumped when the editor should take keyboard focus (e.g. after "New Note"). */
  focusEditorTick = $state(0);
  /** Bumped when the search field should take focus (Ctrl+Shift+F, or Ctrl+F outside the editor). */
  focusSearchTick = $state(0);
  /** Bumped to open the find bar in the open note (Ctrl+F in the editor). */
  findInNoteTick = $state(0);
  /** Show the inline "new folder" field in the sidebar. */
  creatingFolder = $state(false);
  /** Where the folder being created goes (null = top level). */
  newFolderParent = $state<string | null>(null);

  private pending: { id: string; getContent: () => DocJSON } | null = null;
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  /** Saves that failed, by note: kept and retried so the changes aren't lost. */
  private unsaved = new Map<string, { content: DocJSON; tries: number }>();
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
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
    this.setToast("error", errorText(err), 8000);
    console.error(err);
  }

  showInfo(message: string, action?: ToastAction) {
    this.setToast("info", message, action ? 8000 : 4000, action);
  }

  /** Run the current toast's button (e.g. Undo), if there is one. */
  runToastAction(): boolean {
    const action = this.toast?.action;
    if (!action) return false;
    this.toast = null;
    action.run();
    return true;
  }

  private setToast(kind: "error" | "info", message: string, ms: number, action?: ToastAction) {
    this.toast = { kind, message, action };
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
        isLocked: updated.isLocked,
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
    await this.refreshLock();
    await this.refreshCounts();
    await this.loadNotes();
    await this.selectNote(this.notes[0]?.id ?? null);
  }

  /** Reload the note list (e.g. after importing) and open `selectId`. */
  async reloadNotes(selectId: string | null) {
    await this.flush();
    this.clearSearch();
    await this.loadNotes();
    await this.refreshCounts();
    await this.selectNote(selectId ?? this.notes[0]?.id ?? null);
  }

  /** Reload everything from the backend, e.g. after restoring a backup. */
  async reloadAll() {
    this.pending = null;
    if (this.saveTimer) clearTimeout(this.saveTimer);
    // The notes were replaced (e.g. restored): old unsaved changes no longer apply.
    this.unsaved.clear();
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
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

  /** Save any pending edit immediately (and retry earlier saves that failed). */
  async flush() {
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    const pending = this.pending;
    this.pending = null;
    // Read the content synchronously, before anything else can change the editor.
    const content = pending?.getContent();
    for (const [id, failed] of [...this.unsaved]) {
      if (id !== pending?.id) await this.save(id, failed.content);
    }
    if (pending && content) await this.save(pending.id, content);
  }

  /** Save one note's content. A failure keeps the content and retries it later. */
  private async save(id: string, content: DocJSON) {
    try {
      this.replaceSummary(await api.updateNoteContent(id, content));
      this.unsaved.delete(id);
    } catch (err) {
      const tries = (this.unsaved.get(id)?.tries ?? 0) + 1;
      this.unsaved.set(id, { content, tries });
      if (tries === 1) {
        this.showError(`Couldn't save your changes: ${errorText(err)} TuxNotes will keep trying.`);
      }
      this.scheduleRetry(tries);
    }
  }

  /** Retry failed saves after 2 s, 5 s, 15 s, 30 s, 60 s (and on every later save). */
  private scheduleRetry(tries: number) {
    const delays = [2_000, 5_000, 15_000, 30_000, 60_000];
    if (this.retryTimer || tries > delays.length) return;
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      void this.flush();
    }, delays[tries - 1]);
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

  // ---------------------------------------------------------- locked notes

  async refreshLock() {
    const status = await this.guard(() => api.lockStatus());
    if (status) this.lock = status;
  }

  /** Open the password window; resolves true once it succeeded. */
  askLockPassword(mode: LockPromptMode): Promise<boolean> {
    this.lockPrompt?.resolve(false);
    return new Promise((resolve) => {
      this.lockPrompt = {
        mode,
        resolve: (ok) => {
          this.lockPrompt = null;
          resolve(ok);
        },
      };
    });
  }

  /** Make sure locked notes are unlocked, asking for the password (or for a new one). */
  private async ensureUnlocked(): Promise<boolean> {
    await this.refreshLock();
    if (this.lock.unlocked) return true;
    return this.askLockPassword(this.lock.passwordSet ? "unlock" : "set");
  }

  /** Re-read the open note, e.g. after locking or unlocking. */
  private async reloadCurrent() {
    const id = this.current?.id;
    if (!id) return;
    const note = await this.guard(() => api.getNote(id));
    if (note && this.current?.id === id) this.current = note;
  }

  async lockNote(id: string) {
    await this.flush();
    if (!(await this.ensureUnlocked())) return;
    const summary = await this.guard(() => api.lockNote(id));
    if (!summary) return;
    this.replaceSummary(summary);
    await this.refreshLock();
    this.showInfo("Note locked. It stays open until you choose Lock Now or close TuxNotes.");
  }

  async removeLock(id: string) {
    await this.flush();
    if (!(await this.ensureUnlocked())) return;
    const summary = await this.guard(() => api.removeNoteLock(id));
    if (!summary) return;
    this.replaceSummary(summary);
    await this.reloadCurrent();
    await this.refreshLock();
  }

  /** Unlock with a password typed on the lock screen. Returns an error message, or null. */
  async unlockNotes(password: string): Promise<string | null> {
    try {
      this.lock = await api.unlockNotes(password);
    } catch (e) {
      return errorText(e);
    }
    await this.reloadCurrent();
    return null;
  }

  /** Hide all locked notes again. */
  async lockNow() {
    if (!this.lock.unlocked) return;
    await this.flush();
    const status = await this.guard(() => api.lockNotes());
    if (status) this.lock = status;
    await this.reloadCurrent();
  }

  /** Called after the password window changed the status (set, unlock, change). */
  async lockStatusChanged(status: LockStatus) {
    this.lock = status;
    await this.reloadCurrent();
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
    if (this.filter.kind === "folder" && this.filter.folderId !== folderId && !this.searchResults) {
      const next = this.removeFromLists(id);
      if (this.selectedId === id) await this.selectNote(next);
    } else {
      // Search results span all folders, so the note stays in them (and selected);
      // only the folder list behind the search loses it.
      if (this.filter.kind === "folder" && this.filter.folderId !== folderId) {
        this.notes = this.notes.filter((n) => n.id !== id);
      }
      this.replaceSummary(updated);
    }
    await this.refreshCounts();
  }

  async trashNote(id: string) {
    await this.flush();
    const trashed = await this.guard(() => api.trashNote(id));
    if (!trashed) return;
    const next = this.removeFromLists(id);
    if (this.selectedId === id) await this.selectNote(next);
    await this.refreshCounts();
    const title = trashed.title || "New Note";
    this.showInfo(`“${title}” moved to Recently Deleted`, { label: "Undo", run: () => void this.undoTrash(id) });
  }

  /** Undo a delete: bring the note back and open it again. */
  async undoTrash(id: string) {
    const restored = await this.guard(() => api.restoreNote(id));
    if (!restored) return;
    if (this.isTrash) this.filter = { kind: "all" };
    await this.loadNotes();
    await this.refreshCounts();
    await this.selectNote(id);
  }

  async duplicateNote(id: string) {
    if (this.notes.find((n) => n.id === id)?.isLocked || (this.current?.id === id && this.current.isLocked)) {
      this.showInfo("Locked notes can't be duplicated. Remove the lock first.");
      return;
    }
    await this.flush();
    const copy = await this.guard(() => api.duplicateNote(id));
    if (!copy) return;
    this.clearSearch();
    await this.loadNotes();
    await this.refreshCounts();
    await this.selectNote(copy.id);
    this.showInfo("Note duplicated");
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
    // Decide by the note, not the view: search results inside Recently Deleted are
    // ordinary notes, which go to Recently Deleted rather than being deleted for good.
    const note = this.visibleNotes.find((n) => n.id === id) ?? (this.current?.id === id ? this.current : null);
    if (note ? note.deletedAt : this.isTrash) await this.deleteNoteForever(id);
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
    // Keep the open note, unless it was one of the notes just deleted.
    if (this.isTrash || this.current?.deletedAt) await this.selectNote(this.notes[0]?.id ?? null);
    await this.refreshCounts();
  }

  // --------------------------------------------------------------- folders

  /** Show the inline "new folder" field, at the top level or inside `parentId`. */
  startNewFolder(parentId: string | null = null) {
    this.newFolderParent = parentId;
    this.creatingFolder = true;
  }

  async createFolder(name: string): Promise<boolean> {
    const parentId = this.newFolderParent;
    const folder = await this.guard(() => api.createFolder(name, parentId));
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

  /** Nest or reorder a folder (drag and drop in the sidebar). */
  async moveFolder(id: string, parentId: string | null, index: number) {
    const moved = await this.guard(() => api.moveFolder(id, parentId, index));
    if (moved) await this.refreshCounts();
  }

  async deleteFolder(id: string) {
    const folder = this.folders.find((f) => f.id === id);
    if (!folder) return;
    const subtree = [id, ...descendantIds(this.folders, id)];
    const subfolders = subtree.length - 1;
    const n = this.folders.filter((f) => subtree.includes(f.id)).reduce((sum, f) => sum + f.noteCount, 0);
    const parts = [
      subfolders ? `Its ${subfolders} subfolder${subfolders === 1 ? "" : "s"} will be deleted too.` : "",
      n === 0
        ? subfolders ? "" : "This folder is empty."
        : `${n} note${n === 1 ? "" : "s"} will be moved to Recently Deleted.`,
    ];
    const ok = await this.confirm({
      title: `Delete “${folder.name}”?`,
      message: parts.filter(Boolean).join(" ") || "This folder is empty.",
      confirmLabel: "Delete Folder",
      destructive: true,
    });
    if (!ok) return;
    await this.flush();
    const moved = await this.guard(() => api.deleteFolder(id));
    if (moved === undefined) return;
    await this.refreshCounts();
    if (this.filter.kind === "folder" && subtree.includes(this.filter.folderId)) {
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
