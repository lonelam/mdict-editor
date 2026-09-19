<script lang="ts">
  import { store } from "../lib/store.svelte";
  import { idKey } from "../lib/types";

  const tab = $derived(store.activeTab);
  const canBack = $derived(store.jump.pos > 0);
  const canFwd = $derived(store.jump.pos < store.jump.stack.length - 1);
</script>

<footer class="statusbar">
  <span class="cell">
    {tab ? `${tab.meta.sourceName} · ${tab.meta.key}` : "就绪"}
  </span>
  <span class="cell" class:edited={tab?.meta.edited}>
    {tab?.meta.edited ? "已编辑" : "未修改"}
  </span>
  <span class="cell grow"></span>
  <span class="cell">{store.selection.length > 0 ? `已选 ${store.selection.length}` : ""}</span>
  <button
    class="cell nav"
    disabled={!canBack}
    title="跳转后退 (Alt+←)"
    onclick={() => store.jumpBack()}>←</button>
  <button
    class="cell nav"
    disabled={!canFwd}
    title="跳转前进 (Alt+→)"
    onclick={() => store.jumpForward()}>→</button>
  <span class="cell dim">{tab ? idKey(tab.meta.id) : `源 ${store.sources.length}`}</span>
</footer>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    height: 26px;
    border-top: 1px solid var(--border-subtle);
    background: var(--bg-pane);
    font-size: 11px;
    color: var(--text-2);
    overflow: hidden;
  }
  .cell {
    padding: 0 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    border: none;
    background: transparent;
  }
  .grow { flex: 1; }
  .edited { color: #e08a00; }
  .dim { color: var(--text-3); }
  .nav {
    cursor: pointer;
    color: var(--text-1);
    font-size: 12px;
  }
  .nav:disabled { opacity: 0.35; cursor: default; }
</style>
