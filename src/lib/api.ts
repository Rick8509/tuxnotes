// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
//
// When the UI runs in a normal browser (`npm run dev` without Tauri), calls go to
// an in-memory mock so the interface can be developed without building Rust.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { DocJSON, Folder, Note, NoteFilter, NoteSummary } from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

let invokeImpl: Invoke | null = isTauri ? tauriInvoke : null;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!invokeImpl) {
    const { mockInvoke } = await import("./mock");
    invokeImpl = mockInvoke;
  }
  try {
    return await invokeImpl<T>(cmd, args);
  } catch (err) {
    // Rust errors arrive as plain strings.
    throw new Error(typeof err === "string" ? err : String((err as Error)?.message ?? err));
  }
}

export const api = {
  listFolders: () => call<Folder[]>("list_folders"),
  createFolder: (name: string) => call<Folder>("create_folder", { name }),
  renameFolder: (id: string, name: string) => call<Folder>("rename_folder", { id, name }),
  deleteFolder: (id: string) => call<number>("delete_folder", { id }),

  listNotes: (filter: NoteFilter) => call<NoteSummary[]>("list_notes", { filter }),
  getNote: (id: string) => call<Note>("get_note", { id }),
  createNote: (folderId: string | null) => call<Note>("create_note", { folderId }),
  updateNoteContent: (id: string, content: DocJSON) =>
    call<NoteSummary>("update_note_content", { id, content }),
  setNotePinned: (id: string, pinned: boolean) =>
    call<NoteSummary>("set_note_pinned", { id, pinned }),
  moveNote: (id: string, folderId: string) => call<NoteSummary>("move_note", { id, folderId }),
  trashNote: (id: string) => call<NoteSummary>("trash_note", { id }),
  restoreNote: (id: string) => call<NoteSummary>("restore_note", { id }),
  deleteNotePermanently: (id: string) => call<void>("delete_note_permanently", { id }),
  emptyTrash: () => call<number>("empty_trash"),
  trashCount: () => call<number>("trash_count"),
  searchNotes: (query: string) => call<NoteSummary[]>("search_notes", { query }),
};
