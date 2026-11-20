import { describe, expect, it } from "vitest";
import { descendantIds, dropPlacement, folderPath, totalNoteCount, visibleRows } from "./folders";
import type { Folder } from "./types";

const f = (id: string, parentId: string | null = null, isDefault = false): Folder => ({
  id, name: id, parentId, isDefault, sortOrder: 0, noteCount: 0, createdAt: 0, updatedAt: 0,
});
// Notes, Work (Projects (Old), Admin), Home
const folders = [f("Notes", null, true), f("Work"), f("Projects", "Work"), f("Old", "Projects"), f("Admin", "Work"), f("Home")];
const byId = (id: string) => folders.find((x) => x.id === id)!;

describe("folder tree", () => {
  it("lists rows depth-first with depth and children flags", () => {
    expect(visibleRows(folders).map((r) => `${"·".repeat(r.depth)}${r.folder.id}${r.hasChildren ? "+" : ""}`)).toEqual([
      "Notes", "Work+", "·Projects+", "··Old", "·Admin", "Home",
    ]);
  });
  it("hides the children of collapsed folders", () => {
    expect(visibleRows(folders, new Set(["Work"])).map((r) => r.folder.id)).toEqual(["Notes", "Work", "Home"]);
  });
  it("counts the notes in a folder and all its subfolders", () => {
    const counted = folders.map((x, i) => ({ ...x, noteCount: i + 1 })); // Notes 1, Work 2, Projects 3, Old 4, Admin 5, Home 6
    expect(totalNoteCount(counted, "Work")).toBe(2 + 3 + 4 + 5);
    expect(totalNoteCount(counted, "Projects")).toBe(3 + 4);
    expect(totalNoteCount(counted, "Home")).toBe(6);
  });

  it("finds descendants and paths", () => {
    expect(descendantIds(folders, "Work")).toEqual(["Projects", "Admin", "Old"]);
    expect(folderPath(folders, "Old")).toBe("Work › Projects › Old");
  });
});

describe("dropPlacement", () => {
  it("drops inside a folder at the end", () => {
    expect(dropPlacement(folders, "Home", byId("Work"), "inside")).toEqual({ parentId: "Work", index: 2 });
  });
  it("drops before/after a sibling, counting only movable folders", () => {
    expect(dropPlacement(folders, "Home", byId("Work"), "before")).toEqual({ parentId: null, index: 0 });
    expect(dropPlacement(folders, "Work", byId("Home"), "after")).toEqual({ parentId: null, index: 1 });
    expect(dropPlacement(folders, "Home", byId("Admin"), "before")).toEqual({ parentId: "Work", index: 1 });
  });
  it("anything dropped next to the default folder goes first", () => {
    expect(dropPlacement(folders, "Home", byId("Notes"), "after")).toEqual({ parentId: null, index: 0 });
  });
  it("refuses dropping a folder into itself or its own subfolders", () => {
    expect(dropPlacement(folders, "Work", byId("Work"), "inside")).toBeNull();
    expect(dropPlacement(folders, "Work", byId("Old"), "inside")).toBeNull();
    expect(dropPlacement(folders, "Work", byId("Projects"), "after")).toBeNull();
  });
});
