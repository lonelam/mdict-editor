<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { idKey, type Category, type ResourceMeta } from "../lib/types";

  let {
    sourceId,
    category,
    onrowmenu,
  }: {
    sourceId: number;
    category: Category;
    onrowmenu?: (e: MouseEvent, m: ResourceMeta) => void;
  } = $props();

  let rows = $state<ResourceMeta[]>([]);
  let loading = $state(false);
  let done = $state(false);
  let version = -1;

  async function loadMore(reset = false) {
    if (loading || (done && !reset)) return;
    loading = true;
    try {
      const page = await api.listResources({
        source: sourceId,
        category,
        offset: reset ? 0 : rows.length,
        limit: 100,
      });
      rows = reset ? page : [...rows, ...page];
      if (page.length < 100) done = true;
    } catch (e) {
      store.toast("error", String(e));
    } finally {
      loading = false;
    }
  }

  onMount(() => void loadMore(true));

  // Reset when the overlay changes (edits/reverts change edited/deleted flags).
  $effect(() => {
    void store.overlayVersion;
    if (version !== -1 && version !== store.overlayVersion) {
      rows = [];
      done = false;
      void loadMore(true);
    }
    version = store.overlayVersion;
  });
</script>

<div class="rows">
  {#each rows as m (idKey(m.id))}
    <div class="res-row" class:edited={m.edited} class:deleted={m.deleted}>
      <input
        type="checkbox"
        title="加入批量选择"
        checked={store.selection.includes(idKey(m.id))}
        onclick={(e) => e.stopPropagation()}
        onchange={() => store.toggleSelection(idKey(m.id))}
      />
      <button
        class="res-btn"
        onclick={() => store.openResource(m.id)}
        oncontextmenu={(e) => onrowmenu?.(e, m)}
      >
        <span class="key" title={m.key}>{m.key}</span>
        <span class="size">{m.sizeCurrent !== null ? `${m.sizeCurrent}B` : ""}</span>
      </button>
    </div>
  {:else}
    <p class="none">空</p>
  {/each}
  {#if !done}
    <button class="more" disabled={loading} onclick={() => loadMore()}>
      {loading ? "加载中…" : "加载更多"}
    </button>
  {/if}
</div>

<style>
  .rows { display: flex; flex-direction: column; }
  .res-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-left: 40px;
  }
  .res-row input { flex-shrink: 0; }
  .res-btn {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    border: none;
    background: transparent;
    font: inherit;
    font-size: 12px;
    padding: 3px 6px;
    border-radius: 6px;
    cursor: pointer;
    color: var(--text-1);
    text-align: left;
  }
  .res-btn:hover { background: var(--bg-hover); }
  .key {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .size { color: var(--text-3); font-size: 10px; flex-shrink: 0; }
  .res-row.edited .key { color: #e08a00; }
  .res-row.edited .key::after { content: " ⚡"; }
  .res-row.deleted .key { text-decoration: line-through; color: var(--text-3); }
  .more {
    margin: 4px 0 4px 40px;
    border: 1px dashed var(--border-subtle);
    border-radius: 6px;
    background: transparent;
    color: var(--text-3);
    font-size: 11px;
    padding: 3px;
    cursor: pointer;
  }
  .none { color: var(--text-3); font-size: 11px; padding-left: 46px; margin: 2px 0; }
</style>
