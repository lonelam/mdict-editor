<script lang="ts">
  import { store } from "../lib/store.svelte";
  import { idKey } from "../lib/types";

  const catIcon: Record<string, string> = {
    entry: "E", html: "<>", css: "#", js: "JS", image: "IMG",
    audio: "♪", video: "▶", font: "F", text: "T", other: "BIN",
  };
  function icon(meta: { category: string }): string {
    return catIcon[meta.category] ?? "?";
  }
</script>

<div class="tabbar">
  {#each store.tabs as tab (tab.key)}
    <div
      class="tab"
      class:active={tab.key === store.activeTabKey}
      role="tab"
      tabindex="0"
      onclick={() => (store.activeTabKey = tab.key)}
      onauxclick={(e) => {
        if (e.button === 1) store.closeTab(tab.key);
      }}
    >
      <span class="cat-icon">{icon(tab.meta)}</span>
      <span class="tab-title" title={tab.meta.key}>{tab.meta.key}</span>
      {#if tab.dirty}<span class="dirty" title="未保存">●</span>{/if}
      <button
        class="close"
        title="关闭 (Ctrl+W)"
        onclick={(e) => {
          e.stopPropagation();
          store.closeTab(tab.key);
        }}>×</button>
    </div>
  {/each}
  {#if store.tabs.length === 0}
    <span class="hint">在左侧选择资源以打开</span>
  {/if}
</div>

<style>
  .tabbar {
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 4px 6px 0;
    border-bottom: 1px solid var(--border-subtle);
    overflow-x: auto;
    min-height: 34px;
    background: var(--bg-pane);
  }
  .hint {
    color: var(--text-3);
    font-size: 12px;
    align-self: center;
    padding: 0 8px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 6px 10px;
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: 6px 6px 0 0;
    cursor: pointer;
    font-size: 12px;
    max-width: 220px;
    user-select: none;
    white-space: nowrap;
  }
  .tab:hover { background: var(--bg-hover); }
  .tab.active {
    background: var(--bg-app);
    border-color: var(--border-subtle);
  }
  .cat-icon {
    font-size: 9px;
    font-weight: 700;
    padding: 1px 4px;
    border-radius: 3px;
    background: var(--accent-soft);
    color: var(--accent);
    flex-shrink: 0;
  }
  .tab-title {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .dirty { color: #e08a00; }
  .close {
    border: none;
    background: transparent;
    color: var(--text-3);
    cursor: pointer;
    font-size: 13px;
    padding: 0 2px;
    border-radius: 3px;
  }
  .close:hover { background: var(--bg-active); color: var(--text-1); }
</style>
