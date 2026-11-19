// Markdown export and import, using the same content model as the editor.

import { MarkdownManager } from "@tiptap/markdown";
import { contentExtensions } from "./editor/content";
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

/**
 * Markdown → note document. Images are lifted out of paragraphs into their own blocks.
 * Local images are returned in `images` (their nodes keep `src` until attached); images
 * from the web become plain links (nothing is downloaded); data: images are dropped.
 */
export function markdownToDoc(markdown: string): { doc: DocJSON; images: PendingImage[] } {
  const parsed = md().parse(markdown) as JSONNode;
  const images: PendingImage[] = [];
  const blocks: JSONNode[] = [];

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

  for (const block of parsed.content ?? []) {
    const children = block.type === "image" ? [block] : block.type === "paragraph" ? block.content : undefined;
    if (!children?.some((c) => c.type === "image")) {
      // Plain text at the top level isn't valid; wrap it.
      blocks.push(block.type === "text" ? { type: "paragraph", content: [block] } : block);
      continue;
    }
    // Split the paragraph around local images (text, image, text); web images stay inline as links.
    let run: JSONNode[] = [];
    const flush = () => {
      if (run.some((c) => c.type !== "text" || (c.text ?? "").trim())) blocks.push({ type: "paragraph", content: run });
      run = [];
    };
    for (const child of children) {
      if (child.type !== "image") {
        run.push(child);
        continue;
      }
      const result = classify(child);
      if (result && "inline" in result) run.push(result.inline);
      else if (result) {
        flush();
        blocks.push(result.block);
      }
    }
    flush();
  }

  return { doc: { type: "doc", content: blocks.length ? blocks : [{ type: "paragraph" }] } as DocJSON, images };
}

/** Replace an imported image's `src` with its attachment (or a text fallback if it couldn't be attached). */
export function resolveImage(doc: DocJSON, src: string, attachmentId: string | null, alt: string): DocJSON {
  const content = (doc.content as JSONNode[]).map((node) => {
    if (node.type !== "image" || node.attrs?.src !== src || node.attrs?.attachmentId) return node;
    return attachmentId
      ? { type: "image", attrs: { attachmentId, alt: node.attrs?.alt ?? (alt || null) } }
      : { type: "paragraph", content: [{ type: "text", text: `[image: ${alt || src}]` }] };
  });
  return { ...doc, content } as DocJSON;
}
