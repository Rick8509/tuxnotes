<script lang="ts">
  // Shown in place of the editor while a locked note is locked.
  import { LockKeyhole } from "@lucide/svelte";
  import { app } from "../stores/app.svelte";

  let password = $state("");
  let error = $state("");
  let busy = $state(false);
  let showHint = $state(false);
  let input = $state<HTMLInputElement>();

  // Focus the field when a locked note is opened (not while typing elsewhere).
  $effect(() => {
    if (app.current?.id && !document.activeElement?.closest("input, textarea, .menu")) input?.focus();
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !password) return;
    busy = true;
    const failed = await app.unlockNotes(password);
    busy = false;
    password = "";
    error = failed ?? "";
    if (failed) input?.focus();
  }
</script>

<div class="locked">
  <div class="icon"><LockKeyhole size={34} strokeWidth={1.6} /></div>
  <h2>This note is locked.</h2>
  {#if app.lock.passwordSet}
    <p>Enter the password for locked notes to view it.</p>
    <form onsubmit={submit}>
      <input
        bind:this={input}
        bind:value={password}
        type="password"
        autocomplete="current-password"
        placeholder="Password"
        aria-label="Password for locked notes"
        disabled={busy}
      />
      <button type="submit" class="unlock" disabled={busy || !password}>{busy ? "Checking…" : "Unlock"}</button>
    </form>
    <p class="error" role="alert">{error}</p>
    {#if app.lock.hint}
      {#if showHint}
        <p class="hint">Hint: {app.lock.hint}</p>
      {:else}
        <button class="link" onclick={() => (showHint = true)}>Show Hint</button>
      {/if}
    {/if}
  {:else}
    <p>The password for this note isn't set up on this computer.</p>
  {/if}
</div>

<style>
  .locked {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    text-align: center;
  }
  .icon {
    color: var(--text-tertiary);
    margin-bottom: 4px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
  }
  p {
    margin: 0;
    color: var(--text-secondary);
  }
  form {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  input {
    width: 220px;
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
  .unlock {
    padding: 6px 14px;
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 600;
  }
  .unlock:disabled {
    opacity: 0.6;
  }
  .error {
    min-height: 1.3em;
    color: var(--danger);
    font-size: 12px;
  }
  .hint {
    font-size: 12px;
  }
  .link {
    color: var(--accent-strong);
    font-size: 12px;
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
