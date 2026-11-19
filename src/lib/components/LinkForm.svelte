<script lang="ts">
  // The small box for adding or editing a link (Ctrl+K), shown under the text.
  import { onMount, untrack } from "svelte";
  import { normalizeUrl } from "../links";

  interface Props {
    x: number;
    y: number;
    /** Current address when editing an existing link. */
    href: string;
    /** Ask for the link text too (nothing was selected). */
    withText: boolean;
    editing: boolean;
    onsave: (href: string, text: string) => void;
    onremove: () => void;
    oncancel: () => void;
  }
  let { x, y, href, withText, editing, onsave, onremove, oncancel }: Props = $props();

  // The box is created fresh each time it opens, so it starts from the current address once.
  let address = $state(untrack(() => href));
  let label = $state("");
  let error = $state<string | null>(null);
  let addressInput = $state<HTMLInputElement>();
  let labelInput = $state<HTMLInputElement>();

  onMount(() => {
    const first = withText ? labelInput : addressInput;
    first?.focus();
    first?.select();
  });

  function save() {
    const normalized = normalizeUrl(address);
    if (!normalized) {
      error = "Enter a web address (like example.com) or an email address.";
      addressInput?.focus();
      return;
    }
    onsave(normalized, label.trim());
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      oncancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="link-form" role="dialog" tabindex="-1" aria-label={editing ? "Edit link" : "Add link"} style:left="{x}px" style:top="{y}px" {onkeydown}>
  {#if withText}
    <label>
      <span>Text</span>
      <input bind:this={labelInput} bind:value={label} placeholder="Optional" spellcheck="false" />
    </label>
  {/if}
  <label>
    <span>Link</span>
    <input
      bind:this={addressInput}
      bind:value={address}
      placeholder="example.com or name@example.com"
      spellcheck="false"
      aria-invalid={!!error}
      oninput={() => (error = null)}
    />
  </label>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="actions">
    {#if editing}<button class="btn danger" onclick={onremove}>Remove Link</button>{/if}
    <span class="grow"></span>
    <button class="btn" onclick={oncancel}>Cancel</button>
    <button class="btn primary" onclick={save}>{editing ? "Save" : "Add Link"}</button>
  </div>
</div>

<style>
  .link-form {
    position: absolute;
    z-index: 30;
    width: 340px;
    padding: 12px;
    border-radius: var(--radius-lg);
    background: var(--bg-list);
    box-shadow: var(--shadow-pop);
    font-size: 13px;
    line-height: 1.4;
    user-select: none;
  }
  label {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  label span {
    width: 34px;
    color: var(--text-secondary);
    font-weight: 500;
  }
  input {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 8px;
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
  input[aria-invalid="true"] {
    border-color: var(--danger);
  }
  .error {
    margin: -2px 0 8px 42px;
    color: var(--danger);
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .grow {
    flex: 1;
  }
  .btn {
    height: 28px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    font-weight: 600;
  }
  .btn:hover {
    background: var(--bg-active);
  }
  .btn.primary {
    background: var(--accent);
    color: var(--accent-contrast);
  }
  .btn.danger {
    color: var(--danger);
  }
</style>
