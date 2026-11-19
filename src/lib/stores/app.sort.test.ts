import { describe, expect, it, vi } from "vitest";

// The store module touches localStorage/window at import; stub the API layer only.
vi.mock("../api", () => ({ api: {}, isTauri: false }));
const { compareNotes } = await import("./app.svelte");

const note = (id: string, title: string, createdAt: number, updatedAt: number, isPinned = false) => ({
  id, folderId: "f", title, snippet: "", isPinned, createdAt, updatedAt, deletedAt: null,
});

describe("compareNotes", () => {
  const a = note("a", "banana", 3, 1);
  const b = note("b", "Apple", 1, 3);
  const c = note("c", "", 2, 2);
  const p = note("p", "zzz", 0, 0, true);
  const order = (sortBy: "edited" | "created" | "title") => [a, b, c, p].sort((x, y) => compareNotes(x, y, sortBy)).map((n) => n.id);

  it("keeps pinned notes first and sorts the rest by the chosen field", () => {
    expect(order("edited")).toEqual(["p", "b", "c", "a"]);
    expect(order("created")).toEqual(["p", "a", "c", "b"]);
    // Title sort is case-insensitive; untitled notes sort as "New Note".
    expect(order("title")).toEqual(["p", "b", "a", "c"]);
  });
});
