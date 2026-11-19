# TuxNotes

**Open notes for Linux.** A lightweight, local-first notes app with a clean three-pane design inspired by Apple Notes.

> Status: early development (v0.2).

## Features

- Three panes: folders, note list and editor
- Rich text: title, heading and subheading styles, bold, italic, underline, strikethrough, monospaced blocks, quotes
- Checklists with round checkboxes, plus bulleted and numbered lists
- **Tables**: insert from the toolbar, right-click a cell to add or remove rows and columns, Tab between cells, drag borders to resize columns
- **Images and attachments**: paste a screenshot, drag in files, or use the paperclip button. Images show in the note; other files appear as chips you can open (double-click) or save a copy of. Attachments are included in backups
- Pinned notes, folders, and **Recently Deleted** (notes are removed permanently after 30 days)
- Fast full-text search that ignores accents ("cafe" finds "café")
- **Google Drive backup and restore**: manual or automatic (daily/weekly), keeps the last N backups, and restores on any computer. TuxNotes can only see files it created.
- Rounded, borderless window. Hide the sidebar, use focus mode to hide both side panes, and drag the dividers to resize them; the window remembers its size and layout
- Notes grouped like Apple Notes (Pinned, Today, Yesterday, Previous 7 Days, …); drag a note onto a folder to move it
- **Settings** (`Ctrl+,`): sort notes by date edited, date created or title; date grouping on or off; how new notes start; System, Light or Dark theme; editor text size
- Autosave, keyboard-first workflow
- Small and fast: built with [Tauri](https://tauri.app) and stored in a single SQLite file

### Keyboard shortcuts

| Action | Shortcut |
|---|---|
| New note | `Ctrl+N` |
| Settings | `Ctrl+,` |
| New folder | `Ctrl+Shift+N` |
| Search | `Ctrl+F` |
| Delete note | `Ctrl+Delete` |
| Checklist | `Ctrl+Shift+L` |
| Title / Heading / Subheading / Body | `Ctrl+Shift+T` / `H` / `J` / `B` |
| Bold / Italic / Underline / Strikethrough | `Ctrl+B` / `I` / `U` / `Ctrl+Shift+S` |
| Show/hide sidebar | `Ctrl+Alt+S` |
| Focus mode (hide sidebar and note list) | `Ctrl+Alt+F` |
| Full screen | `F11` |
| Zoom in / out | `Ctrl+=` / `Ctrl+-` |
| Open the selected note for typing (in the note list) | `Enter` or `Tab` |
| Delete the selected note (in the note list) | `Delete` |
| Open a link in your browser | `Ctrl+click` |
| Next / previous table cell (Tab in the last cell adds a row) | `Tab` / `Shift+Tab` |

## Where are my notes?

Everything is stored locally in `~/.local/share/io.github.tuxnotes.TuxNotes/`: `notes.db` (SQLite) and an `attachments/` folder for images and files. Settings live in `~/.config/io.github.tuxnotes.TuxNotes/settings.json`.

Before a restore, TuxNotes saves a copy of your current notes in `~/.local/share/io.github.tuxnotes.TuxNotes/safety/`.

## Google Drive backup

Open **Settings** (the gear at the bottom of the sidebar, or `Ctrl+,`) → **Backup**, then click **Back Up to Google Drive** or **Restore from Google Drive**. TuxNotes asks before opening your browser to sign in with Google; once you've allowed access, it does the backup (and turns on daily automatic backups), or shows your backups to restore.

Release builds come with Google sign-in built in. If you build TuxNotes yourself, see [docs/google-setup.md](docs/google-setup.md) to create your own (free) Google OAuth client.

## Building from source

### 1. Install the requirements (Debian/Ubuntu)

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev libxdo-dev build-essential curl wget file
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust 1.90 or newer
# Node.js 20+ is also required
```

For Fedora, Arch and other distributions, see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

### 2. Run it in development mode

```bash
npm install            # first time only, and after dependencies change
npm run tauri dev
```

Leave it running while you work:

- Changes in `src/` (the interface) appear in the window almost instantly, with no restart.
- Changes in `src-tauri/` or `crates/` (Rust) make it recompile and restart the app automatically (usually 5–20 s).

The first build takes a few minutes; later builds are incremental.

### 3. Build installable packages

```bash
npx tauri build --bundles deb    # only the .deb (fastest)
npx tauri build                  # .deb, .rpm and AppImage
```

Packages are written to `target/release/bundle/<format>/`. To install or update your local copy:

```bash
sudo apt install ./target/release/bundle/deb/TuxNotes_*_amd64.deb
```

Rebuilding or reinstalling never touches your notes, which live in `~/.local/share/io.github.tuxnotes.TuxNotes/`.

### Working on the UI without Rust

`npm run dev` serves the interface at http://localhost:1420 in a normal browser. A built-in mock backend stores data in `localStorage`, so no Rust toolchain is needed.

## Project layout

```
crates/tuxnotes-core/   Storage engine: SQLite, migrations, search, trash, backup archives (no GUI deps)
crates/tuxnotes-gdrive/ Google sign-in (loopback + PKCE) and Google Drive v3 client
src-tauri/              Desktop shell: window and IPC commands that call tuxnotes-core
src/                    Svelte 5 + TipTap user interface
```

## Development checks

Run these before committing. CI runs the same checks on every pull request.

```bash
cargo fmt --all                   # format Rust code
cargo test --workspace            # storage, Drive client and app tests
cargo clippy --workspace --all-targets -- -D warnings
npm run check && npm test         # typecheck + frontend unit tests
```

## Releasing

Push a version tag; GitHub Actions builds the Linux packages and creates a draft release:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

Bump the version first in `package.json`, `Cargo.toml` (workspace) and `src-tauri/tauri.conf.json`.

## Roadmap

- [x] v0.1: folders, notes, rich text, checklists, search, pinning, Recently Deleted
- [x] v0.2: backup to and restore from Google Drive, with automatic scheduled backups
- [ ] v0.3: tables ✅, images and attachments ✅, Markdown and PDF export, encrypted backups
- [ ] Flathub release

## License

[MIT](LICENSE)
