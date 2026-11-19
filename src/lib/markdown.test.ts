import { describe, expect, it } from "vitest";
import { markdownToDoc, noteToMarkdown, resolveImage } from "./markdown";
import type { DocJSON } from "./types";

const t = (text: string, marks?: unknown[]) => ({ type: "text", text, ...(marks ? { marks } : {}) });
const p = (...content: unknown[]) => ({ type: "paragraph", content });

describe("noteToMarkdown", () => {
  it("exports text, checklists, tables and attachments", () => {
    const doc = {
      type: "doc",
      content: [
        p(t("Trip")),
        p(t("bring "), t("passport", [{ type: "bold" }])),
        { type: "taskList", content: [{ type: "taskItem", attrs: { checked: true }, content: [p(t("tickets"))] }] },
        { type: "table", content: [
          { type: "tableRow", content: [{ type: "tableHeader", content: [p(t("Day"))] }, { type: "tableHeader", content: [p(t("City"))] }] },
          { type: "tableRow", content: [{ type: "tableCell", content: [p(t("1"))] }, { type: "tableCell", content: [p(t("Rome"))] }] },
        ] },
        { type: "image", attrs: { attachmentId: "img-1", alt: "map [old].png" } },
        { type: "attachment", attrs: { attachmentId: "file-1", filename: "plan.pdf", size: 3, mime: "application/pdf" } },
      ],
    } as DocJSON;
    const out = noteToMarkdown(doc);
    expect(out).toContain("Trip\n\nbring **passport**");
    expect(out).toContain("- [x] tickets");
    expect(out).toMatch(/\| Day\s*\| City\s*\|/);
    expect(out).toContain("![map \\[old\\].png](attachment:img-1)");
    expect(out).toContain("[📎 plan.pdf](attachment:file-1)");
    expect(out).not.toMatch(/\n{3,}/);
  });
});

describe("markdownToDoc", () => {
  it("imports headings, marks, checklists and tables", () => {
    const { doc, images } = markdownToDoc("# Plan\n\nSome **bold** text\n\n- [x] done\n- [ ] todo\n\n| A | B |\n|---|---|\n| 1 | 2 |\n");
    expect((doc.content as { type: string }[]).map((n) => n.type)).toEqual(["heading", "paragraph", "taskList", "table"]);
    expect(images).toEqual([]);
  });

  it("lifts images out of paragraphs and sorts them by source", () => {
    const { doc, images } = markdownToDoc("Before ![a](pics/a.png) after\n\n![web](https://example.com/x.png)\n\n![inline](data:image/png;base64,AAAA)\n");
    const types = (doc.content as { type: string }[]).map((n) => n.type);
    expect(types).toEqual(["paragraph", "image", "paragraph", "paragraph"]);
    expect(images).toEqual([{ src: "pics/a.png", alt: "a" }]);
    // Web images become links, not downloads.
    expect(JSON.stringify(doc)).toContain('"href":"https://example.com/x.png"');
    expect(JSON.stringify(doc)).not.toContain("data:image");
  });

  it("keeps web images inline as links inside their sentence", () => {
    const { doc } = markdownToDoc("See ![logo](https://example.com/logo.png) online.\n");
    expect(doc.content).toEqual([
      { type: "paragraph", content: [
        { type: "text", text: "See " },
        { type: "text", text: "logo", marks: [{ type: "link", attrs: { href: "https://example.com/logo.png" } }] },
        { type: "text", text: " online." },
      ] },
    ]);
  });

  it("resolves pending images to attachments or a text fallback", () => {
    const { doc } = markdownToDoc("![a](a.png)\n\n![b](b.png)\n");
    let out = resolveImage(doc, "a.png", "att-1", "a");
    out = resolveImage(out, "b.png", null, "b");
    expect(out.content).toEqual([
      { type: "image", attrs: { attachmentId: "att-1", alt: "a" } },
      { type: "paragraph", content: [{ type: "text", text: "[image: b]" }] },
    ]);
  });

  it("round-trips its own export", () => {
    const original = "# Groceries\n\n- [ ] milk\n- [x] eggs\n\nVisit [shop](https://shop.example)\n";
    const { doc } = markdownToDoc(original);
    expect(noteToMarkdown(doc)).toBe(original);
  });
});
