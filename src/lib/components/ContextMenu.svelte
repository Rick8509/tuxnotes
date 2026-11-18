<script lang="ts" module>
  export type MenuItem =
    | { kind?: "item"; label: string; action: () => void; danger?: boolean; disabled?: boolean }
    | { kind: "separator" }
    | { kind: "heading"; label: string };
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onclose: () => void;
  }
  let { x, y, items, onclose }: Props = $props();

  let menu: HTMLDivElement;
  let pos = $state({ left: 0, top: 0 });

  onMount(async () => {
    pos = { left: x, top: y };
    await tick();
    // Keep the menu inside the window.
    const r = menu.getBoundingClientRect();
    pos = {
      left: Math.min(x, window.innerWidth - r.width - 6),
      top: Math.min(y, window.innerHeight - r.height - 6),
    };
    menu.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  });

  function run(action: () => void) {
    onclose();
    action();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const buttons = [...menu.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    const idx = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.key === "ArrowDown" ? idx + 1 : idx - 1;
    buttons[(next + buttons.length) % buttons.length]?.focus();
  }
</script>

<svelte:window onblur={onclose} onresize={onclose} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onmousedown={onclose}
  oncontextmenu={(e) => {
    e.preventDefault();
    onclose();
  }}
></div>
<div
  class="menu"
  role="menu"
  tabindex="-1"
  bind:this={menu}
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  {onkeydown}
>
  {#each items as item, i (i)}
    {#if item.kind === "separator"}
      <div class="sep" role="separator"></div>
    {:else if item.kind === "heading"}
      <div class="heading">{item.label}</div>
    {:else}
      <button
        role="menuitem"
        class:danger={item.danger}
        disabled={item.disabled}
        onclick={() => run(item.action)}>{item.label}</button
      >
    {/if}
  {/each}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
  }
  .menu {
    position: fixed;
    z-index: 91;
    min-width: 190px;
    max-height: 70vh;
    overflow-y: auto;
    padding: 5px;
    background: var(--bg-list);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  button {
    display: block;
    width: 100%;
    text-align: left;
    padding: 5px 10px;
    border-radius: 5px;
  }
  button:hover:not(:disabled),
  button:focus-visible {
    background: var(--accent);
    color: var(--accent-contrast);
    outline: none;
  }
  button:disabled {
    color: var(--text-tertiary);
  }
  .danger {
    color: var(--danger);
  }
  .sep {
    height: 1px;
    margin: 5px 6px;
    background: var(--separator);
  }
  .heading {
    padding: 5px 10px 2px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }
</style>
