// Checklist extras, like Apple Notes: ticked items move to the bottom, plus
// Check All / Uncheck All / Move Checked to Bottom / Delete Checked Items.

import { Extension, type Editor } from "@tiptap/core";
import { Fragment, type Node as PMNode } from "@tiptap/pm/model";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { AttrStep, Mapping, ReplaceAroundStep, type Step } from "@tiptap/pm/transform";

const key = new PluginKey("checklistSort");

/** Where an item should go after its checkbox changed (index in the list after removal). */
export function targetIndex(checkedStates: boolean[], index: number): number {
  const others = checkedStates.filter((_, i) => i !== index);
  if (checkedStates[index]) return others.length; // ticked: to the bottom
  // unticked: just after the last unticked item
  let lastUnchecked = -1;
  others.forEach((c, i) => {
    if (!c) lastUnchecked = i;
  });
  return lastUnchecked + 1;
}

/** Stable partition: unticked items first, ticked ones after, each keeping their order. */
export function checkedLastOrder(checkedStates: boolean[]): number[] {
  const idx = checkedStates.map((_, i) => i);
  return [...idx.filter((i) => !checkedStates[i]), ...idx.filter((i) => checkedStates[i])];
}

export const ChecklistSort = Extension.create<{ enabled: () => boolean }>({
  name: "checklistSort",

  addOptions() {
    return { enabled: () => true };
  },

  addProseMirrorPlugins() {
    const enabled = () => this.options.enabled();
    return [
      new Plugin({
        key,
        // When exactly one checkbox changed, move that item. Runs as part of the same
        // change, so a single Undo reverts both the tick and the move.
        appendTransaction(transactions, _oldState, newState) {
          if (!enabled() || !transactions.some((t) => t.docChanged)) return null;
          if (transactions.some((t) => t.getMeta(key))) return null;
          // Undo/redo must restore exactly what was there, not re-sort it.
          if (transactions.some((t) => t.getMeta("history$"))) return null;
          const flips = findCheckboxFlips(transactions);
          if (flips.length !== 1) return null; // bulk changes keep their own order

          const $item = newState.doc.resolve(flips[0]);
          const list = $item.parent;
          if (list.type.name !== "taskList") return null;
          const index = $item.index();
          const states: boolean[] = [];
          list.forEach((child) => states.push(!!child.attrs.checked));
          const target = targetIndex(states, index);
          if (target === index) return null;

          const children: PMNode[] = [];
          list.forEach((child, _offset, i) => {
            if (i !== index) children.push(child);
          });
          children.splice(target, 0, list.child(index));
          const start = $item.start();
          return newState.tr.replaceWith(start, start + list.content.size, Fragment.fromArray(children)).setMeta(key, true);
        },
      }),
    ];
  },
});

/**
 * Positions (in the final document) of task items whose checkbox changed. TaskItem records
 * a tick as `setNodeMarkup` (a structural ReplaceAroundStep); attribute steps count too.
 */
function findCheckboxFlips(transactions: readonly { steps: readonly Step[]; docs: readonly PMNode[]; doc: PMNode; mapping: Mapping }[]): number[] {
  const flips: number[] = [];
  transactions.forEach((tr, t) => {
    tr.steps.forEach((step, i) => {
      let pos: number | null = null;
      if (step instanceof AttrStep && step.attr === "checked") pos = step.pos;
      else if (step instanceof ReplaceAroundStep && step.slice.content.firstChild?.type.name === "taskItem") pos = step.from;
      if (pos === null) return;
      const before = tr.docs[i].nodeAt(pos);
      const after = (i + 1 < tr.docs.length ? tr.docs[i + 1] : tr.doc).nodeAt(pos);
      if (before?.type.name !== "taskItem" || after?.type.name !== "taskItem" || before.attrs.checked === after.attrs.checked) return;
      // Map to the final document: rest of this transaction, then later ones.
      let final = tr.mapping.slice(i + 1).map(pos);
      for (const later of transactions.slice(t + 1)) final = later.mapping.map(final);
      flips.push(final);
    });
  });
  return flips;
}

/** The checklist at a document position (the innermost one), if any. */
export function checklistAt(editor: Editor, pos: number): { node: PMNode; start: number } | null {
  const $pos = editor.state.doc.resolve(pos);
  for (let depth = $pos.depth; depth > 0; depth--) {
    if ($pos.node(depth).type.name === "taskList") return { node: $pos.node(depth), start: $pos.start(depth) };
  }
  return null;
}

function rewriteList(editor: Editor, pos: number, change: (items: PMNode[]) => PMNode[]) {
  const list = checklistAt(editor, pos);
  if (!list) return;
  const items: PMNode[] = [];
  list.node.forEach((child) => items.push(child));
  const next = change(items);
  const { tr, schema } = editor.state;
  const end = list.start + list.node.content.size;
  if (next.length === 0) {
    // Nothing left: replace the whole list with an empty line.
    tr.replaceWith(list.start - 1, end + 1, schema.nodes.paragraph.create());
  } else {
    tr.replaceWith(list.start, end, Fragment.fromArray(next));
  }
  editor.view.dispatch(tr.setMeta(key, true).scrollIntoView());
  editor.commands.focus();
}

export const checklistActions = {
  setAll(editor: Editor, pos: number, checked: boolean) {
    rewriteList(editor, pos, (items) => items.map((i) => i.type.create({ ...i.attrs, checked }, i.content, i.marks)));
  },
  moveCheckedToBottom(editor: Editor, pos: number) {
    rewriteList(editor, pos, (items) => checkedLastOrder(items.map((i) => !!i.attrs.checked)).map((i) => items[i]));
  },
  deleteChecked(editor: Editor, pos: number) {
    rewriteList(editor, pos, (items) => items.filter((i) => !i.attrs.checked));
  },
  /** How many items the list has, and how many are ticked. */
  counts(editor: Editor, pos: number): { total: number; checked: number } {
    const list = checklistAt(editor, pos);
    let checked = 0;
    list?.node.forEach((c) => {
      if (c.attrs.checked) checked++;
    });
    return { total: list?.node.childCount ?? 0, checked };
  },
};
