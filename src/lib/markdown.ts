// Markdown export and import, using the same content model as the editor.

import { getSchema } from "@tiptap/core";
import { MarkdownManager } from "@tiptap/markdown";
import type { Schema } from "@tiptap/pm/model";
import { contentExtensions } from "./editor/content";
import { extractText } from "./text";
import type { DocJSON } from "./types";

type JSONNode = { type?: string; attrs?: Record<string, unknown>; content?: JSONNode[]; text?: string; marks?: unknown[] };

let manager: MarkdownManager | null = null;
function md(): MarkdownManager {
  manager ??= new MarkdownManager({ extensions: contentExtensions() });
  return manager;
}

/** The note as Markdown. Attachments appear as `attachment:<id>` links (the exporter rewrites them). */
export function noteToMarkdown(doc: DocJSON): string {
  return md().serialize(doc as never).replace(/\n{3,}/g, "\n\n").trim() + "\n";
}

/** An image that should become an attachment, found while importing. */
export interface PendingImage {
  /** Path as written in the Markdown, relative to the .md file. */
  src: string;
  alt: string;
}

/** Nodes whose children are blocks, so an image block can go there. */
const CONTAINERS = new Set(["doc", "blockquote", "listItem", "taskItem", "tableCell", "tableHeader"]);
/** Containers whose first child must be a paragraph. */
const PARAGRAPH_FIRST = new Set(["listItem", "taskItem"]);
/** Blocks that hold text (and, in Markdown, inline images). */
const TEXTBLOCKS = new Set(["paragraph", "heading"]);

/**
 * Markdown → note document. Images are lifted out of paragraphs and headings into their
 * own blocks, at any depth (lists, quotes, tables). Local images are returned in `images`
 * (their nodes keep `src` until attached); images from the web become plain links
 * (nothing is downloaded); data: images are dropped.
 */
export function markdownToDoc(markdown: string): { doc: DocJSON; images: PendingImage[] } {
  const parsed = md().parse(markdown) as JSONNode;
  const images: PendingImage[] = [];

  /** A web image becomes an inline link; a local one a pending image block; anything else is dropped. */
  const classify = (node: JSONNode): { inline: JSONNode } | { block: JSONNode } | null => {
    const src = String(node.attrs?.src ?? "").trim();
    const alt = String(node.attrs?.alt ?? "");
    if (/^https?:\/\//i.test(src)) {
      return { inline: { type: "text", text: alt || src, marks: [{ type: "link", attrs: { href: src } }] } };
    }
    if (!src || /^[a-z][a-z0-9+.-]*:/i.test(src)) return null; // data:, file:, etc.
    images.push({ src, alt });
    return { block: { type: "image", attrs: { src, alt: alt || null } } };
  };

  /** Split a paragraph/heading around its local images (text, image, text). */
  const split = (block: JSONNode): JSONNode[] => {
    const out: JSONNode[] = [];
    let run: JSONNode[] = [];
    const flush = () => {
      if (run.some((c) => c.type !== "text" || (c.text ?? "").trim())) out.push({ ...block, content: run });
      run = [];
    };
    for (const child of block.content ?? []) {
      if (child.type !== "image") {
        run.push(child);
        continue;
      }
      const result = classify(child);
      if (result && "inline" in result) run.push(result.inline);
      else if (result) {
        flush();
        out.push(result.block);
      }
    }
    flush();
    return out;
  };

  const visit = (node: JSONNode): JSONNode => {
    if (!node.content) return node;
    if (!CONTAINERS.has(node.type ?? "")) return { ...node, content: node.content.map(visit) };
    const out: JSONNode[] = [];
    for (const child of node.content) {
      if (child.type === "text") {
        out.push({ type: "paragraph", content: [child] }); // loose text isn't a valid block
      } else if (child.type === "image") {
        const result = classify(child);
        if (result && "block" in result) out.push(result.block);
        else if (result) out.push({ type: "paragraph", content: [result.inline] });
      } else if (TEXTBLOCKS.has(child.type ?? "") && child.content?.some((c) => c.type === "image")) {
        out.push(...split(child));
      } else {
        out.push(visit(child));
      }
    }
    if (PARAGRAPH_FIRST.has(node.type ?? "") && out[0]?.type !== "paragraph") out.unshift({ type: "paragraph" });
    return { ...node, content: out.length ? out : [{ type: "paragraph" }] };
  };

  return { doc: visit({ ...parsed, type: "doc" }) as DocJSON, images };
}

/** Replace an imported image's `src` with its attachment (or a text fallback if it couldn't be attached). */
export function resolveImage(doc: DocJSON, src: string, attachmentId: string | null, alt: string): DocJSON {
  const visit = (node: JSONNode): JSONNode => {
    if (node.type === "image" && node.attrs?.src === src && !node.attrs?.attachmentId) {
      return attachmentId
        ? { type: "image", attrs: { attachmentId, alt: node.attrs?.alt ?? (alt || null) } }
        : { type: "paragraph", content: [{ type: "text", text: `[image: ${alt || src}]` }] };
    }
    return node.content ? { ...node, content: node.content.map(visit) } : node;
  };
  return visit(doc as JSONNode) as DocJSON;
}

let schema: Schema | null = null;

/**
 * The document if the editor can show it; otherwise its text as plain paragraphs, so an
 * import never ends up as a read-only "can't show this" note.
 */
export function fitToEditor(doc: DocJSON): { doc: DocJSON; fitted: boolean } {
  schema ??= getSchema(contentExtensions());
  try {
    schema.nodeFromJSON(doc).check();
    return { doc, fitted: false };
  } catch {
    const lines = extractText(doc).plain.split("\n");
    const content = lines.map((l) => (l ? { type: "paragraph", content: [{ type: "text", text: l }] } : { type: "paragraph" }));
    return { doc: { type: "doc", content } as DocJSON, fitted: true };
  }
}
