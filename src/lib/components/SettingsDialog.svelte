<script lang="ts">
  // The Settings window (Ctrl+,): General, Appearance, Backup, Shortcuts, About.
  import { onMount } from "svelte";
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { Cloud, Info, Keyboard, Palette, SlidersHorizontal, X } from "@lucide/svelte";
  import { api, isTauri } from "../api";
  import { app, type SettingsSection } from "../stores/app.svelte";
  import { prefs, type FirstLine, type SortBy, type TextSize, type Theme } from "../stores/prefs.svelte";
  import type { AppInfo } from "../types";
  import BackupPanel, { type BackupView } from "./BackupPanel.svelte";

  const REPO_URL = "https://github.com/Rick8509/tuxnotes";

  const sections: { id: SettingsSection; label: string; icon: typeof Cloud }[] = [
    { id: "general", label: "General", icon: SlidersHorizontal },
    { id: "appearance", label: "Appearance", icon: Palette },
    { id: "backup", label: "Backup", icon: Cloud },
    { id: "shortcuts", label: "Shortcuts", icon: Keyboard },
    { id: "about", label: "About", icon: Info },
  ];

  const section = $derived(app.settingsSection ?? "general");
  let backupView = $state<BackupView>("home");
  let info = $state<AppInfo | null>(null);
  let windowEl = $state<HTMLDivElement>();

  onMount(() => {
    windowEl?.focus();
    api.appInfo().then((i) => (info = i)).catch(() => {});
  });

  /** Can't close or switch sections while a backup/restore is running. */
  const locked = $derived(section === "backup" && backupView === "working");

  function go(id: SettingsSection) {
    if (!locked) app.settingsSection = id;
  }

  function close() {
    if (!locked) app.settingsSection = null;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== "Escape" || app.confirmRequest) return;
    e.preventDefault();
    if (section === "backup" && backupView === "restore") backupView = "home";
    else close();
  }

  function setSort(value: SortBy) {
    prefs.set("sortBy", value);
    app.resortNotes();
  }

  async function openRepo() {
    try {
      if (isTauri) await openUrl(REPO_URL);
      else window.open(REPO_URL, "_blank", "noopener");
    } catch (e) {
      app.showError(`Couldn't open the browser: ${e}`);
    }
  }

  async function openNotesFolder() {
    if (!info) return;
    try {
      if (isTauri) await revealItemInDir(info.database);
      else app.showInfo(info.dataDir);
    } catch (e) {
      app.showError(`Couldn't open the folder: ${e}`);
    }
  }

  const themes: { id: Theme; label: string }[] = [
    { id: "system", label: "System" },
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
  ];
  const sizes: { id: TextSize; label: string }[] = [
    { id: "small", label: "Small" },
    { id: "medium", label: "Medium" },
    { id: "large", label: "Large" },
    { id: "xlarge", label: "Extra Large" },
  ];
  const firstLines: { id: FirstLine; label: string }[] = [
    { id: "title", label: "Title" },
    { id: "heading", label: "Heading" },
    { id: "body", label: "Body" },
  ];

  const shortcuts: { group: string; items: [string, string][] }[] = [
    {
      group: "Notes",
      items: [
        ["New note", "Ctrl+N"],
        ["New folder", "Ctrl+Shift+N"],
        ["Find in the open note", "Ctrl+F (in a note)"],
        ["Search all notes", "Ctrl+Shift+F"],
        ["Delete note", "Ctrl+Delete"],
        ["Settings", "Ctrl+,"],
      ],
    },
    {
      group: "Formatting",
      items: [
        ["Bold / Italic / Underline", "Ctrl+B / I / U"],
        ["Strikethrough", "Ctrl+Shift+S"],
        ["Checklist", "Ctrl+Shift+L"],
        ["Title / Heading / Subheading", "Ctrl+Shift+T / H / J"],
        ["Body text", "Ctrl+Shift+B"],
        ["Add or edit a link", "Ctrl+K"],
        ["Open link in browser", "Ctrl+Click"],
      ],
    },
    {
      group: "Checklists",
      items: [
        ["Check all, uncheck all, delete checked", "Right-click a checklist"],
      ],
    },
    {
      group: "Tables",
      items: [
        ["Next / previous cell", "Tab / Shift+Tab"],
        ["New row (in the last cell)", "Tab"],
        ["Row and column actions", "Right-click a cell"],
      ],
    },
    {
      group: "Window",
      items: [
        ["Show or hide sidebar", "Ctrl+Alt+S"],
        ["Focus mode", "Ctrl+Alt+F"],
        ["Full screen", "F11"],
        ["Zoom in / out", "Ctrl+= / Ctrl+-"],
      ],
    },
    {
      group: "Note list",
      items: [
        ["Previous / next note", "↑ / ↓"],
        ["Start typing in the note", "Enter or Tab"],
        ["Delete the selected note", "Delete"],
      ],
    },
  ];
