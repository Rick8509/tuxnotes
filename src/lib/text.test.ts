// Keep in sync with the Rust tests in crates/tuxnotes-core/src/text.rs.
import { describe, expect, it } from "vitest";
import { MAX_TITLE_CHARS, extractText, isBlank } from "./text";

const para = (text: string) => ({ type: "paragraph", content: [{ type: "text", text }] });

describe("extractText", () => {
  it("handles an empty document", () => {
    expect(extractText({ type: "doc", content: [{ type: "paragraph" }] })).toEqual({
      title: "",
      snippet: "",
      plain: "",
    });
  });

  it("uses the first non-empty line as title and the rest as snippet", () => {
    const out = extractText({
      type: "doc",
      content: [
        { type: "paragraph" },
        { type: "heading", content: [{ type: "text", text: "  Groceries " }] },
        para("milk"),
        { type: "paragraph" },
        para("eggs"),
      ],
    });
    expect(out).toEqual({ title: "Groceries", snippet: "milk eggs", plain: "  Groceries\nmilk\neggs" });
  });

  it("splits task items and hard breaks into lines", () => {
    const out = extractText({
      type: "doc",
      content: [
        para("Todo"),
        {
          type: "taskList",
          content: [
            { type: "taskItem", content: [para("first")] },
            { type: "taskItem", content: [para("second")] },
          ],
        },
        { type: "paragraph", content: [{ type: "text", text: "a" }, { type: "hardBreak" }, { type: "text", text: "b" }] },
      ],
    });
    expect(out.plain).toBe("Todo\nfirst\nsecond\na\nb");
  });

  it("truncates long titles by characters", () => {
    const out = extractText({ type: "doc", content: [para("é".repeat(500))] });
    expect([...out.title].length).toBe(MAX_TITLE_CHARS);
    expect(out.title.endsWith("…")).toBe(true);
  });

  it("ignores malformed nodes", () => {
    expect(extractText({ type: "doc", content: [42, "x", null, { foo: 1 }, para("ok")] }).title).toBe("ok");
  });
});

describe("tables", () => {
  const cell = (type: string, text: string) => ({
    type,
    attrs: { colspan: 1, rowspan: 1, colwidth: null },
    content: [text ? para(text) : { type: "paragraph" }],
  });
  it("reads each row as one line (same as Rust)", () => {
    const out = extractText({
      type: "doc",
      content: [
        para("Plan"),
        {
          type: "table",
          content: [
            { type: "tableRow", content: [cell("tableHeader", "Milestone"), cell("tableHeader", "Status")] },
            { type: "tableRow", content: [cell("tableCell", "M1"), cell("tableCell", "Done")] },
            { type: "tableRow", content: [cell("tableCell", ""), cell("tableCell", "Later")] },
          ],
        },
        para("After the table"),
      ],
    });
    expect(out.plain).toBe("Plan\nMilestone Status\nM1 Done\nLater\nAfter the table");
    expect(out.snippet).toBe("Milestone Status M1 Done Later After the table");
  });
  it("an empty table is content", () => {
    expect(isBlank({ type: "doc", content: [{ type: "table", content: [{ type: "tableRow", content: [cell("tableCell", "")] }] }] })).toBe(false);
  });
});

describe("isBlank", () => {
  it("matches the Rust rules", () => {
    expect(isBlank({ type: "doc", content: [{ type: "paragraph" }, { type: "heading" }] })).toBe(true);
    expect(isBlank({ type: "doc", content: [para(" ")] })).toBe(false);
    expect(isBlank({ type: "doc", content: [{ type: "taskList", content: [{ type: "taskItem", content: [{ type: "paragraph" }] }] }] })).toBe(false);
    expect(isBlank({ type: "doc", content: [{ type: "horizontalRule" }] })).toBe(false);
  });
});
