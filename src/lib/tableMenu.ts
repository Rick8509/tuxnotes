// Table actions shared by the right-click menu inside a table and the toolbar's Table button.

import type { Editor } from "@tiptap/core";
import type { Node as PMNode } from "@tiptap/pm/model";
import type { MenuItem } from "./components/ContextMenu.svelte";

/** The table node around the cursor, if any. */
function currentTable(editor: Editor): PMNode | null {
  const { $from } = editor.state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    const node = $from.node(depth);
    if (node.type.name === "table") return node;
  }
  return null;
}

export function isInTable(editor: Editor | null): boolean {
  return !!editor && currentTable(editor) !== null;
}

/** Whether the table's first row is a header row. */
function hasHeaderRow(editor: Editor): boolean {
  const firstRow = currentTable(editor)?.firstChild;
  if (!firstRow || firstRow.childCount === 0) return false;
  let allHeaders = true;
  firstRow.forEach((cell) => {
    if (cell.type.name !== "tableHeader") allHeaders = false;
  });
  return allHeaders;
}

export function insertTable(editor: Editor) {
  editor.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run();
}

/** Menu items for the table under the cursor. */
export function tableMenuItems(editor: Editor): MenuItem[] {
  const run = (fn: (chain: ReturnType<Editor["chain"]>) => ReturnType<Editor["chain"]>) => () =>
    void fn(editor.chain().focus()).run();
  const can = editor.can();
  const items: MenuItem[] = [
    { label: "Add Row Above", action: run((c) => c.addRowBefore()) },
    { label: "Add Row Below", action: run((c) => c.addRowAfter()) },
    { label: "Add Column Left", action: run((c) => c.addColumnBefore()) },
    { label: "Add Column Right", action: run((c) => c.addColumnAfter()) },
    { kind: "separator" },
    { label: "Delete Row", action: run((c) => c.deleteRow()), disabled: !can.deleteRow() },
    { label: "Delete Column", action: run((c) => c.deleteColumn()), disabled: !can.deleteColumn() },
  ];
  if (can.mergeCells() || can.splitCell()) {
    items.push(
      { kind: "separator" },
      can.mergeCells()
        ? { label: "Merge Cells", action: run((c) => c.mergeCells()) }
        : { label: "Split Cell", action: run((c) => c.splitCell()) },
    );
  }
  items.push(
    { kind: "separator" },
    {
      label: hasHeaderRow(editor) ? "Remove Header Row" : "Make First Row a Header",
      action: run((c) => c.toggleHeaderRow()),
    },
    { kind: "separator" },
    { label: "Delete Table", action: run((c) => c.deleteTable()), danger: true },
  );
  return items;
}
