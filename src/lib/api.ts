// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
//
// When the UI runs in a normal browser (`npm run dev` without Tauri), calls go to
// an in-memory mock so the interface can be developed without building Rust.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  AttachResult,
  Attachment,
  AutoBackup,
  BackupInfo,
  BackupStatus,
  DocJSON,
  Folder,
  MarkdownFile,
  Note,
  NoteFilter,
  NoteSummary,
  RestoreResult,
} from "./types";

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
  discardNoteIfBlank: (id: string) => call<boolean>("discard_note_if_blank", { id }),

  backupStatus: () => call<BackupStatus>("backup_status"),
  setBackupPreferences: (autoBackup: AutoBackup, keepBackups: number) =>
    call<BackupStatus>("set_backup_preferences", { autoBackup, keepBackups }),
  googleSignIn: () => call<BackupStatus>("google_sign_in"),
  googleCancelSignIn: () => call<void>("google_cancel_sign_in"),
  googleSignOut: () => call<BackupStatus>("google_sign_out"),
  backupNow: () => call<BackupInfo>("backup_now"),
  listBackups: () => call<BackupInfo[]>("list_backups"),
  restoreBackup: (id: string) => call<RestoreResult>("restore_backup", { id }),

  appInfo: () => call<AppInfo>("app_info"),

  /** Add a pasted or dropped file to a note. The bytes go over IPC as raw binary. */
  async addAttachment(noteId: string, file: File): Promise<Attachment> {
    if (!isTauri) return (await import("./mock")).mockAddAttachment(noteId, file);
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      return await tauriInvoke<Attachment>("add_attachment", bytes, {
        headers: {
          "x-note-id": encodeURIComponent(noteId),
          "x-filename": encodeURIComponent(file.name || "pasted-image.png"),
          "x-mime": encodeURIComponent(file.type || ""),
        },
      });
    } catch (err) {
      throw new Error(typeof err === "string" ? err : String((err as Error)?.message ?? err));
    }
  },
  /** Show the file picker and attach the chosen files. */
  attachFiles: (noteId: string) =>
    isTauri ? call<AttachResult>("attach_files", { noteId }) : import("./mock").then((m) => m.mockAttachFiles(noteId)),
  openAttachment: (id: string) =>
    isTauri ? call<void>("open_attachment", { id }) : import("./mock").then((m) => m.mockOpenAttachment(id)),
  /** Save Markdown (attachments are copied next to it). Returns the path, or null if cancelled. */
  exportMarkdown: (title: string, markdown: string) =>
    isTauri
      ? call<string | null>("export_markdown", { title, markdown })
      : import("./mock").then((m) => m.mockExportMarkdown(title, markdown)),
  exportPdf: (title: string) => (isTauri ? call<string | null>("export_pdf", { title }) : import("./mock").then((m) => m.mockPrint())),
  printNote: () => (isTauri ? call<void>("print_note") : import("./mock").then((m) => m.mockPrint())),
  pickMarkdownFiles: () =>
    isTauri ? call<MarkdownFile[]>("pick_markdown_files") : import("./mock").then((m) => m.mockPickMarkdownFiles()),
  importImage: (noteId: string, markdownPath: string, src: string) =>
    isTauri
      ? call<Attachment>("import_image", { noteId, markdownPath, src })
      : Promise.reject(new Error("Images can't be imported in the browser preview.")),
  saveAttachmentAs: (id: string) =>
    isTauri ? call<boolean>("save_attachment_as", { id }) : import("./mock").then((m) => m.mockSaveAttachment(id)),
};
