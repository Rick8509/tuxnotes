<script lang="ts">
  // A draggable divider on the right edge of a pane. Double-click resets widths.
  import { ui } from "../stores/ui.svelte";

  interface Props {
    /** Current width of the pane being resized. */
    width: number;
    onresize: (px: number) => void;
    label: string;
  }
  let { width, onresize, label }: Props = $props();

  let startX = 0;
  let startWidth = 0;

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    startX = e.clientX;
    startWidth = width;
    ui.resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (!ui.resizing || !(e.currentTarget as HTMLElement).hasPointerCapture(e.pointerId)) return;
    onresize(startWidth + e.clientX - startX);
  }

  function onpointerup(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    if (ui.resizing) ui.endResize();
  }

  function onkeydown(e: KeyboardEvent) {
    const step = e.shiftKey ? 40 : 10;
    if (e.key === "ArrowLeft") onresize(width - step);
    else if (e.key === "ArrowRight") onresize(width + step);
    else return;
    e.preventDefault();
    ui.endResize();
  }
</script>

<!-- A focusable separator is the ARIA "window splitter" pattern; it handles arrow keys. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="resizer"
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={width}
  tabindex="0"
  title="Drag to resize · double-click to reset"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
  ondblclick={() => ui.resetWidths()}
  {onkeydown}
></div>

<style>
  .resizer {
    position: absolute;
    top: 0;
    right: -4px;
    bottom: 0;
    width: 8px;
    z-index: 20;
    cursor: col-resize;
    touch-action: none;
  }
  .resizer::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    background: transparent;
    transition: background 0.15s;
  }
  .resizer:hover::after,
  .resizer:focus-visible::after {
    background: var(--accent);
    transition-delay: 0.15s;
  }
  .resizer:focus-visible {
    outline: none;
  }
</style>
