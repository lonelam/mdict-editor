<script lang="ts">
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import type { ResourceContent } from "../lib/types";

  const tab = $derived(store.activeTab);

  let content = $derived<ResourceContent | null>(null);
  let version = $state(-1);
  $effect(() => {
    const id = tab?.meta.id;
    version = store.overlayVersion;
    if (!id) return;
    let cancelled = false;
    api.readResource(id).then((c) => {
      if (!cancelled) content = c;
    });
    return () => {
      cancelled = true;
    };
  });

  async function undo() {
    if (!tab) return;
    const ok = await api.undoResource(tab.meta.id);
    store.toast(ok ? "ok" : "info", ok ? "已回退一个修订" : "没有可回退的修订");
    store.bumpOverlay();
  }

  async function revert() {
    if (!tab) return;
    const ok = await api.revertResource(tab.meta.id);
    store.toast(ok ? "ok" : "info", ok ? "已还原到原始内容" : "该资源没有编辑");
    store.bumpOverlay();
  }

  async function markDelete() {
    if (!tab) return;
    await api.deleteResource(tab.meta.id);
    store.toast("ok", "已标记删除（导出时生效，可还原）");
    store.bumpOverlay();
  }

  function fmtSize(n: number): string {
    return n > 1024 * 1024
      ? `${(n / 1024 / 1024).toFixed(2)} MB`
      : n > 1024
        ? `${(n / 1024).toFixed(1)} KB`
        : `${n} B`;
  }
</script>

<div class="inspector">
  <h2>属性</h2>
  {#if !tab || !content}
    <p class="empty">未选择资源</p>
  {:else}
    <dl>
      <dt>键 / 词头</dt>
      <dd class="break">{content.meta.key}</dd>
      <dt>分类</dt>
      <dd>{content.meta.category} · {content.meta.mime}</dd>
      <dt>所属源</dt>
      <dd>{content.meta.sourceName}</dd>
      <dt>当前大小</dt>
      <dd>{fmtSize(content.sizeCurrent)}</dd>
      {#if content.meta.edited}
        <dt>原始大小</dt>
        <dd>{fmtSize(content.sizeOriginal ?? 0)}</dd>
        <dt>修订历史</dt>
        <dd>{content.historyLen} 版</dd>
      {/if}
      {#if content.meta.deleted}
        <dt class="danger">状态</dt>
        <dd class="danger">已标记删除</dd>
      {/if}
    </dl>

    <div class="actions">
      {#if content.meta.edited}
        <button onclick={undo} disabled={content.historyLen === 0}>撤销一版</button>
        <button onclick={revert}>还原原始</button>
      {:else}
        <button class="danger" onclick={markDelete}>标记删除</button>
      {/if}
      {#if content.meta.deleted && !content.meta.edited}
        <!-- deleted flag lives in the revision; revert clears it -->
      {/if}
    </div>
  {/if}
</div>

<style>
  .inspector {
    padding: 12px;
    overflow-y: auto;
    font-size: 12px;
    border-top: 1px solid var(--border-subtle);
    background: var(--bg-pane);
  }
  h2 { margin: 0 0 8px; font-size: 13px; }
  .empty { color: var(--text-3); }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 12px;
    margin: 0;
  }
  dt { color: var(--text-3); }
  dd { margin: 0; word-break: break-all; }
  dd.break { font-family: var(--font-code); font-size: 11px; }
  .danger { color: var(--danger); }
  .actions {
    display: flex;
    gap: 6px;
    margin-top: 12px;
    flex-wrap: wrap;
  }
  .actions button {
    flex: 1;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 12px;
    padding: 5px 8px;
    cursor: pointer;
    white-space: nowrap;
  }
  .actions button:hover:not(:disabled) { background: var(--bg-hover); }
  .actions button:disabled { opacity: 0.5; cursor: default; }
  .actions .danger { color: var(--danger); border-color: var(--danger); }
</style>
