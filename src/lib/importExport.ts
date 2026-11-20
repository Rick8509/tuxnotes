// Export the open note (Markdown, PDF, print) and import Markdown files as notes.

import { api } from "./api";
import { fitToEditor, markdownToDoc, noteToMarkdown, resolveImage } from "./markdown";
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
    if (!note?.content) return; // nothing to export (or locked)
    const path = await api.exportMarkdown(note.title || "Note", noteToMarkdown(note.content));
    if (path) app.showInfo(`Exported “${fileName(path)}”`);
  } catch (e) {
    app.showError(e);
  }
}

export async function exportPdf() {
  try {
    const note = await savedCurrent();
    if (!note?.content) return; // nothing to export (or locked)
    const path = await api.exportPdf(note.title || "Note");
    if (path) app.showInfo(`Saved “${fileName(path)}”`);
  } catch (e) {
    app.showError(e);
  }
}

export async function printNote() {
  if (!app.current?.content) return; // a hidden locked note: nothing to print
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
    let imported = 0;
    let missingImages = 0;
    let failed = 0;
    let lastError: unknown = null;
    // Each file on its own: one that fails doesn't stop the rest or leave an empty note.
    for (const file of files) {
      let noteId: string | null = null;
      try {
        const parsed = markdownToDoc(file.text);
        let doc: DocJSON = parsed.doc;
        // An empty file still gets a title: its file name.
        if (!extractText(doc).title) {
          doc = { ...doc, content: [{ type: "paragraph", content: [{ type: "text", text: file.name }] }, ...(doc.content ?? [])] };
        }
        const note = await api.createNote(folderId);
        noteId = note.id;
        for (const img of parsed.images) {
          try {
            const a = await api.importImage(note.id, file.path, img.src);
            doc = resolveImage(doc, img.src, a.id, img.alt);
          } catch {
            missingImages++;
            doc = resolveImage(doc, img.src, null, img.alt);
          }
        }
        await api.updateNoteContent(note.id, fitToEditor(doc).doc);
        first ??= note.id;
        imported++;
      } catch (e) {
        failed++;
        lastError = e;
        console.warn(`couldn't import ${file.name}:`, e);
        if (noteId) await api.discardNoteIfBlank(noteId).catch(() => {});
      }
    }
    await app.reloadNotes(first);
    if (imported === 0 && lastError) {
      app.showError(lastError);
      return;
    }
    const notes = `Imported ${imported} note${imported === 1 ? "" : "s"}`;
    const extras = [
      missingImages && `${missingImages} image${missingImages === 1 ? "" : "s"} couldn't be found`,
      failed && `${failed} file${failed === 1 ? "" : "s"} couldn't be imported`,
    ].filter(Boolean);
    app.showInfo(extras.length ? `${notes} (${extras.join(", ")})` : notes);
  } catch (e) {
    app.showError(e);
  }
}
