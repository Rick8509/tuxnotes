// Folder tree helpers. The backend returns a flat list (each level already in order);
// these build the tree the sidebar shows.

import type { Folder } from "./types";

export interface FolderRow {
  folder: Folder;
  depth: number;
  hasChildren: boolean;
}

/** Children of `parentId` (null = top level), in order. */
export function childrenOf(folders: Folder[], parentId: string | null): Folder[] {
  return folders.filter((f) => (f.parentId ?? null) === parentId);
}

/** Visible rows, depth-first, skipping the children of collapsed folders. */
export function visibleRows(folders: Folder[], collapsed: ReadonlySet<string> = new Set()): FolderRow[] {
  const rows: FolderRow[] = [];
  const seen = new Set<string>();
  const walk = (parentId: string | null, depth: number) => {
    for (const folder of childrenOf(folders, parentId)) {
      if (seen.has(folder.id)) continue; // never loop on damaged data
      seen.add(folder.id);
      const hasChildren = folders.some((f) => f.parentId === folder.id);
      rows.push({ folder, depth, hasChildren });
      if (hasChildren && !collapsed.has(folder.id)) walk(folder.id, depth + 1);
    }
  };
  walk(null, 0);
  return rows;
}

/** Every folder inside `id`, at any depth. */
export function descendantIds(folders: Folder[], id: string): string[] {
  const out: string[] = [];
  const queue = [id];
  while (queue.length) {
    const current = queue.shift()!;
    for (const child of childrenOf(folders, current)) {
      if (child.id === id || out.includes(child.id)) continue;
      out.push(child.id);
      queue.push(child.id);
    }
  }
  return out;
}

/** "Work › Projects" style path for a folder. */
export function folderPath(folders: Folder[], id: string): string {
  const parts: string[] = [];
  let current = folders.find((f) => f.id === id);
  for (let guard = 0; current && guard < 64; guard++) {
    parts.unshift(current.name);
    current = current.parentId ? folders.find((f) => f.id === current!.parentId) : undefined;
  }
  return parts.join(" › ");
}

export type DropZone = "before" | "inside" | "after";

/**
 * Where a dragged folder lands: the new parent and its position among the movable
 * siblings there (the default folder doesn't count; it always stays first).
 */
export function dropPlacement(
  folders: Folder[],
  draggedId: string,
  target: Folder,
  zone: DropZone,
): { parentId: string | null; index: number } | null {
  if (target.id === draggedId || descendantIds(folders, draggedId).includes(target.id)) return null;
  if (zone === "inside") {
    const kids = childrenOf(folders, target.id).filter((f) => f.id !== draggedId);
    return { parentId: target.id, index: kids.length };
  }
  const parentId = target.parentId ?? null;
  const siblings = childrenOf(folders, parentId).filter((f) => f.id !== draggedId && !f.isDefault);
  if (target.isDefault) return { parentId, index: 0 };
  const at = siblings.findIndex((f) => f.id === target.id);
  return { parentId, index: zone === "before" ? at : at + 1 };
}
