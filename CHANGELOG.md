# Changelog

All notable changes to TuxNotes are listed here.

## Unreleased

### Added
- **Undo delete, duplicate and note info.** Deleting a note shows a message with **Undo** (or press Ctrl+Z in the note list) that brings it straight back. **Duplicate** (note context menu or ⋯ menu) copies a note into the same folder. **⋯ → Note Info…** shows word and character counts, checklist progress, attachments, the folder path, and created/modified dates.
- **Subfolders.** Right-click a folder → **New Subfolder…** (up to 5 levels). Drag a folder onto the middle of another to nest it, onto its top or bottom edge to reorder, or onto the "Folders" label to move it back to the top level. Expand and collapse branches with the arrow (remembered). Folder names only need to be unique within the same parent. Deleting a folder deletes its subfolders too; all their notes go to Recently Deleted. The **Move to** menu shows the folder tree.
- **Export and import** (the **⋯** button in the editor): **Export as Markdown…** saves the note as a `.md` file, copying its images and files into a "‹name› files" folder next to it; **Export as PDF…** saves a PDF directly (no print dialog), always in light colours and showing only the note; **Print…** (`Ctrl+P`); **Import Markdown Files…** turns Markdown files into notes in the current folder, including checklists, tables and images that sit next to the file. Images from the web become links (nothing is downloaded), and only images inside the imported file's folder are attached.
- **Find in note** (`Ctrl+F` while typing in a note): a find bar with a match counter, next/previous (`Enter` / `Shift+Enter`, wrapping around) and Done (`Esc`, leaving the cursor on the match). Ignores case and accents. **Search highlights:** while you search all notes (`Ctrl+Shift+F`, or `Ctrl+F` outside a note), the search words are highlighted in the note you open, which scrolls to the first match.
- **Checklist extras.** Ticked items move to the bottom of their checklist and unticked ones move back up (Settings → General → "Move checked items to the bottom", on by default; one Undo reverts both the tick and the move). Right-click a checklist for **Check All**, **Uncheck All**, **Move Checked to Bottom** and **Delete Checked Items**.
- **Link editing.** `Ctrl+K` (or **Link…** in the Aa menu) turns selected text into a link, edits the link under the cursor, or inserts a new link with its own text. Type `example.com` or `name@example.com` and TuxNotes adds the `https://` or `mailto:` part; anything that isn't a web, email or phone link is refused. With the cursor in a link, a small bubble shows the address with **Open**, **Edit** and **Remove**. `Ctrl+click` still opens links directly.
- **Images and attachments.** Paste a screenshot, drag files in from your file manager, or use the paperclip button. Images show in the note; other files appear as a chip with type, name and size: double-click to open, right-click to open, save a copy or delete. Up to 50 MB per file. File names are searchable. Attachments are stored next to your notes and included in backups. Files no longer used by any note are cleaned up after a day, so undo keeps working.
- **Tables.** Insert one from the toolbar; right-click a cell to add or delete rows and columns, merge or split cells, or turn the header row on or off. Tab and Shift+Tab move between cells, and Tab in the last cell adds a row. Drag a column border to resize it. Text inside tables is searchable and shows in the note preview.
- **Settings window** (`Ctrl+,`): sort and grouping options, how new notes start, System/Light/Dark theme, text size, backup, keyboard shortcuts, and an About page.
- **Google Drive backup and restore**: back up or restore with one click. TuxNotes asks before opening your browser to sign in, then carries on by itself. Automatic daily or weekly backups, and keeps the last N.
- Rounded window, hideable sidebar, focus mode, resizable panes, Apple-style date sections, drag notes onto folders, and the window remembers its size.
- Notes, folders, rich text, checklists, pinning, Recently Deleted (30 days) and accent-insensitive search.

### Safety
- If a note contains something the running version can't show (for example, a note written by a newer TuxNotes), it opens **read-only** with its text visible, so it can't be overwritten by mistake.

### Compatibility
- Backups now use **format 2**, which includes attachments. Older backups (format 1) still restore normally.
- Once this version has opened your notes, its database (schema 3) can't be opened by earlier TuxNotes builds. Update every computer you use.
- Notes that contain **tables** can't be shown by TuxNotes builds from before table support. Update every computer you use before restoring a backup that contains tables.
