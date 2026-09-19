<script lang="ts">
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import ResourceRows from "./ResourceRows.svelte";
  import { idKey, type Category, type ResourceMeta } from "../lib/types";

  const catLabels: Record<Category, string> = {
    entry: "词条", html: "HTML", css: "CSS", js: "JS", image: "图片",
    audio: "音频", video: "视频", font: "字体", text: "文本", other: "其他",
  };
  const catOrder: Category[] = [
    "entry", "html", "css", "js", "image", "audio", "video", "font", "text", "other",
  ];

  let expandedSources = $state<Set<number>>(new Set());
  let expandedCats = $state<Set<string>>(new Set());

  function toggleSource(id: number) {
    const next = new Set(expandedSources);
    next.has(id) ? next.delete(id) : next.add(id);
    expandedSources = next;
  }
  function toggleCat(key: string) {
    const next = new Set(expandedCats);
    next.has(key) ? next.delete(key) : next.add(key);
    expandedCats = next;
  }

  function sourceCats(sourceId: number): { category: Category; count: number; edited: number }[] {
    const stats = store.stats[sourceId] ?? [];
    return [...stats].sort(
      (a, b) => catOrder.indexOf(a.category) - catOrder.indexOf(b.category)
    );
  }

  function kindBadge(kind: string): string {
    return kind === "mdx" ? "MDX" : kind === "mdd" ? "MDD" : "EXT";
  }

  // ---- global search results ----
  let searchResults = $state<ResourceMeta[]>([]);
  let searching = $state(false);
  let searchSeq = 0;
  $effect(() => {
    const q = store.search;
    void store.overlayVersion;
    const seq = ++searchSeq;
    if (!q) {
      searchResults = [];
      return;
    }
    searching = true;
    const timer = setTimeout(async () => {
      try {
        searchResults = await api.listResources({ prefix: q, offset: 0, limit: 300 });
      } catch (e) {
        store.toast("error", String(e));
      } finally {
        if (seq === searchSeq) searching = false;
      }
    }, 200);
    return () => clearTimeout(timer);
  });
</script>

<aside class="explorer">
  <header>
    <h2>资源</h2>
    <button class="open" onclick={() => store.openFiles()} disabled={store.loading}>
      {store.sources.length === 0 ? "打开文件" : "追加"}
    </button>
  </header>
  <input
    class="search"
    placeholder="全局搜索前缀…"
    bind:value={store.search}
  />

  {#if store.sources.length === 0}
    <div class="empty">
      <p>尚无资源。</p>
      <p>打开 .mdx / .mdd 词典，</p>
      <p>或直接添加外部 .js / .css。</p>
    </div>
  {:else if store.search}
    <div class="results">
      <div class="cat-row">
        <span>搜索 “{store.search}” — {searchResults.length} 项</span>
      </div>
      {#each searchResults as m (idKey(m.id))}
        <div class="res-row" class:edited={m.edited} class:deleted={m.deleted}>
          <input
            type="checkbox"
            checked={store.selection.includes(idKey(m.id))}
            onclick={(e) => e.stopPropagation()}
            onchange={() => store.toggleSelection(idKey(m.id))}
          />
          <button class="res-btn" onclick={() => store.openResource(m.id)}>
            <span class="key">{m.key}</span>
            <span class="src">{m.sourceName}</span>
          </button>
        </div>
      {/each}
      {#if searching}<p class="loading">搜索中…</p>{/if}
    </div>
  {:else}
    <div class="tree">
      {#each store.sources as src (src.id)}
        <div class="source-block">
          <button class="source-row" onclick={() => toggleSource(src.id)}>
            <span class="twist">{expandedSources.has(src.id) ? "▾" : "▸"}</span>
            <span class="badge {src.kind}">{kindBadge(src.kind)}</span>
            <span class="name" title={src.title ?? src.name}>{src.title ?? src.name}</span>
            <span class="count">{src.entryCount.toLocaleString()}</span>
          </button>

          {#if expandedSources.has(src.id)}
            {#each sourceCats(src.id) as st (st.category)}
              {@const catKey = `${src.id}:${st.category}`}
              <button class="cat-row" onclick={() => toggleCat(catKey)}>
                <span class="twist">{expandedCats.has(catKey) ? "▾" : "▸"}</span>
                <span class="cat-name">{catLabels[st.category]}</span>
                {#if st.edited > 0}
                  <span class="edited-count" title="已编辑">⚡{st.edited}</span>
                {/if}
                <span class="count">{st.count.toLocaleString()}</span>
              </button>
              {#if expandedCats.has(catKey)}
                <ResourceRows sourceId={src.id} category={st.category} />
              {/if}
            {/each}
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  {#if store.selection.length > 0}
    <footer class="selection">
      已选 {store.selection.length} 项
      <button onclick={() => (store.selection = [])}>清除</button>
    </footer>
  {/if}
</aside>

<style>
  .explorer {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg-pane);
    border-right: 1px solid var(--border-subtle);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 8px;
  }
  h2 { margin: 0; font-size: 13px; }
  .open {
    border: none;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    font-size: 12px;
    padding: 5px 10px;
    cursor: pointer;
  }
  .open:disabled { opacity: 0.6; }
  .search {
    margin: 0 12px 8px;
    padding: 5px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    font-size: 12px;
    background: var(--bg-app);
    color: var(--text-1);
  }
  .tree, .results {
    flex: 1;
    overflow-y: auto;
    padding: 0 4px 8px;
  }
  .empty {
    padding: 24px 16px;
    color: var(--text-3);
    font-size: 12px;
    text-align: center;
    line-height: 1.8;
  }
  .empty p { margin: 0; }
  .source-row, .cat-row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    border: none;
    background: transparent;
    font: inherit;
    font-size: 12px;
    padding: 5px 6px;
    border-radius: 6px;
    cursor: pointer;
    color: var(--text-1);
    text-align: left;
  }
  .source-row { font-weight: 600; }
  .cat-row { padding-left: 24px; }
  .source-row:hover, .cat-row:hover { background: var(--bg-hover); }
  .twist { color: var(--text-3); font-size: 10px; width: 10px; }
  .badge {
    font-size: 9px;
    font-weight: 700;
    padding: 1px 4px;
    border-radius: 3px;
    color: #fff;
    flex-shrink: 0;
  }
  .badge.mdx { background: #2f6fed; }
  .badge.mdd { background: #30a14e; }
  .badge.ext { background: #8b5cf6; }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cat-name { flex: 1; }
  .count { color: var(--text-3); font-size: 11px; flex-shrink: 0; }
  .edited-count { color: #e08a00; font-size: 11px; flex-shrink: 0; }
  .res-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-left: 40px;
  }
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
    padding: 4px 6px;
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
  .src { color: var(--text-3); font-size: 10px; flex-shrink: 0; }
  .res-row.edited .key { color: #e08a00; }
  .res-row.edited .key::after { content: " ⚡"; }
  .res-row.deleted .key {
    text-decoration: line-through;
    color: var(--text-3);
  }
  .loading { color: var(--text-3); font-size: 12px; padding: 8px 12px; }
  .selection {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-top: 1px solid var(--border-subtle);
    font-size: 12px;
    color: var(--accent);
  }
  .selection button {
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    background: var(--bg-app);
    font-size: 11px;
    padding: 2px 8px;
    cursor: pointer;
  }
</style>
