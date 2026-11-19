// Export the open note (Markdown, PDF, print) and import Markdown files as notes.

import { api } from "./api";
import { markdownToDoc, noteToMarkdown, resolveImage } from "./markdown";
import { app } from "./stores/app.svelte";
import { extractText } from "./text";
import type { DocJSON } from "./types";

/** The open note as saved (pending edits are saved first). */
async function savedCurrent() {
  await app.flush();
  const id = app.current?.id;
  return id ? api.getNote(id) : null;
}

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

export async function exportMarkdown() {
  try {
    const note = await savedCurrent();
    if (!note) return;
    const path = await api.exportMarkdown(note.title || "Note", noteToMarkdown(note.content));
    if (path) app.showInfo(`Exported “${fileName(path)}”`);
  } catch (e) {
    app.showError(e);
  }
}

export async function exportPdf() {
  try {
    const note = await savedCurrent();
    if (!note) return;
    const path = await api.exportPdf(note.title || "Note");
    if (path) app.showInfo(`Saved “${fileName(path)}”`);
  } catch (e) {
    app.showError(e);
  }
}

export async function printNote() {
  try {
    await app.flush();
    await api.printNote();
  } catch (e) {
    app.showError(e);
  }
}

/** Pick Markdown files and turn each into a note in the current folder. */
export async function importMarkdown() {
  try {
    await app.flush();
    const files = await api.pickMarkdownFiles();
    if (files.length === 0) return;
    const folderId = app.filter.kind === "folder" ? app.filter.folderId : null;
    let first: string | null = null;
    let missingImages = 0;
    for (const file of files) {
      const parsed = markdownToDoc(file.text);
      let doc: DocJSON = parsed.doc;
      // An empty file still gets a title: its file name.
      if (!extractText(doc).title) {
        doc = { ...doc, content: [{ type: "paragraph", content: [{ type: "text", text: file.name }] }, ...(doc.content ?? [])] };
      }
      const note = await api.createNote(folderId);
      for (const img of parsed.images) {
        try {
          const a = await api.importImage(note.id, file.path, img.src);
          doc = resolveImage(doc, img.src, a.id, img.alt);
        } catch {
          missingImages++;
          doc = resolveImage(doc, img.src, null, img.alt);
        }
      }
      await api.updateNoteContent(note.id, doc);
      first ??= note.id;
    }
    await app.reloadNotes(first);
    const n = files.length;
    app.showInfo(
      `Imported ${n} note${n === 1 ? "" : "s"}` +
        (missingImages ? ` (${missingImages} image${missingImages === 1 ? "" : "s"} couldn't be found)` : ""),
    );
  } catch (e) {
    app.showError(e);
  }
}
