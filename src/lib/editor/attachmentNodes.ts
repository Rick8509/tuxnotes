// Editor nodes for attachments. Notes store only the attachment id; the file itself is
// loaded through attachmentUrl() (the `attachment://` protocol in the app).

import { Node, mergeAttributes } from "@tiptap/core";
import { attachmentUrl } from "../attachmentUrls";
import { formatBytes } from "../format";
import type { Attachment } from "../types";
import { IMAGE_MIMES } from "../types";

/** An image shown inline, full width at most. */
export const ImageNode = Node.create({
  name: "image",
  group: "block",
  atom: true,
  draggable: true,
  selectable: true,

  addAttributes() {
    return {
      attachmentId: {
        default: null,
        parseHTML: (el) => el.getAttribute("data-attachment-id"),
        renderHTML: (attrs) => ({ "data-attachment-id": attrs.attachmentId, src: attachmentUrl(attrs.attachmentId) }),
      },
      alt: {
        default: null,
        parseHTML: (el) => el.getAttribute("alt"),
        renderHTML: (attrs) => (attrs.alt ? { alt: attrs.alt } : {}),
      },
    };
  },

  // Only our own images (e.g. copied between notes). Images pasted from web pages point
  // at remote addresses and are left out, for privacy and so notes work offline.
  parseHTML() {
    return [{ tag: "img[data-attachment-id]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["img", mergeAttributes({ class: "note-image", draggable: "true" }, HTMLAttributes)];
  },
});

/** Short label for the file icon, e.g. "PDF". */
export function fileBadge(filename: string): string {
  const dot = filename.lastIndexOf(".");
  const ext = dot > 0 ? filename.slice(dot + 1) : "";
  return ext && ext.length <= 4 ? ext.toUpperCase() : "FILE";
}

/** Any other file: a compact chip with type, name and size. Double-click opens it. */
export const FileNode = Node.create({
  name: "attachment",
  group: "block",
  atom: true,
  draggable: true,
  selectable: true,

  addAttributes() {
    const data = (name: string, parse: (v: string | null) => unknown = (v) => v) => ({
      parseHTML: (el: HTMLElement) => parse(el.getAttribute(`data-${name}`)),
      renderHTML: (attrs: Record<string, unknown>) => ({ [`data-${name}`]: String(attrs[camel(name)] ?? "") }),
    });
    return {
      attachmentId: { default: null, ...data("attachment-id") },
      filename: { default: "file", ...data("filename") },
      size: { default: 0, ...data("size", (v) => Number(v) || 0) },
      mime: { default: "", ...data("mime") },
    };
  },

  parseHTML() {
    return [{ tag: "div[data-attachment-id][data-filename]" }];
  },

  renderHTML({ node, HTMLAttributes }) {
    const { filename, size } = node.attrs as { filename: string; size: number };
    return [
      "div",
      mergeAttributes({ class: "note-file", title: `${filename} — double-click to open` }, HTMLAttributes),
      ["span", { class: "note-file-icon" }, fileBadge(filename)],
      ["span", { class: "note-file-text" }, ["span", { class: "note-file-name" }, filename], ["span", { class: "note-file-size" }, formatBytes(size)]],
    ];
  },
});

function camel(name: string): string {
  return name.replace(/-(\w)/g, (_, c: string) => c.toUpperCase());
}

/** The editor node to insert for a newly added attachment. */
export function nodeForAttachment(a: Attachment) {
  return IMAGE_MIMES.includes(a.mime)
    ? { type: "image", attrs: { attachmentId: a.id, alt: a.filename } }
    : { type: "attachment", attrs: { attachmentId: a.id, filename: a.filename, size: a.size, mime: a.mime } };
}
