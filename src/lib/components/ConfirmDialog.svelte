<script lang="ts">
  import { app } from "../stores/app.svelte";

  let confirmButton = $state<HTMLButtonElement>();

  $effect(() => {
    if (app.confirmRequest) confirmButton?.focus();
  });

  function onkeydown(e: KeyboardEvent) {
    if (!app.confirmRequest) return;
    if (e.key === "Escape") {
      e.preventDefault();
      app.confirmRequest.resolve(false);
    }
  }
</script>

<svelte:window {onkeydown} />

{#if app.confirmRequest}
  {@const req = app.confirmRequest}
  <div class="scrim">
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title">
      <h2 id="confirm-title">{req.title}</h2>
      <p>{req.message}</p>
      <div class="actions">
        <button class="btn" onclick={() => req.resolve(false)}>Cancel</button>
        <button
          class="btn primary"
          class:destructive={req.destructive}
          bind:this={confirmButton}
          onclick={() => req.resolve(true)}>{req.confirmLabel}</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.25);
  }
  .dialog {
    width: 340px;
    padding: 20px;
    border-radius: var(--radius-lg);
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    text-align: center;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 14px;
    font-weight: 700;
  }
  p {
    margin: 0 0 18px;
    color: var(--text-secondary);
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
  .btn:hover {
    background: var(--bg-active);
  }
  .primary {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .primary:hover {
    background: var(--accent-strong);
  }
  .destructive {
    background: var(--danger);
    color: #fff;
  }
  .destructive:hover {
    background: var(--danger);
    filter: brightness(1.08);
  }
</style>
