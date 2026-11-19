// The note content model: every node and mark a note can contain. Shared by the
// editor and the Markdown converter so both always agree.

import StarterKit from "@tiptap/starter-kit";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { TableKit } from "@tiptap/extension-table";
import { FileNode, ImageNode } from "./attachmentNodes";

export function contentExtensions() {
  return [
    StarterKit.configure({
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
