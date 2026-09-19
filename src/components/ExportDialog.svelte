<script lang="ts">
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import type { ExportReport } from "../lib/types";

  let { onclose }: { onclose: () => void } = $props();

  let outDir = $state("");
  let rebuildMdx = $state(true);
  let rebuildMdd = $state(true);
  let embed = $state(false);
  let embedTarget = $state<number | null>(null);
  let saveExternals = $state(false);

  let running = $state(false);
  let report = $state<ExportReport | null>(null);

  const mddSources = $derived(store.sources.filter((s) => s.kind === "mdd"));
  const hasExternals = $derived(store.sources.some((s) => s.kind === "ext"));

  async function run() {
    if (!outDir.trim()) {
      store.toast("error", "请填写输出目录");
      return;
    }
    running = true;
    try {
      report = await api.exportBuild({
        outDir,
        mdx: rebuildMdx,
        mdd: rebuildMdd,
        embedExternals: embed,
        embedTarget,
        saveExternals,
      });
      const ok = report.files.filter((f) => f.checkOk).length;
      store.toast(report.ok ? "ok" : "info", `导出完成：${ok}/${report.files.length} 项校验通过`);
    } catch (e) {
      store.toast("error", String(e));
    } finally {
      running = false;
    }
  }

  function fmt(n: number): string {
    return n > 1024 * 1024
      ? `${(n / 1024 / 1024).toFixed(2)} MB`
      : `${(n / 1024).toFixed(1)} KB`;
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog">
    <header>
      <h2>导出</h2>
      <button class="x" onclick={onclose}>×</button>
    </header>

    <section>
      <h3>输出目录</h3>
      <input class="dir" bind:value={outDir} placeholder="例如 D:\\export\\dict" />
    </section>

    <section>
      <h3>内容</h3>
      <label><input type="checkbox" bind:checked={rebuildMdx} /> 重建有词条修订的 MDX</label>
      <label><input type="checkbox" bind:checked={rebuildMdd} /> 重建有资源修订的 MDD</label>
      {#if hasExternals}
        <label><input type="checkbox" bind:checked={embed} /> 嵌入外部 js/css 到 MDD</label>
        {#if embed}
          <select bind:value={embedTarget} class="indented">
            <option value={null}>第一个 MDD 源</option>
            {#each mddSources as s (s.id)}
              <option value={s.id}>{s.name}</option>
            {/each}
          </select>
        {/if}
        <label><input type="checkbox" bind:checked={saveExternals} /> 外部文件另存为普通文件</label>
      {:else}
        <p class="note">未加载外部 js/css 文件，嵌入选项不可用。</p>
      {/if}
    </section>

    <section class="grow">
      <h3>结果</h3>
      {#if report}
        <div class="report">
          {#each report.files as f (f.path)}
            <div class="file">
              <span class:ok={f.checkOk} class:bad={!f.checkOk}>
                {f.checkOk ? "✓" : "!"}
              </span>
              <span class="path" title={f.path}>{f.path}</span>
              <span class="meta">
                {#if f.entries > 0}{f.entries.toLocaleString()} 条 · {/if}{fmt(f.bytes)}
                {#if f.message}· {f.message}{/if}
              </span>
            </div>
          {/each}
          {#if report.skippedExternals.length > 0}
            <p class="note">跳过（键冲突）: {report.skippedExternals.join(", ")}</p>
          {/if}
        </div>
      {:else}
        <p class="note">所有输出写入改写层之上的构建结果；每个文件导出后自动重新打开校验（条目数 + 抽查内容）。</p>
      {/if}
    </section>

    <footer>
      <button disabled={running} onclick={run}>{running ? "构建中…" : "开始导出"}</button>
    </footer>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .dialog {
    width: min(680px, 92vw);
    height: min(520px, 88vh);
    background: var(--bg-pane);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    padding: 16px;
    gap: 12px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  header { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0; font-size: 15px; }
  h3 { margin: 0 0 6px; font-size: 12px; color: var(--text-2); }
  .x { border: none; background: none; font-size: 16px; cursor: pointer; color: var(--text-3); }
  section { font-size: 13px; }
  section.grow { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .dir {
    width: 100%;
    box-sizing: border-box;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 6px 10px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 13px;
    font-family: var(--font-code);
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 4px 0;
  }
  select {
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 3px 6px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 12px;
  }
  .indented { margin-left: 22px; }
  .note { color: var(--text-3); font-size: 12px; }
  .report {
    flex: 1;
    overflow: auto;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 12px;
  }
  .file {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 4px 0;
    border-bottom: 1px solid var(--border-subtle);
  }
  .ok { color: var(--ok); }
  .bad { color: var(--danger); font-weight: 700; }
  .path {
    flex-shrink: 0;
    max-width: 55%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-code);
    font-size: 11px;
    direction: rtl;
    text-align: left;
  }
  .meta { color: var(--text-3); }
  footer { display: flex; justify-content: flex-end; }
  footer button {
    padding: 7px 18px;
    border: none;
    border-radius: 8px;
    background: var(--accent);
    color: #fff;
    font-size: 13px;
    cursor: pointer;
  }
  footer button:disabled { opacity: 0.5; cursor: default; }
</style>
