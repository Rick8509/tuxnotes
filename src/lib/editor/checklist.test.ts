import { describe, expect, it } from "vitest";
import { checkedLastOrder, targetIndex } from "./checklist";

describe("targetIndex (where a ticked/unticked item moves)", () => {
  it("ticked items go to the bottom", () => {
    // [milk ✓(just ticked), eggs, bread] → milk after bread
    expect(targetIndex([true, false, false], 0)).toBe(2);
    // already last
    expect(targetIndex([false, false, true], 2)).toBe(2);
  });
  it("unticked items go just after the last unticked item", () => {
    // [a, b, c(✓ → just unticked), d ✓, e ✓] stays right after b
    expect(targetIndex([false, false, false, true, true], 2)).toBe(2);
    // [a, b ✓, c ✓, d (just unticked)] → after a
    expect(targetIndex([false, true, true, false], 3)).toBe(1);
    // everything else ticked → to the top
    expect(targetIndex([true, true, false], 2)).toBe(0);
  });
});

describe("checkedLastOrder", () => {
  it("keeps order within unticked and ticked groups", () => {
    expect(checkedLastOrder([true, false, true, false])).toEqual([1, 3, 0, 2]);
    expect(checkedLastOrder([])).toEqual([]);
  });
});
