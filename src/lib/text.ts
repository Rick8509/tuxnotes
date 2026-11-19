// TypeScript port of crates/tuxnotes-core/src/text.rs, used by the browser mock
// and for instant title/preview updates in the note list while typing.

const BLOCK_NODES = new Set([
  "paragraph",
  "heading",
  "listItem",
  "taskItem",
  "codeBlock",
  "blockquote",
  "horizontalRule",
  "tableRow",
  "image",
]);
const INLINE_SEPARATED_NODES = new Set(["tableCell", "tableHeader"]);

export const MAX_TITLE_CHARS = 200;
export const MAX_SNIPPET_CHARS = 160;

export interface ExtractedText {
  title: string;
  snippet: string;
  plain: string;
}

/** `inCell`: inside a table cell, blocks are separated by spaces so each table row reads as one line. */
function walk(node: unknown, out: string[], inCell = false): void {
  if (!node || typeof node !== "object") return;
  const n = node as { type?: unknown; text?: unknown; content?: unknown };
  if (typeof n.text === "string") out.push(n.text);
  const type = typeof n.type === "string" ? n.type : "";
  if (type === "hardBreak") out.push(inCell ? " " : "\n");
  const isCell = INLINE_SEPARATED_NODES.has(type);
  if (Array.isArray(n.content)) for (const child of n.content) walk(child, out, inCell || isCell);
  const last = out.length ? out[out.length - 1] : "";
  const endsLine = BLOCK_NODES.has(type) && !(inCell && type !== "tableRow");
  if (endsLine) {
    if (out.length && !last.endsWith("\n")) out.push("\n");
  } else if ((isCell || BLOCK_NODES.has(type)) && out.length && !/[ \n]$/.test(last)) {
    out.push(" ");
  }
}

export function truncateChars(s: string, max: number): string {
  const chars = [...s];
  if (chars.length <= max) return s;
  return chars.slice(0, Math.max(0, max - 1)).join("").trimEnd() + "…";
}

export function extractText(doc: unknown): ExtractedText {
  const parts: string[] = [];
  walk(doc, parts);
  const raw = parts.join("");
  const lines = raw.split("\n");
  const nonEmpty = lines.map((l) => l.trim()).filter(Boolean);
  return {
    title: truncateChars(nonEmpty[0] ?? "", MAX_TITLE_CHARS),
    snippet: truncateChars(nonEmpty.slice(1).join(" "), MAX_SNIPPET_CHARS),
    plain: lines
      .map((l) => l.trimEnd())
      .filter((l) => l.trim())
      .join("\n"),
  };
}

/** Port of `text::is_blank` in Rust: only empty paragraphs/headings, nothing else. */
export function isBlank(node: unknown): boolean {
  if (!node || typeof node !== "object") return true;
  const n = node as { type?: unknown; text?: unknown; content?: unknown };
  if (typeof n.type === "string" && !["doc", "paragraph", "heading", "hardBreak"].includes(n.type)) return false;
  if (n.text !== undefined) return false;
  return !Array.isArray(n.content) || n.content.every(isBlank);
}
