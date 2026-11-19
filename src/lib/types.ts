// Mirrors the serde models in crates/tuxnotes-core/src/models.rs.

/** A TipTap/ProseMirror JSON document. */
export type DocJSON = { type: "doc"; content?: unknown[] } & Record<string, unknown>;

export interface Folder {
  id: string;
  name: string;
  isDefault: boolean;
  /** The folder this one is inside, or null at the top level. */
  parentId: string | null;
  sortOrder: number;
  noteCount: number;
  createdAt: number;
  updatedAt: number;
}

export interface NoteSummary {
  id: string;
  folderId: string;
  title: string;
  snippet: string;
  isPinned: boolean;
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
  /** Password-locked: only the title is visible until locked notes are unlocked. */
  isLocked: boolean;
}

export interface Note {
  id: string;
  folderId: string;
  title: string;
  /** null for a locked note while locked notes are locked. */
  content: DocJSON | null;
  isPinned: boolean;
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
  isLocked: boolean;
}

/** Mirrors `LockStatus` in crates/tuxnotes-core/src/lock.rs. */
export interface LockStatus {
  passwordSet: boolean;
  unlocked: boolean;
  hint: string;
  lockedNotes: number;
}

export type NoteFilter =
  | { kind: "all" }
  | { kind: "folder"; folderId: string }
  | { kind: "trash" };

export function sameFilter(a: NoteFilter, b: NoteFilter): boolean {
  if (a.kind !== b.kind) return false;
  return a.kind !== "folder" || a.folderId === (b as { folderId: string }).folderId;
}

// ------------------------------------------------------------------ backup
// Mirrors src-tauri/src/backup_service.rs and settings.rs.

export type AutoBackup = "off" | "daily" | "weekly";
export type BackupBusy = "signingIn" | "backingUp" | "restoring";
export type ClientSource = "environment" | "configFile" | "builtIn" | "none";

export interface BackupStatus {
  clientSource: ClientSource;
  signedIn: boolean;
  account: string | null;
  busy: BackupBusy | null;
  autoBackup: AutoBackup;
  keepBackups: number;
  lastBackupAt: number | null;
  lastBackupError: string | null;
  /** New backups are encrypted with a password. */
  encrypted: boolean;
  /** Back up (if anything changed) when TuxNotes closes. */
  backupOnClose: boolean;
}

export interface BackupInfo {
  id: string;
  name: string;
  createdAt: number | null;
  size: number | null;
  noteCount: number | null;
  appVersion: string | null;
  encrypted: boolean;
  /** Encrypted with a password this computer doesn't have the key for. */
  needsPassword: boolean;
}

/** What `restoreBackup` fails with when the backup's password is needed. */
export const BACKUP_PASSWORD_NEEDED = "BACKUP_PASSWORD_NEEDED";

export interface RestoreResult {
  noteCount: number;
  folderCount: number;
  backupCreatedAt: number;
  safetyCopy: string;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  database: string;
  configDir: string;
}

// ------------------------------------------------------------- attachments
// Mirrors tuxnotes-core/src/attachments.rs and src-tauri/src/attachments.rs.

export interface Attachment {
  id: string;
  noteId: string | null;
  filename: string;
  mime: string;
  size: number;
  sha256: string;
  createdAt: number;
}

export interface AttachResult {
  attachments: Attachment[];
  errors: string[];
}

/** Image types shown inline (same list as the Rust side). */
export const IMAGE_MIMES = ["image/png", "image/jpeg", "image/gif", "image/webp", "image/avif", "image/bmp"];

/** A Markdown file picked for import. */
export interface MarkdownFile {
  path: string;
  /** File name without extension. */
  name: string;
  text: string;
}
