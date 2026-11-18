<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { Cloud, CloudUpload, LoaderCircle, RefreshCw, RotateCcw, X } from "@lucide/svelte";
  import { api, isTauri } from "../api";
  import { formatBytes, formatEditorDate } from "../format";
  import { app } from "../stores/app.svelte";
  import type { AutoBackup, BackupInfo, BackupStatus } from "../types";

  let status = $state<BackupStatus | null>(null);
  let backups = $state<BackupInfo[] | null>(null);
  let loadingList = $state(false);
  let listError = $state<string | null>(null);
  let clientId = $state("");
  let clientSecret = $state("");
  let dialog = $state<HTMLDivElement>();

  const busy = $derived(status?.busy ?? null);
  const configured = $derived(!!status && status.clientSource !== "none");

  async function refreshStatus() {
    try {
      status = await api.backupStatus();
    } catch (e) {
      app.showError(e);
    }
  }

  async function refreshList() {
    if (!status?.signedIn) {
      backups = null;
      return;
    }
    loadingList = true;
    listError = null;
    try {
      backups = await api.listBackups();
    } catch (e) {
      listError = e instanceof Error ? e.message : String(e);
    } finally {
      loadingList = false;
      await refreshStatus(); // e.g. sign-in may have expired
    }
  }

  onMount(() => {
    dialog?.focus();
    void refreshStatus().then(refreshList);
    // Automatic backups run in the background; keep this window in sync.
    let unlisten: (() => void) | undefined;
    if (isTauri) {
      listen("backup-status-changed", () => void refreshStatus().then(refreshList)).then(
        (fn) => (unlisten = fn),
      );
    }
    return () => unlisten?.();
  });

  /** Run an action with the busy state reflected immediately in the UI. */
  async function run<T>(kind: BackupStatus["busy"], action: () => Promise<T>): Promise<T | undefined> {
    if (status) status = { ...status, busy: kind };
    try {
      return await action();
    } catch (e) {
      // Cancelling sign-in is the user's own choice, not an error worth a toast.
      if (!String(e instanceof Error ? e.message : e).includes("Sign-in cancelled")) app.showError(e);
      return undefined;
    } finally {
      await refreshStatus();
    }
  }

  async function signIn() {
    const s = await run("signingIn", () => api.googleSignIn());
    if (s) {
      app.showInfo(s.account ? `Connected to ${s.account}` : "Connected to Google Drive");
      await refreshList();
    }
  }

  async function signOut() {
    const ok = await app.confirm({
      title: "Disconnect Google Drive?",
      message: "Backups already on Google Drive are kept. Automatic backups stop until you sign in again.",
      confirmLabel: "Disconnect",
    });
    if (!ok) return;
    await run("signingIn", () => api.googleSignOut());
    backups = null;
  }

  async function backupNow() {
    await app.flush(); // include the latest edits
    const info = await run("backingUp", () => api.backupNow());
    if (info) {
      app.showInfo(`Backed up ${info.noteCount ?? 0} notes to Google Drive`);
      await refreshList();
    }
  }

  async function restore(b: BackupInfo) {
    const when = b.createdAt ? formatEditorDate(b.createdAt) : b.name;
    const ok = await app.confirm({
      title: "Restore this backup?",
      message:
        `All notes on this computer will be replaced with the backup from ${when}` +
        (b.noteCount != null ? ` (${b.noteCount} notes)` : "") +
        ". A copy of your current notes is saved on this computer first.",
      confirmLabel: "Restore",
      destructive: true,
    });
    if (!ok) return;
    await app.flush();
    const result = await run("restoring", () => api.restoreBackup(b.id));
    if (result) {
      await app.reloadAll();
      app.showInfo(`Restored ${result.noteCount} notes from backup`);
      app.showBackup = false;
    }
  }

  async function setPreferences(autoBackup: AutoBackup, keepBackups: number) {
    try {
      status = await api.setBackupPreferences(autoBackup, keepBackups);
    } catch (e) {
      app.showError(e);
    }
  }

  async function saveClient() {
    try {
      status = await api.setGoogleClient(clientId.trim() || null, clientSecret.trim() || null);
      app.showInfo(clientId.trim() ? "Custom Google client saved" : "Using the built-in Google client");
    } catch (e) {
      app.showError(e);
    }
  }

  function close() {
    if (busy === "signingIn") void api.googleCancelSignIn();
    app.showBackup = false;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !app.confirmRequest) {
      e.preventDefault();
      close();
    }
  }

  const sourceLabel: Record<string, string> = {
    builtIn: "Built-in",
    custom: "Custom (from Advanced)",
    environment: "From TUXNOTES_GOOGLE_CLIENT_ID",
    none: "Not set up",
  };
</script>

<svelte:window {onkeydown} />

