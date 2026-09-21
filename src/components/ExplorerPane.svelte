<script lang="ts">
  import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
  import ContextMenu from "./ContextMenu.svelte";
  import SourcePropsDialog from "./SourcePropsDialog.svelte";
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import ResourceRows from "./ResourceRows.svelte";
  import type { InsertionInfo } from "../lib/api";
  import { idKey, type Category, type ResourceMeta } from "../lib/types";

  // ---- context menus / props ----
  let menu = $state<{
    x: number;
    y: number;
    items: { label: string; danger?: boolean; disabled?: boolean; act: () => void }[];
  } | null>(null);
  let propsOf = $state<api.SourceProps | null>(null);

  function openSourceMenu(e: MouseEvent, id: number) {
    e.preventDefault();
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        {
          label: "属性",
          act: async () => {
            try {
              propsOf = await api.sourceProps(id);
            } catch (err) {
              store.toast("error", String(err));
            }
          },
        },
        {
          label: "移除此文件（丢弃全部变更，不影响原文件）",
          danger: true,
          act: () => store.removeSource(id),
        },
      ],
    };
  }

  function openRowMenu(e: MouseEvent, m: ResourceMeta) {
    e.preventDefault();
    const deleted = m.deleted;
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "打开", act: () => store.openResource(m.id) },
        deleted
          ? { label: "还原（取消删除）", act: () => store.restoreResource(m.id) }
          : { label: "标记删除（导出生效）", danger: true, act: () => store.markDeleted(m.id) },
      ],
    };
  }

  // ---- insertions ----
  let insertions = $state<InsertionInfo[]>([]);
  let showInsertForm = $state(false);
  let insKey = $state("");
  let insHtml = $state("<p></p>");
  let busy = $state(false);

  const mdxSources = $derived(store.sources.filter((s) => s.kind === "mdx"));
  const mddSources = $derived(store.sources.filter((s) => s.kind === "mdd"));

  async function refreshInsertions() {
    try {
      insertions = await api.listInsertions(null);
    } catch (e) {
      store.toast("error", String(e));
    }
  }

  async function submitEntry() {
    if (mdxSources.length === 0) {
      store.toast("error", "请先打开一个 MDX 源");
      return;
    }
    busy = true;
    try {
      await api.insertEntry(mdxSources[0].id, insKey, insHtml);
      store.toast("ok", `词条 “${insKey}” 已加入待插入列表（导出时写入）`);
      insKey = "";
      insHtml = "<p></p>";
      showInsertForm = false;
      await refreshInsertions();
    } catch (e) {
      store.toast("error", String(e));
    } finally {
      busy = false;
    }
  }

  let insTarget = $state<number | null>(null);
  let insPrefix = $state("");
  let showResForm = $state(false);
  let showNewPanel = $state(false);

  function toggleResForm() {
    showResForm = !showResForm;
    if (showResForm && insTarget === null && mddSources.length > 0) {
      insTarget = mddSources[0].id;
    }
  }

  async function pickResourceFiles() {
    if (mddSources.length === 0) {
      store.toast("error", "请先打开（或新建）一个 MDD 源");
      return;
    }
    const paths = await openFileDialog({ multiple: true });
    if (!paths) return;
    const target = insTarget ?? mddSources[0].id;
    busy = true;
    try {
      const [added, errors] = await api.insertResources(
        target,
        paths as string[],
        insPrefix.trim() || null
      );
      if (added > 0) store.toast("ok", `已加入 ${added} 个资源（导出时写入）`);
      for (const e of errors) store.toast("error", e);
      await refreshInsertions();
    } catch (e) {
      store.toast("error", String(e));
    } finally {
      busy = false;
    }
  }

  /** Save-dialog → backend creates the empty file and opens it as a source. */
  async function createNew(kind: "mdx" | "mdd") {
    const ext = kind === "mdx" ? ".mdx" : ".mdd";
    let path = await saveFileDialog({
      defaultPath: kind === "mdx" ? "新词典.mdx" : "新资源库.mdd",
      filters: [
        { name: kind === "mdx" ? "MDX 词典" : "MDD 资源库", extensions: [ext.slice(1)] },
      ],
    });
    if (!path) return;
    // open_path dispatches on the extension — make sure it is there.
    if (!path.toLowerCase().endsWith(ext)) path += ext;
    const s = await store.createSource(kind, path);
    if (s) {
      showNewPanel = false;
      if (kind === "mdd" && showResForm && insTarget === null) insTarget = s.id;
    }
  }

  async function dropInsertion(idx: number) {
    try {
      await api.removeInsertion(idx);
      await refreshInsertions();
    } catch (e) {
      store.toast("error", String(e));
    }
  }

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
    <div class="insert-actions">
      <button title="向第一个 MDX 源插入新词条" onclick={() => (showInsertForm = !showInsertForm)}>
        ＋词条
      </button>
      <button title="选择文件导入 MDD 资源（可设目标与前缀）" onclick={toggleResForm} disabled={busy}>
        ＋资源
      </button>
      <button title="新建空的 MDX / MDD 文件并加载" onclick={() => (showNewPanel = !showNewPanel)}>
        ＋新建
      </button>
    </div>
  </header>
  {#if showInsertForm}
    <form class="insert-form" onsubmit={(e) => { e.preventDefault(); void submitEntry(); }}>
      <label>词头 <input bind:value={insKey} placeholder="新词条词头" required /></label>
      <textarea bind:value={insHtml} rows="4" spellcheck="false"></textarea>
      <div class="row">
        <span class="hint" hidden={mdxSources.length === 0}>目标: {mdxSources[0]?.name}</span>
        <button type="submit" disabled={busy || !insKey.trim()}>加入待插入</button>
      </div>
      {#if mdxSources.length === 0}
        <p class="hint">还没有 MDX 源——先 <button type="button" class="link" onclick={() => createNew("mdx")}>新建一个 MDX</button> 或打开现有词典。</p>
      {/if}
    </form>
  {/if}

  {#if showResForm}
    <div class="insert-res">
      {#if mddSources.length > 0}
        <label>
          导入资源到
          <select bind:value={insTarget}>
            {#each mddSources as s (s.id)}
              <option value={s.id}>{s.name}</option>
            {/each}
          </select>
        </label>
        <label>
          路径前缀
          <input bind:value={insPrefix} placeholder="如 img/custom（可空）" />
        </label>
        <button onclick={pickResourceFiles} disabled={busy}>选择文件并加入待插入…</button>
      {:else}
        <span class="hint">还没有 MDD 源——</span>
        <button onclick={() => createNew("mdd")} disabled={busy}>新建 MDD 资源库…</button>
      {/if}
    </div>
  {/if}

  {#if showNewPanel}
    <div class="new-panel">
      <button onclick={() => createNew("mdx")} disabled={busy}>新建 MDX 词典…</button>
      <button onclick={() => createNew("mdd")} disabled={busy}>新建 MDD 资源库…</button>
      <p class="hint">空文件创建后立即加载；词条/资源加入待插入，导出时写入。</p>
    </div>
  {/if}

  {#if insertions.length > 0}
    <div class="insert-list">
      <div class="insert-head">待插入 · {insertions.length} 项（导出时写入）</div>
      {#each insertions as ins (ins.index)}
        <div class="insert-row">
          <span class="tag" class:res={ins.kind === "resource"}>
            {ins.kind === "entry" ? "词条" : "资源"}
          </span>
          <span class="name" title={ins.name}>{ins.name}</span>
          <span class="size">{ins.size}B</span>
          <button class="drop" title="移除" onclick={() => dropInsertion(ins.index)}>×</button>
        </div>
      {/each}
    </div>
  {/if}

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
      <div class="empty-actions">
        <button onclick={() => createNew("mdx")}>新建 MDX 词典…</button>
        <button onclick={() => createNew("mdd")}>新建 MDD 资源库…</button>
      </div>
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
          <button
            class="res-btn"
            onclick={() => store.openResource(m.id)}
            oncontextmenu={(e) => openRowMenu(e, m)}
          >
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
          <button
            class="source-row"
            onclick={() => toggleSource(src.id)}
            oncontextmenu={(e) => openSourceMenu(e, src.id)}
          >
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
                <ResourceRows
                    sourceId={src.id}
                    category={st.category}
                    onrowmenu={(e, m) => openRowMenu(e, m)}
                  />
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

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}
{#if propsOf}
  <SourcePropsDialog props={propsOf} onclose={() => (propsOf = null)} />
{/if}

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
  .insert-actions { display: flex; gap: 4px; }
  .insert-actions button {
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 11px;
    padding: 3px 8px;
    cursor: pointer;
  }
  .insert-actions button:hover:not(:disabled) { background: var(--bg-hover); }
  .insert-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0 12px 8px;
    padding: 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--bg-app);
  }
  .insert-form label { font-size: 11px; color: var(--text-2); display: flex; gap: 6px; align-items: center; }
  .insert-form input {
    flex: 1;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    padding: 4px 6px;
    font-size: 12px;
    background: var(--bg-pane);
    color: var(--text-1);
  }
  .insert-form textarea {
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    padding: 6px;
    font-family: var(--font-code);
    font-size: 11px;
    resize: vertical;
    background: var(--bg-pane);
    color: var(--text-1);
  }
  .insert-form .row { display: flex; justify-content: flex-end; align-items: center; gap: 8px; }
  .insert-form .row button {
    border: none;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    font-size: 12px;
    padding: 4px 12px;
    cursor: pointer;
  }
  .insert-form .row button:disabled { opacity: 0.5; cursor: default; }
  .insert-res {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 10px;
    align-items: center;
    margin: 0 12px 8px;
    padding: 8px;
    border: 1px dashed var(--border-subtle);
    border-radius: 8px;
    font-size: 11px;
  }
  .insert-res label { display: flex; align-items: center; gap: 4px; color: var(--text-2); }
  .insert-res select, .insert-res input {
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 11px;
    padding: 3px 6px;
  }
  .insert-res input { width: 150px; }
  .insert-res button {
    border: none;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    font-size: 11px;
    padding: 4px 10px;
    cursor: pointer;
  }
  .insert-res button:disabled, .new-panel button:disabled { opacity: 0.5; cursor: default; }
  .new-panel {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0 12px 8px;
    padding: 8px;
    border: 1px dashed var(--border-subtle);
    border-radius: 8px;
    background: var(--bg-app);
  }
  .new-panel button {
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-pane);
    color: var(--text-1);
    font-size: 12px;
    padding: 5px 8px;
    cursor: pointer;
    text-align: left;
  }
  .new-panel button:hover:not(:disabled) { background: var(--bg-hover); }
  .hint { font-size: 11px; color: var(--text-3); margin: 0; }
  .hint .link, button.link {
    border: none;
    background: none;
    color: var(--accent);
    cursor: pointer;
    font-size: 11px;
    padding: 0;
    text-decoration: underline;
  }
  .empty-actions {
    display: flex;
    justify-content: center;
    gap: 8px;
    margin-top: 8px;
  }
  .empty-actions button {
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 11px;
    padding: 3px 8px;
    cursor: pointer;
  }
  .empty-actions button:hover { background: var(--bg-hover); }
  .insert-list {
    margin: 0 12px 8px;
    border: 1px dashed var(--accent);
    border-radius: 8px;
    padding: 6px;
  }
  .insert-head { font-size: 11px; color: var(--accent); margin-bottom: 4px; }
  .insert-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    padding: 2px 4px;
  }
  .insert-row .tag {
    font-size: 9px;
    font-weight: 700;
    padding: 1px 4px;
    border-radius: 3px;
    background: var(--accent);
    color: #fff;
    flex-shrink: 0;
  }
  .insert-row .tag.res { background: #8b5cf6; }
  .insert-row .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .insert-row .size { color: var(--text-3); font-size: 10px; flex-shrink: 0; }
  .insert-row .drop {
    border: none;
    background: transparent;
    color: var(--text-3);
    cursor: pointer;
    font-size: 13px;
    padding: 0 2px;
  }
  .insert-row .drop:hover { color: var(--danger); }
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
