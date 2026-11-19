<script lang="ts">
  // The password window for locked notes: set the first password, enter it, or change it.
  import { onMount, untrack } from "svelte";
  import { LockKeyhole } from "@lucide/svelte";
  import { api, errorText } from "../api";
  import { app, type LockPrompt } from "../stores/app.svelte";
  import type { LockStatus } from "../types";

  let { prompt }: { prompt: LockPrompt } = $props();

  const MIN_CHARS = 4;
  let current = $state("");
  let password = $state("");
  let verify = $state("");
  // The window is created per request, so reading the mode once is right.
  let hint = $state(untrack(() => (prompt.mode === "change" ? app.lock.hint : "")));
  let error = $state("");
  let busy = $state(false);
  let showHint = $state(false);
  let first = $state<HTMLInputElement>();

  onMount(() => first?.focus());

  const titles = {
    set: "Set a Password for Locked Notes",
    unlock: "Enter the Password for Locked Notes",
    change: "Change the Password for Locked Notes",
  } as const;
  const actions = { set: "Set Password", unlock: "Unlock", change: "Change Password" } as const;

  /** Problems that can be spotted before asking TuxNotes (it checks again). */
  function check(): string {
    if (prompt.mode === "unlock") return current ? "" : "Enter the password.";
    if (prompt.mode === "change" && !current) return "Enter the current password.";
    if ([...password].length < MIN_CHARS) return `Use a password of at least ${MIN_CHARS} characters.`;
    if (password !== verify) return "The passwords don't match.";
    if (hint.trim() && hint.toLowerCase().includes(password.toLowerCase())) return "The hint can't contain the password.";
    return "";
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    error = check();
    if (error) return;
    busy = true;
    try {
      let status: LockStatus;
      if (prompt.mode === "set") status = await api.setLockPassword(password, hint);
      else if (prompt.mode === "unlock") status = await api.unlockNotes(current);
      else status = await api.changeLockPassword(current, password, hint);
      await app.lockStatusChanged(status);
      if (prompt.mode === "change") app.showInfo("Password changed");
      prompt.resolve(true);
    } catch (err) {
      error = errorText(err);
      busy = false;
      if (prompt.mode !== "set") {
        current = "";
        first?.focus();
      }
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !busy) {
      e.preventDefault();
      prompt.resolve(false);
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="scrim">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="lock-title">
    <form onsubmit={submit}>
      <div class="icon"><LockKeyhole size={22} /></div>
      <h2 id="lock-title">{titles[prompt.mode]}</h2>

      {#if prompt.mode === "set"}
        <p>
          This password locks all your locked notes. <strong>If you forget it, TuxNotes can't open them</strong>: there is no
          way to recover it.
        </p>
      {/if}

      {#if prompt.mode !== "set"}
        <input
          bind:this={first}
          bind:value={current}
          type="password"
          autocomplete="current-password"
          placeholder={prompt.mode === "change" ? "Current password" : "Password"}
          aria-label={prompt.mode === "change" ? "Current password" : "Password"}
          disabled={busy}
        />
      {/if}
      {#if prompt.mode !== "unlock"}
        {#if prompt.mode === "set"}
          <input
            bind:this={first}
            bind:value={password}
            type="password"
            autocomplete="new-password"
            placeholder="Password"
            aria-label="New password"
            disabled={busy}
          />
        {:else}
          <input bind:value={password} type="password" autocomplete="new-password" placeholder="New password" aria-label="New password" disabled={busy} />
        {/if}
        <input bind:value={verify} type="password" autocomplete="new-password" placeholder="Verify" aria-label="Verify password" disabled={busy} />
        <input bind:value={hint} maxlength="200" placeholder="Hint (optional)" aria-label="Password hint" disabled={busy} />
      {/if}

      {#if prompt.mode === "unlock" && app.lock.hint}
        {#if showHint}
          <p class="hint-text">Hint: {app.lock.hint}</p>
        {:else}
          <button type="button" class="link" onclick={() => (showHint = true)}>Show Hint</button>
        {/if}
      {/if}

      {#if prompt.mode === "set"}
        <p class="small">A locked note's title (its first line) stays visible in the list. Images and files in it aren't encrypted.</p>
      {/if}

      <p class="error" role="alert">{error}</p>

      <div class="actions">
        <button type="button" class="btn" disabled={busy} onclick={() => prompt.resolve(false)}>Cancel</button>
        <button type="submit" class="btn primary" disabled={busy}>{busy ? "Checking…" : actions[prompt.mode]}</button>
      </div>
    </form>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.25);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .dialog {
    width: 360px;
    padding: 20px;
    border-radius: var(--radius-lg);
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    text-align: center;
  }
  .icon {
    display: grid;
    place-items: center;
    color: var(--accent-strong);
  }
  h2 {
    margin: 0 0 2px;
    font-size: 14px;
    font-weight: 700;
  }
  p {
    margin: 0 0 4px;
    color: var(--text-secondary);
  }
  p strong {
    color: var(--text);
  }
  .small {
    font-size: 12px;
    color: var(--text-tertiary);
  }
  .hint-text {
    margin: 0;
  }
  .error {
    min-height: 1.3em;
    margin: 0;
    color: var(--danger);
    font-size: 12px;
  }
  input {
    width: 100%;
    padding: 7px 10px;
    border: 1px solid var(--separator-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-editor);
    color: var(--text);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .link {
    align-self: center;
    color: var(--accent-strong);
    font-size: 12px;
  }
  .link:hover {
    text-decoration: underline;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .btn {
    flex: 1;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    font-weight: 500;
  }
  .btn:hover:not(:disabled) {
    background: var(--bg-active);
  }
  .btn:disabled {
    opacity: 0.6;
  }
  .primary {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-strong);
  }
</style>