<div class="scrim">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="backup-title" tabindex="-1" bind:this={dialog}>
    <header>
      <h2 id="backup-title">Backup &amp; Restore</h2>
      <button class="icon-btn" aria-label="Close" onclick={close}><X size={16} /></button>
    </header>

    {#if !status}
      <div class="center"><LoaderCircle size={20} class="spin" /></div>
    {:else}
      <!-- ------------------------------------------------ account -->
      <section>
        <h3>Google Drive</h3>
        {#if !configured}
          <p class="muted">
            This build of TuxNotes doesn't include a Google sign-in client. You can add your own under
            <strong>Advanced</strong> below. <code>docs/google-setup.md</code> explains how.
          </p>
        {:else if status.signedIn}
          <div class="account">
            <Cloud size={18} />
            <span class="grow">{status.account ?? "Connected"}</span>
            <button class="link" disabled={!!busy} onclick={signOut}>Disconnect</button>
          </div>
        {:else if busy === "signingIn"}
          <div class="account">
            <LoaderCircle size={18} class="spin" />
            <span class="grow">Finish signing in in your web browser…</span>
            <button class="link" onclick={() => api.googleCancelSignIn()}>Cancel</button>
          </div>
        {:else}
          <p class="muted">
            Back up your notes to your own Google Drive and restore them on any computer. TuxNotes can only
            see the backups it creates.
          </p>
          <button class="btn primary" disabled={!!busy} onclick={signIn}>Sign in with Google</button>
        {/if}
      </section>

      {#if status.signedIn}
        <!-- ---------------------------------------------- backup now -->
        <section>
          <div class="row">
            <div class="grow">
              {#if busy === "backingUp"}
                <span>Backing up…</span>
              {:else if status.lastBackupError}
                <span class="error">Last backup failed: {status.lastBackupError}</span>
              {:else if status.lastBackupAt}
                <span>Last backup: {formatEditorDate(status.lastBackupAt)}</span>
              {:else}
                <span class="muted">No backups yet</span>
              {/if}
            </div>
            <button class="btn primary" disabled={!!busy} onclick={backupNow}>
              {#if busy === "backingUp"}<LoaderCircle size={15} class="spin" />{:else}<CloudUpload size={15} />{/if}
              Back Up Now
            </button>
          </div>

          <div class="prefs">
            <label>
              Automatic backups
              <select
                value={status.autoBackup}
                onchange={(e) => setPreferences(e.currentTarget.value as AutoBackup, status!.keepBackups)}
              >
                <option value="off">Off</option>
                <option value="daily">Daily</option>
                <option value="weekly">Weekly</option>
              </select>
            </label>
            <label>
              Keep on Drive
              <select
                value={String(status.keepBackups)}
                onchange={(e) => setPreferences(status!.autoBackup, Number(e.currentTarget.value))}
              >
                {#each [5, 10, 20, 50] as n (n)}
                  <option value={String(n)}>Last {n} backups</option>
                {/each}
                {#if ![5, 10, 20, 50].includes(status.keepBackups)}
                  <option value={String(status.keepBackups)}>Last {status.keepBackups} backups</option>
                {/if}
              </select>
            </label>
          </div>
        </section>

        <!-- ---------------------------------------------- restore list -->
        <section>
          <div class="row">
            <h3 class="grow">Backups on Google Drive</h3>
            <button class="icon-btn" aria-label="Refresh" title="Refresh" disabled={loadingList || !!busy} onclick={refreshList}>
              <RefreshCw size={14} />
            </button>
          </div>
          {#if listError}
            <p class="error">{listError}</p>
          {:else if !backups}
            <div class="center"><LoaderCircle size={18} class="spin" /></div>
          {:else if backups.length === 0}
            <p class="muted">No backups yet. Click “Back Up Now” to create the first one.</p>
          {:else}
            <ul class="backups">
              {#each backups as b (b.id)}
                <li>
                  <div class="grow">
                    <div>{b.createdAt ? formatEditorDate(b.createdAt) : b.name}</div>
                    <div class="muted small">
                      {#if b.noteCount != null}{b.noteCount} notes{/if}
                      {#if b.size != null} · {formatBytes(b.size)}{/if}
                    </div>
                  </div>
                  <button class="btn" disabled={!!busy} onclick={() => restore(b)}>
                    {#if busy === "restoring"}<LoaderCircle size={14} class="spin" />{:else}<RotateCcw size={14} />{/if}
                    Restore
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}

      <!-- ------------------------------------------------ advanced -->
      <details>
        <summary>Advanced</summary>
        <p class="muted small">
          Google sign-in client: <strong>{sourceLabel[status.clientSource]}</strong>. To use your own Google
          Cloud OAuth client (type “Desktop app”), enter it here. Leave empty to use the built-in one.
        </p>
        <input placeholder="Client ID" bind:value={clientId} spellcheck="false" aria-label="Google client ID" />
        <input
          placeholder="Client secret"
          type="password"
          bind:value={clientSecret}
          spellcheck="false"
          aria-label="Google client secret"
        />
        <button class="btn" disabled={!!busy} onclick={saveClient}>Save</button>
      </details>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 95;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    width: min(500px, calc(100vw - 40px));
    max-height: calc(100vh - 60px);
    overflow-y: auto;
    padding: 18px 20px 20px;
    border-radius: var(--radius-lg);
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    outline: none;
    user-select: text;
    -webkit-user-select: text;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  section {
    padding: 14px 0;
    border-bottom: 1px solid var(--separator);
  }
  p {
    margin: 0 0 12px;
  }
  .muted {
    color: var(--text-secondary);
  }
  .small {
    font-size: 12px;
  }
  .error {
    color: var(--danger);
  }
  .row,
  .account {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .row h3 {
    margin: 0;
  }
  .account :global(svg) {
    color: var(--accent);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .center {
    display: grid;
    place-items: center;
    padding: 16px;
    color: var(--text-secondary);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn:hover:not(:disabled) {
    background: var(--bg-active);
  }
  .btn.primary {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .btn.primary:hover:not(:disabled) {
    background: var(--accent-strong);
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .link {
    color: var(--accent-strong);
    font-weight: 500;
  }
  .link:disabled {
    opacity: 0.5;
  }
  .prefs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    margin-top: 14px;
  }
  .prefs label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: var(--text-secondary);
  }
  select,
  input {
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--separator-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-editor);
    color: var(--text);
  }
  .backups {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .backups li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
  }
  .backups li + li {
    border-top: 1px solid var(--separator);
  }
  details {
    padding-top: 12px;
  }
  summary {
    cursor: pointer;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
  }
  details[open] summary {
    margin-bottom: 10px;
  }
  details input {
    display: block;
    width: 100%;
    margin-bottom: 8px;
  }
  code {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  :global(.spin) {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
