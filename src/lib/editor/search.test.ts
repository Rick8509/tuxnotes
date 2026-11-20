import { describe, expect, it } from "vitest";
import { findInText, foldWithMap } from "./search";

const slices = (text: string, query: string, wordStart = false) => findInText(text, query, wordStart).map(([s, e]) => text.slice(s, e));

describe("findInText", () => {
  it("finds Chinese, Japanese and Thai words inside sentences, even when matching word starts", () => {
    expect(slices("今日は牛乳を買う", "牛乳", true)).toEqual(["牛乳"]);
    expect(slices("ฉันชอบกินข้าว", "กิน", true)).toEqual(["กิน"]);
    expect(slices("milk and 牛乳", "ilk", true)).toEqual([]); // Latin words still need a word start
  });

  it("ignores case and accents", () => {
    expect(slices("Café, CAFE and cafe", "cafe")).toEqual(["Café", "CAFE", "cafe"]);
    expect(slices("naïve résumé", "RESUME")).toEqual(["résumé"]);
  });
  it("maps decomposed characters and emoji back to exact positions", () => {
    const decomposed = "café au lait"; // e + combining acute
    expect(slices(decomposed, "café")).toEqual(["café"]);
    expect(slices("🍕 pizza 🍕 pizza", "pizza")).toEqual(["pizza", "pizza"]);
  });
  it("can require matches to start a word (search terms)", () => {
    expect(slices("grocery groceries ungrocery", "groc", true)).toEqual(["groc", "groc"]);
    expect(slices("grocery groceries ungrocery", "groc", false)).toEqual(["groc", "groc", "groc"]);
  });
  it("doesn't return overlapping matches", () => {
    expect(findInText("aaaa", "aa")).toEqual([[0, 2], [2, 4]]);
  });
  it("handles empty queries", () => {
    expect(findInText("text", "")).toEqual([]);
    expect(findInText("text", "   ".trim())).toEqual([]);
  });
});

describe("foldWithMap", () => {
  it("maps every folded character to its original index", () => {
    const { folded, map } = foldWithMap("Ćx");
    expect(folded).toBe("cx");
    expect(map).toEqual([0, 1]);
  });
});
