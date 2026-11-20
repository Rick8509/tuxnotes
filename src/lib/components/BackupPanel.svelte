<script lang="ts" module>
  export type BackupView = "home" | "waiting" | "working" | "restore" | "password" | "restorePassword";
</script>

<script lang="ts">
  // Settings → Backup. Google Drive backup in the style people know from other apps:
  // two clear actions, a "we'll open your browser" prompt, then TuxNotes
  // carries on by itself once you've signed in.
  import { onDestroy, onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    ArrowLeft,
    CircleAlert,
    CircleCheck,
    CloudUpload,
    HardDriveDownload,
    LoaderCircle,
    Lock,
    LockKeyhole,
  } from "@lucide/svelte";
  import { api, errorText, isTauri } from "../api";
  import { formatBytes, formatRelativeDateTime } from "../format";
  import { app } from "../stores/app.svelte";
  import { BACKUP_PASSWORD_NEEDED, type AutoBackup, type BackupInfo, type BackupStatus } from "../types";

  type Intent = "backup" | "restore";

  // The Settings window reads `view` to handle Escape and to block closing mid-backup.
  let { view = $bindable("home") }: { view?: BackupView } = $props();

  let status = $state<BackupStatus | null>(null);
  let intent = $state<Intent>("backup");
  let workingLabel = $state("");
  let signInUrl = $state<string | null>(null);
  let copied = $state(false);
  let backups = $state<BackupInfo[] | null>(null);
  let listError = $state<string | null>(null);
  let selectedId = $state<string | null>(null);

  // Backup password (encrypting) and the password for restoring an encrypted backup.
  const MIN_BACKUP_PASSWORD = 8;
  let pw = $state("");
  let pw2 = $state("");
  let pwError = $state("");
  let pwBusy = $state(false);
  let restoreTarget = $state<BackupInfo | null>(null);

  const available = $derived(!!status && status.clientSource !== "none");
  const message = errorText;
  const details = (b: BackupInfo) =>
    [
      b.encrypted && "Encrypted",
      b.noteCount != null && `${b.noteCount} note${b.noteCount === 1 ? "" : "s"}`,
      b.size != null && formatBytes(b.size),
    ]
      .filter(Boolean)
      .join(" · ");
  const isCancel = (e: unknown) => message(e).includes("Sign-in cancelled");

  async function refreshStatus() {
    try {
      status = await api.backupStatus();
    } catch (e) {
      app.showError(e);
    }
  }

  onMount(() => {
    view = "home";
    void refreshStatus();
    const unlisteners: Promise<() => void>[] = [];
    if (isTauri) {
      unlisteners.push(listen<string>("google-sign-in-url", (e) => (signInUrl = e.payload)));
      // Automatic backups run in the background; keep this window in sync.
      unlisteners.push(listen("backup-status-changed", () => void refreshStatus()));
    }
    return () => unlisteners.forEach((p) => p.then((fn) => fn()));
  });

  // ------------------------------------------------------------- main flow

  /** Back up / Restore: sign in first if needed (asking before opening the browser), then continue. */
  async function start(which: Intent) {
    if (!status) return;
    intent = which;
    if (!status.signedIn) {
      const ok = await app.confirm({
        title: "Sign in with Google?",
        message:
          "TuxNotes will open your web browser so you can sign in and allow access to Google Drive. " +
          "TuxNotes can only see the backups it creates.",
        confirmLabel: "Open Browser",
      });
      if (!ok) return;
      signInUrl = null;
      copied = false;
      view = "waiting";
      try {
        status = await api.googleSignIn();
      } catch (e) {
        view = "home";
        if (!isCancel(e)) app.showError(e);
        await refreshStatus();
        return;
      }
    }
    if (which === "backup") await backUp(true);
    else await openRestore();
  }

  async function backUp(firstTime = false) {
    await app.flush(); // include the latest edits
    view = "working";
    workingLabel = "Backing up your notes…";
    try {
      const info = await api.backupNow();
      await refreshStatus();
      const notes = `${info.noteCount ?? 0} note${info.noteCount === 1 ? "" : "s"}`;
      const auto = status?.autoBackup !== "off" ? ` TuxNotes will also back up ${status?.autoBackup} from now on.` : "";
      app.showInfo(firstTime ? `Backed up ${notes} to Google Drive.${auto}` : `Backed up ${notes} to Google Drive`);
    } catch (e) {
      app.showError(e);
      await refreshStatus();
    }
    view = "home";
  }

  async function openRestore() {
    view = "restore";
    backups = null;
    listError = null;
    try {
      backups = await api.listBackups();
      selectedId = backups[0]?.id ?? null;
    } catch (e) {
      listError = message(e);
    }
    await refreshStatus();
  }

  async function restoreSelected() {
    const b = backups?.find((x) => x.id === selectedId);
    if (!b) return;
    const when = b.createdAt ? formatRelativeDateTime(b.createdAt) : b.name;
    const ok = await app.confirm({
      title: "Replace your notes with this backup?",
      message:
        `All notes on this computer will be replaced with the backup from ${when}` +
        (b.noteCount != null ? ` (${b.noteCount} notes)` : "") +
        ". A copy of your current notes is kept on this computer, just in case.",
      confirmLabel: "Restore",
      destructive: true,
    });
    if (!ok) return;
    if (b.needsPassword) askRestorePassword(b);
    else await doRestore(b);
  }

  function askRestorePassword(b: BackupInfo, error = "") {
    restoreTarget = b;
    pw = "";
    pwError = error;
    view = "restorePassword";
  }

  async function doRestore(b: BackupInfo, password?: string) {
    await app.flush();
    view = "working";
    workingLabel = "Restoring your notes…";
    try {
      const result = await api.restoreBackup(b.id, password);
      await app.reloadAll();
      app.showInfo(`Restored ${result.noteCount} notes from Google Drive`);
      app.settingsSection = null;
    } catch (e) {
      // Encrypted and this computer can't open it, or the typed password was wrong: ask (again).
      if (message(e) === BACKUP_PASSWORD_NEEDED) askRestorePassword(b);
      else if (password !== undefined) askRestorePassword(b, message(e));
      else {
        app.showError(e);
        view = "restore";
      }
    } finally {
      pw = "";
    }
  }

  async function submitRestorePassword(e: SubmitEvent) {
    e.preventDefault();
    if (pw && restoreTarget) await doRestore(restoreTarget, pw);
  }

  // ------------------------------------------------------------ encryption

  function openPassword() {
    pw = "";
    pw2 = "";
    pwError = "";
    view = "password";
  }

  async function savePassword(e: SubmitEvent) {
    e.preventDefault();
    if (pwBusy || !status) return;
    if ([...pw].length < MIN_BACKUP_PASSWORD) pwError = `Use at least ${MIN_BACKUP_PASSWORD} characters.`;
    else if (pw !== pw2) pwError = "The passwords don't match.";
    else pwError = "";
    if (pwError) return;
    const changing = status.encrypted;
    pwBusy = true;
    try {
      status = await api.setBackupPassword(pw);
      app.showInfo(changing ? "Backup password changed" : "New backups will be encrypted");
      view = "home";
    } catch (err) {
      pwError = message(err);
    } finally {
      pwBusy = false;
      pw = "";
      pw2 = "";
    }
  }

  async function turnOffEncryption() {
    const ok = await app.confirm({
      title: "Stop encrypting backups?",
      message:
        "New backups will be uploaded without a password. Backups already on Google Drive still need their password to be restored.",
      confirmLabel: "Turn Off",
    });
    if (!ok) return;
    try {
      status = await api.setBackupPassword(null);
    } catch (e) {
      app.showError(e);
    }
  }

  function focus(node: HTMLInputElement) {
    node.focus();
  }

  // ------------------------------------------------------------ the rest

  async function cancelSignIn() {
    await api.googleCancelSignIn();
  }

  async function openAgain() {
    if (!signInUrl) return;
    try {
      if (isTauri) await openUrl(signInUrl);
      else window.open(signInUrl, "_blank", "noopener");
    } catch (e) {
      app.showError(`Couldn't open the browser: ${message(e)}`);
    }
  }

  async function copyLink() {
    if (!signInUrl) return;
    try {
      await navigator.clipboard.writeText(signInUrl);
      copied = true;
      setTimeout(() => (copied = false), 2500);
    } catch {
      app.showError("Couldn't copy the link.");
    }
  }

  async function disconnect() {
    const ok = await app.confirm({
      title: "Disconnect Google Drive?",
      message: "Your backups stay in Google Drive. Automatic backups stop until you connect again.",
      confirmLabel: "Disconnect",
    });
    if (!ok) return;
    try {
      status = await api.googleSignOut();
    } catch (e) {
      app.showError(e);
    }
  }

  async function setPreferences(autoBackup: AutoBackup, keepBackups: number, backupOnClose = status?.backupOnClose ?? true) {
    try {
      status = await api.setBackupPreferences(autoBackup, keepBackups, backupOnClose);
    } catch (e) {
      app.showError(e);
    }
  }

  // Leaving the Backup section (or closing Settings) while waiting for Google cancels the sign-in.
  onDestroy(() => {
    if (view === "waiting") void api.googleCancelSignIn();
  });
