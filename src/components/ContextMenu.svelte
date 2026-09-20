<script lang="ts">
  // Minimal context menu: caller owns open state and item list.
  let {
    x,
    y,
    items,
    onclose,
  }: {
    x: number;
    y: number;
    items: { label: string; danger?: boolean; disabled?: boolean; act: () => void }[];
    onclose: () => void;
  } = $props();

  let menuEl: HTMLDivElement;

  function run(item: (typeof items)[number]) {
    if (item.disabled) return;
    onclose();
    item.act();
  }

  function clamp(_el: HTMLDivElement) {
    // Keep the menu inside the window after mount.
    const el = menuEl;
    if (!el) return;
    const r = el.getBoundingClientRect();
    if (r.right > window.innerWidth) el.style.left = `${window.innerWidth - r.width - 8}px`;
    if (r.bottom > window.innerHeight) el.style.top = `${window.innerHeight - r.height - 8}px`;
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div
 class="backdrop"
 onmousedown={(e) => e.target === e.currentTarget && onclose()}
 oncontextmenu={(e) => { e.preventDefault(); onclose(); }}
></div>
<div class="menu" bind:this={menuEl} style:left="{x}px" style:top="{y}px" use:clamp>
  {#each items as item (item.label)}
    <button class="item" class:danger={item.danger} disabled={item.disabled} onclick={() => run(item)}>
      {item.label}
    </button>
  {/each}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 900;
  }
  .menu {
    position: fixed;
    z-index: 901;
    min-width: 180px;
    background: var(--bg-pane);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
    padding: 4px;
    display: flex;
    flex-direction: column;
  }
  .item {
    border: none;
    background: transparent;
    text-align: left;
    font: inherit;
    font-size: 12px;
    padding: 6px 10px;
    border-radius: 5px;
    cursor: pointer;
    color: var(--text-1);
  }
  .item:hover:not(:disabled) { background: var(--bg-hover); }
  .item.danger { color: var(--danger); }
  .item:disabled { opacity: 0.4; cursor: default; }
</style>