</script>

<svelte:window {onkeydown} />

<div class="scrim">
  <div class="window" role="dialog" aria-modal="true" aria-label="Settings" tabindex="-1" bind:this={windowEl}>
    <nav aria-label="Settings sections">
      <h1>Settings</h1>
      {#each sections as s (s.id)}
        <button class="nav-item" class:active={section === s.id} disabled={locked && section !== s.id} onclick={() => go(s.id)}>
          <s.icon size={16} />
          {s.label}
        </button>
      {/each}
    </nav>

    <div class="content">
      <button class="icon-btn close" aria-label="Close settings" disabled={locked} onclick={close}><X size={16} /></button>

      {#if section === "general"}
        <h2>General</h2>
        <div class="group">
          <div class="row">
            <div class="label">
              <span>Sort notes by</span>
            </div>
            <select value={prefs.sortBy} onchange={(e) => setSort(e.currentTarget.value as SortBy)}>
              <option value="edited">Date Edited</option>
              <option value="created">Date Created</option>
              <option value="title">Title</option>
            </select>
          </div>
          <label class="row">
            <div class="label">
              <span>Group notes by date</span>
              <span class="hint">Today, Yesterday, Previous 7 Days, …</span>
            </div>
            <input
              type="checkbox"
              class="switch"
              checked={prefs.groupByDate}
              disabled={prefs.sortBy === "title"}
              onchange={(e) => prefs.set("groupByDate", e.currentTarget.checked)}
            />
          </label>
          <label class="row">
            <div class="label">
              <span>Move checked items to the bottom</span>
              <span class="hint">Ticked checklist items move below the unticked ones.</span>
            </div>
            <input
              type="checkbox"
              class="switch"
              checked={prefs.sortChecked}
              onchange={(e) => prefs.set("sortChecked", e.currentTarget.checked)}
            />
          </label>
          <div class="row">
            <div class="label">
              <span>New notes start with</span>
              <span class="hint">How the first line of a note looks.</span>
            </div>
            <div class="segmented" role="radiogroup" aria-label="New notes start with">
              {#each firstLines as f (f.id)}
                <button role="radio" aria-checked={prefs.firstLine === f.id} class:on={prefs.firstLine === f.id} onclick={() => prefs.set("firstLine", f.id)}>
                  {f.label}
                </button>
              {/each}
            </div>
          </div>
        </div>
        <p class="footnote">Notes in Recently Deleted are removed permanently after 30 days.</p>

      {:else if section === "appearance"}
        <h2>Appearance</h2>
        <div class="group">
          <div class="row stacked">
            <span>Theme</span>
            <div class="themes" role="radiogroup" aria-label="Theme">
              {#each themes as t (t.id)}
                <button class="theme" role="radio" aria-checked={prefs.theme === t.id} class:on={prefs.theme === t.id} onclick={() => prefs.set("theme", t.id)}>
                  <span class="preview {t.id}">
                    <span class="pv-side"></span><span class="pv-list"></span><span class="pv-editor"><span></span><span></span></span>
                  </span>
                  {t.label}
                </button>
              {/each}
            </div>
          </div>
          <div class="row stacked">
            <span>Text size</span>
            <div class="segmented wide" role="radiogroup" aria-label="Text size">
              {#each sizes as s (s.id)}
                <button role="radio" aria-checked={prefs.textSize === s.id} class:on={prefs.textSize === s.id} onclick={() => prefs.set("textSize", s.id)}>
                  {s.label}
                </button>
              {/each}
            </div>
            <p class="sample size-{prefs.textSize}">The quick brown fox jumps over the lazy dog.</p>
          </div>
        </div>

      {:else if section === "backup"}
        <BackupPanel bind:view={backupView} />

      {:else if section === "shortcuts"}
        <h2>Keyboard Shortcuts</h2>
        {#each shortcuts as g (g.group)}
          <h3>{g.group}</h3>
          <div class="group">
            {#each g.items as [action, keys] (action)}
              <div class="row compact">
                <span>{action}</span>
                <kbd>{keys}</kbd>
              </div>
            {/each}
          </div>
        {/each}

      {:else if section === "about"}
        <div class="about">
          <img src="/icon.png" alt="" width="72" height="72" />
          <h2>TuxNotes</h2>
          <p class="tagline">Open notes for Linux</p>
          <p class="muted">Version {info?.version ?? "…"} · MIT License</p>
          <div class="about-actions">
            <button class="btn" onclick={openRepo}>GitHub</button>
            <button class="btn" disabled={!info} onclick={openNotesFolder}>Open Notes Folder</button>
          </div>
        </div>
        <div class="group">
          <div class="row compact">
            <span>Notes</span>
            <code>{info?.database ?? "…"}</code>
          </div>
          <div class="row compact">
            <span>Settings</span>
            <code>{info?.configDir ?? "…"}</code>
          </div>
        </div>
        <p class="footnote">Made with Tauri, Svelte and TipTap.</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 95;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.28);
  }
  .window {
    display: grid;
    grid-template-columns: 190px minmax(0, 1fr);
    width: min(720px, calc(100vw - 40px));
    height: min(520px, calc(100vh - 40px));
    border-radius: var(--radius-lg);
    overflow: hidden;
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    outline: none;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 18px 10px;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--separator);
  }
  nav h1 {
    margin: 0 8px 12px;
    font-size: 15px;
    font-weight: 700;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    text-align: left;
  }
  .nav-item :global(svg) {
    color: var(--accent);
  }
  .nav-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .nav-item.active {
    background: var(--sel-sidebar);
    font-weight: 600;
  }
  .nav-item:disabled {
    opacity: 0.4;
  }

  .content {
    position: relative;
    overflow-y: auto;
    padding: 18px 26px 24px;
    user-select: text;
    -webkit-user-select: text;
  }
  .close {
    position: absolute;
    top: 12px;
    right: 12px;
  }
  h2 {
    margin: 0 0 14px;
    font-size: 18px;
    font-weight: 700;
  }
  h3 {
    margin: 16px 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  /* Grouped rows, like system settings. */
  .group {
    border-radius: var(--radius-md);
    background: var(--bg-input);
    padding: 2px 14px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 52px;
    padding: 10px 0;
  }
  .row + .row {
    border-top: 1px solid var(--separator);
  }
  .row.compact {
    min-height: 38px;
    padding: 7px 0;
  }
  .row.stacked {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .hint {
    font-size: 12px;
    color: var(--text-secondary);
  }
  .footnote {
    margin: 12px 2px 0;
    font-size: 12px;
    color: var(--text-tertiary);
  }
  .muted {
    color: var(--text-secondary);
  }

  select {
    height: 30px;
    min-width: 150px;
    padding: 0 8px;
    border: 1px solid var(--separator-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-editor);
    color: var(--text);
    font: inherit;
  }

  /* iOS-style switch */
  .switch {
    appearance: none;
    -webkit-appearance: none;
    flex: none;
    width: 38px;
    height: 22px;
    margin: 0;
    border-radius: 11px;
    background: var(--separator-strong);
    position: relative;
    transition: background 0.15s;
  }
  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
    transition: transform 0.15s;
  }
  .switch:checked {
    background: var(--accent);
  }
  .switch:checked::after {
    transform: translateX(16px);
  }
  .switch:disabled {
    opacity: 0.4;
  }

  /* Segmented control */
  .segmented {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--bg-active);
  }
  .segmented.wide {
    display: flex;
  }
  .segmented button {
    flex: 1;
    padding: 5px 12px;
    border-radius: 5px;
    font-weight: 500;
    white-space: nowrap;
  }
  .segmented button.on {
    background: var(--bg-editor);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15);
    font-weight: 600;
  }

  /* Theme picker with mini window previews */
  .themes {
    display: flex;
    gap: 14px;
  }
  .theme {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    font-weight: 500;
  }
  .preview {
    display: grid;
    grid-template-columns: 18px 26px 1fr;
    width: 104px;
    height: 66px;
    border-radius: 8px;
    overflow: hidden;
    border: 2px solid transparent;
    box-shadow: 0 0 0 1px var(--separator-strong);
  }
  .theme.on .preview {
    border-color: var(--accent);
    box-shadow: none;
  }
  .preview.light {
    --pv-side: #f0f0f2;
    --pv-list: #fff;
    --pv-ed: #fff;
    --pv-line: #d2d2d7;
  }
  .preview.dark {
    --pv-side: #232325;
    --pv-list: #1c1c1e;
    --pv-ed: #1c1c1e;
    --pv-line: #3a3a3d;
  }
  .preview.system {
    --pv-side: #f0f0f2;
    --pv-list: #fff;
    --pv-ed: linear-gradient(135deg, #fff 50%, #1c1c1e 50%);
    --pv-line: #a1a1a6;
  }
  .pv-side {
    background: var(--pv-side);
  }
  .pv-list {
    background: var(--pv-list);
    border-left: 1px solid var(--pv-line);
    border-right: 1px solid var(--pv-line);
  }
  .pv-editor {
    background: var(--pv-ed);
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 10px 8px;
  }
  .pv-editor span {
    height: 4px;
    border-radius: 2px;
    background: #e0a500;
    width: 70%;
  }
  .pv-editor span + span {
    background: var(--pv-line);
    width: 90%;
  }

  .sample {
    margin: 2px 0 0;
    color: var(--text-secondary);
  }
  .sample.size-small {
    font-size: 13px;
  }
  .sample.size-medium {
    font-size: 15px;
  }
  .sample.size-large {
    font-size: 17px;
  }
  .sample.size-xlarge {
    font-size: 20px;
  }

  kbd {
    padding: 2px 8px;
    border-radius: 5px;
    background: var(--bg-editor);
    border: 1px solid var(--separator-strong);
    font-family: var(--font-ui);
    font-size: 12px;
    white-space: nowrap;
  }
  code {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
    text-align: right;
    word-break: break-all;
  }

  .about {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 8px 0 18px;
  }
  .about img {
    border-radius: 16px;
    margin-bottom: 8px;
  }
  .about h2 {
    margin: 0;
  }
  .tagline {
    margin: 2px 0 4px;
    font-weight: 500;
  }
  .about p {
    margin-top: 2px;
    margin-bottom: 0;
  }
  .about-actions {
    display: flex;
    gap: 8px;
    margin-top: 14px;
  }
  .btn {
    height: 32px;
    padding: 0 14px;
    border-radius: var(--radius-md);
    background: var(--bg-input);
    font-weight: 600;
  }
  .btn:hover:not(:disabled) {
    background: var(--bg-active);
  }
  .btn:disabled {
    opacity: 0.5;
  }
</style>
