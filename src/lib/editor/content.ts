// The note content model: every node and mark a note can contain. Shared by the
// editor and the Markdown converter so both always agree.

import { Node } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { TableKit } from "@tiptap/extension-table";
import { FileNode, ImageNode } from "./attachmentNodes";

/**
 * The note document. `firstLine` records how a note's first line looks, chosen by
 * "New notes start with" when the note was created: "heading" or "body". No value means
 * a title (the default, and every note from before this setting was per note).
 */
const NoteDocument = Node.create({
  name: "doc",
  topNode: true,
  content: "block+",
  addAttributes() {
    return { firstLine: { default: null, rendered: false } };
  },
  renderMarkdown: (node, h) => (node.content ? h.renderChildren(node.content, "\n\n") : ""),
});

export function contentExtensions() {
  return [
    NoteDocument,
    StarterKit.configure({
      document: false,
      heading: { levels: [1, 2, 3] },
      link: { openOnClick: false, autolink: true, defaultProtocol: "https" },
    }),
    TaskList,
    TaskItem.configure({ nested: true }),
    // Tab / Shift+Tab move between cells; Tab in the last cell adds a row.
    TableKit.configure({ table: { resizable: true, cellMinWidth: 60 } }),
    ImageNode,
    FileNode,
  ];
}