</script>

<div class="panel">
    <header>
      {#if view === "restore" || view === "restorePassword"}
        <button class="icon-btn back" aria-label="Back" onclick={() => (view = view === "restore" ? "home" : "restore")}><ArrowLeft size={16} /></button>
        <h2 id="backup-title">Restore from Google Drive</h2>
      {:else if view === "password"}
        <button class="icon-btn back" aria-label="Back" onclick={() => (view = "home")}><ArrowLeft size={16} /></button>
        <h2 id="backup-title">Encrypt Backups</h2>
      {:else}
        <h2 id="backup-title">Google Drive Backup</h2>
      {/if}
    </header>

    {#if !status}
      <div class="center"><LoaderCircle size={22} class="spin" /></div>

    <!-- ------------------------------------------------ waiting for Google -->
    {:else if view === "waiting"}
      <div class="hero">
        <div class="badge"><LoaderCircle size={28} class="spin" /></div>
        <h3>Continue in your browser</h3>
        <p>
          Sign in to Google and allow TuxNotes to use Google Drive. Then come back here: TuxNotes will
          {intent === "backup" ? "back up your notes" : "find your backups"} automatically.
        </p>
        <button class="btn wide" onclick={cancelSignIn}>Cancel</button>
        {#if signInUrl}
          <p class="fallback">
            Browser didn't open?
            <button class="link" onclick={openAgain}>Open it again</button>
            ·
            <button class="link" onclick={copyLink}>{copied ? "Link copied" : "Copy link"}</button>
          </p>
        {/if}
      </div>

    <!-- ------------------------------------------------ busy -->
    {:else if view === "working"}
      <div class="hero">
        <div class="badge"><LoaderCircle size={28} class="spin" /></div>
        <h3>{workingLabel}</h3>
        <p>This usually takes a few seconds.</p>
      </div>

    <!-- ------------------------------------------------ choose a backup -->
    {:else if view === "restore"}
      {#if listError}
        <p class="error-box"><CircleAlert size={16} /> {listError}</p>
        <div class="actions"><button class="btn" onclick={openRestore}>Try Again</button></div>
      {:else if !backups}
        <div class="center"><LoaderCircle size={22} class="spin" /><span>Looking for your backups…</span></div>
      {:else if backups.length === 0}
        <div class="hero">
          <div class="badge muted"><HardDriveDownload size={26} /></div>
          <h3>No backups yet</h3>
          <p>There are no TuxNotes backups in {status.account ?? "this Google account"}'s Google Drive.</p>
          <button class="btn primary wide" onclick={() => backUp(true)}><CloudUpload size={16} /> Back Up Now</button>
        </div>
      {:else}
        <p class="muted intro">Choose a backup. It replaces the notes on this computer.</p>
        <div class="backups" role="radiogroup" aria-label="Backups">
          {#each backups as b, i (b.id)}
            <label class="backup" class:selected={selectedId === b.id}>
              <input type="radio" name="backup" value={b.id} bind:group={selectedId} />
              <span class="grow">
                <span class="when">
                  {b.createdAt ? formatRelativeDateTime(b.createdAt) : b.name}
                  {#if i === 0}<span class="tag">Latest</span>{/if}
                </span>
                <span class="muted small">{details(b)}</span>
              </span>
            </label>
          {/each}
        </div>
        <div class="actions">
          <button class="btn" onclick={() => (view = "home")}>Cancel</button>
          <button class="btn primary" disabled={!selectedId} onclick={restoreSelected}>Restore</button>
        </div>
      {/if}

    <!-- ------------------------------------------------ backup password -->
    {:else if view === "password"}
      <form class="pw-form" onsubmit={savePassword}>
        <div class="badge small-badge"><LockKeyhole size={20} /></div>
        <p>
          Backups are encrypted on this computer before they're uploaded, so only someone with the password can read
          them. TuxNotes remembers it here, so automatic backups keep working.
        </p>
        <p class="strong">If you forget the password, encrypted backups can't be restored.</p>
        <input type="password" autocomplete="new-password" placeholder={status.encrypted ? "New backup password" : "Backup password"} aria-label="Backup password" bind:value={pw} disabled={pwBusy} use:focus />
        <input type="password" autocomplete="new-password" placeholder="Verify" aria-label="Verify backup password" bind:value={pw2} disabled={pwBusy} />
        <p class="error pw-error" role="alert">{pwError}</p>
        {#if status.encrypted}
          <p class="small muted">Backups already on Google Drive keep the password they were made with.</p>
        {/if}
        <div class="actions">
          <button type="button" class="btn" disabled={pwBusy} onclick={() => (view = "home")}>Cancel</button>
          <button type="submit" class="btn primary" disabled={pwBusy}>
            {pwBusy ? "Saving…" : status.encrypted ? "Change Password" : "Encrypt Backups"}
          </button>
        </div>
      </form>

    <!-- ------------------------------------------------ password for an encrypted backup -->
    {:else if view === "restorePassword"}
      <form class="pw-form" onsubmit={submitRestorePassword}>
        <div class="badge small-badge"><LockKeyhole size={20} /></div>
        <p>This backup is encrypted. Enter the password it was made with.</p>
        <input type="password" autocomplete="current-password" placeholder="Backup password" aria-label="Password for this backup" bind:value={pw} use:focus />
        <p class="error pw-error" role="alert">{pwError}</p>
        <div class="actions">
          <button type="button" class="btn" onclick={() => (view = "restore")}>Back</button>
          <button type="submit" class="btn primary" disabled={!pw}>Restore</button>
        </div>
      </form>

    <!-- ------------------------------------------------ not available in this build -->
    {:else if !available}
      <div class="hero">
        <div class="badge muted"><CircleAlert size={26} /></div>
        <h3>Google Drive backup isn't available</h3>
        <p>This copy of TuxNotes was built without Google sign-in. Official releases include it.</p>
        <p class="small muted">
          Built it yourself? Save your Google client file as
          <code>~/.config/io.github.tuxnotes.TuxNotes/google-client.json</code> and reopen TuxNotes
          (see <code>docs/google-setup.md</code>).
        </p>
      </div>

    <!-- ------------------------------------------------ not connected -->
    {:else if !status.signedIn}
      <div class="hero">
        <div class="badge"><CloudUpload size={28} /></div>
        <h3>Keep your notes safe</h3>
        {#if status.lastBackupError}
          <!-- e.g. the Google sign-in expired: backups stopped until you connect again -->
          <p class="error-box"><CircleAlert size={16} /> {status.lastBackupError}</p>
        {/if}
        <p>Back up your notes to your own Google Drive and restore them on any computer.</p>
        <button class="btn primary wide" onclick={() => start("backup")}>
          <CloudUpload size={16} /> Back Up to Google Drive
        </button>
        <button class="btn wide" onclick={() => start("restore")}>
          <HardDriveDownload size={16} /> Restore from Google Drive
        </button>
        <p class="privacy"><Lock size={12} /> TuxNotes can only see the backups it creates.</p>
      </div>

    <!-- ------------------------------------------------ connected -->
    {:else}
      <div class="card">
        <div class="account">
          <span class="avatar">{(status.account ?? "G").charAt(0).toUpperCase()}</span>
          <span class="grow">
            <span class="email">{status.account ?? "Google account"}</span>
            <span class="muted small">Connected to Google Drive</span>
          </span>
          <button class="link" onclick={disconnect}>Disconnect</button>
        </div>
        <div class="last">
          {#if status.lastBackupError}
            <CircleAlert size={15} class="warn" />
            <span class="error">Last backup failed: {status.lastBackupError}</span>
          {:else if status.lastBackupAt}
            <CircleCheck size={15} class="ok" />
            <span>Last backup: {formatRelativeDateTime(status.lastBackupAt)}</span>
          {:else}
            <span class="muted">No backups yet</span>
          {/if}
        </div>
      </div>

      <div class="actions two">
        <button class="btn primary" onclick={() => backUp()}><CloudUpload size={16} /> Back Up Now</button>
        <button class="btn" onclick={openRestore}><HardDriveDownload size={16} /> Restore…</button>
      </div>

      <div class="prefs">
        <label>
          <span>Back up automatically</span>
          <select value={status.autoBackup} onchange={(e) => setPreferences(e.currentTarget.value as AutoBackup, status!.keepBackups)}>
            <option value="daily">Every day</option>
            <option value="weekly">Every week</option>
            <option value="off">Never</option>
          </select>
        </label>
        <label>
          <span>Keep on Google Drive</span>
          <select value={String(status.keepBackups)} onchange={(e) => setPreferences(status!.autoBackup, Number(e.currentTarget.value))}>
            {#each [5, 10, 20, 50] as n (n)}
              <option value={String(n)}>Last {n} backups</option>
            {/each}
            {#if ![5, 10, 20, 50].includes(status.keepBackups)}
              <option value={String(status.keepBackups)}>Last {status.keepBackups} backups</option>
            {/if}
          </select>
        </label>
        <label>
          <span>Back up when closing TuxNotes</span>
          <input
            type="checkbox"
            class="switch"
            checked={status.backupOnClose}
            onchange={(e) => setPreferences(status!.autoBackup, status!.keepBackups, e.currentTarget.checked)}
          />
        </label>
        <div class="pref-row">
          <span>Encrypt backups <span class="muted">· {status.encrypted ? "On" : "Off"}</span></span>
          {#if status.encrypted}
            <span class="pref-actions">
              <button class="link" onclick={openPassword}>Change Password…</button>
              <button class="link" onclick={turnOffEncryption}>Turn Off</button>
            </span>
          {:else}
            <button class="btn" onclick={openPassword}>Set Password…</button>
          {/if}
        </div>
      </div>
      <p class="privacy left">
        <Lock size={12} /> Saved in the “TuxNotes Backups” folder in your Google Drive{status.encrypted
          ? ", encrypted with your backup password"
          : ""}.
      </p>
    {/if}
</div>

<style>
  .panel {
    max-width: 440px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 8px;
  }
  header h2 {
    flex: 1;
    margin: 0;
    font-size: 18px;
    font-weight: 700;
  }
  .back {
    margin-left: -8px;
  }

  /* Centered illustration + text + stacked buttons. */
  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 10px;
    padding: 12px 4px 4px;
  }
  .hero h3 {
    margin: 4px 0 0;
    font-size: 17px;
    font-weight: 700;
  }
  .hero p {
    margin: 0 0 6px;
    color: var(--text-secondary);
    line-height: 1.5;
  }
  .badge {
    display: grid;
    place-items: center;
    width: 60px;
    height: 60px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent-strong);
  }
  .badge.muted {
    background: var(--bg-input);
    color: var(--text-secondary);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 34px;
    padding: 0 14px;
    border-radius: var(--radius-md);
    background: var(--bg-input);
    font-weight: 600;
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
  .btn.wide {
    width: 100%;
  }
  .link {
    color: var(--accent-strong);
    font-weight: 600;
  }
  .link:hover {
    text-decoration: underline;
  }

  .privacy {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--text-tertiary);
  }
  .privacy.left {
    justify-content: flex-start;
    margin-top: 14px;
  }
  .fallback {
    font-size: 12px;
  }

  /* Connected state */
  .card {
    border-radius: var(--radius-md);
    background: var(--bg-input);
    padding: 12px;
    margin: 6px 0 14px;
  }
  .account {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .avatar {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 700;
  }
  .email {
    display: block;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .last {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--separator);
    font-size: 12px;
  }
  .last :global(.ok) {
    color: #34a853;
    flex: none;
  }
  .last :global(.warn) {
    color: var(--danger);
    flex: none;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
  .actions.two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    margin-top: 0;
  }

  .prefs {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 16px;
  }
  .prefs label,
  .pref-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 30px;
  }
  .pref-actions {
    display: flex;
    gap: 12px;
  }

  /* Password forms */
  .pw-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .pw-form p {
    margin: 0;
  }
  .pw-form .strong {
    font-weight: 600;
  }
  .pw-form input {
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--separator-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-editor);
    color: var(--text);
    outline: none;
  }
  .pw-form input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .pw-error {
    min-height: 1.3em;
    font-size: 12px;
  }
  .small-badge {
    width: 40px;
    height: 40px;
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

  /* Restore list */
  .intro {
    margin: 0 0 10px;
  }
  .backups {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 300px;
    overflow-y: auto;
  }
  .backup {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--separator);
    border-radius: var(--radius-md);
  }
  .backup:hover {
    background: var(--bg-hover);
  }
  .backup.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .backup input {
    accent-color: var(--accent);
    margin: 0;
  }
  .when {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }
  .tag {
    padding: 1px 6px;
    border-radius: 10px;
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: 10px;
    font-weight: 700;
  }
  .grow {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
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
  .error-box {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 12px;
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    color: var(--danger);
    margin: 4px 0 0;
  }
  .center {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 28px;
    color: var(--text-secondary);
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
