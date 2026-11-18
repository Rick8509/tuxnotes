// Mirrors the serde models in crates/tuxnotes-core/src/models.rs.

/** A TipTap/ProseMirror JSON document. */
export type DocJSON = { type: "doc"; content?: unknown[] } & Record<string, unknown>;

export interface Folder {
  id: string;
  name: string;
  isDefault: boolean;
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
}

export interface Note {
  id: string;
  folderId: string;
  title: string;
  content: DocJSON;
  isPinned: boolean;
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
}

export type NoteFilter =
  | { kind: "all" }
  | { kind: "folder"; folderId: string }
  | { kind: "trash" };

export function sameFilter(a: NoteFilter, b: NoteFilter): boolean {
  if (a.kind !== b.kind) return false;
  return a.kind !== "folder" || a.folderId === (b as { folderId: string }).folderId;
}
