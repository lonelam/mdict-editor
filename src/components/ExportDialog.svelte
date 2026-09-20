<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
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
  let lossy = $state(false);
  let onlyEdited = $state(false);

  // Default output location: the first mdx source's folder (else any source's).
  const defaultDir = $derived(
    store.sources.find((s) => s.kind === "mdx")?.dir ??
      store.sources.find((s) => s.dir)?.dir ??
      ""
  );
  $effect(() => {
    if (!outDir && defaultDir) outDir = defaultDir;
  });

  async function pickDir() {
    const picked = await openDialog({ directory: true });
    if (typeof picked === "string") outDir = picked;
  }

  let running = $state(false);
  let report = $state<ExportReport | null>(null);
  let job = $state<string | null>(null);
  let progress = $state<{ done: number; total: number; item: string } | null>(null);
  let exportedMdx = $state<string[]>([]);

  const mddSources = $derived(store.sources.filter((s) => s.kind === "mdd"));
  const hasExternals = $derived(store.sources.some((s) => s.kind === "ext"));

  async function run() {
    if (!outDir.trim()) {
      store.toast("error", "请填写输出目录");
      return;
    }
    running = true;
    progress = { done: 0, total: 0, item: "启动中…" };
    try {
      const start = api.exportStart({
        outDir,
        mdx: rebuildMdx,
        mdd: rebuildMdd,
        embedExternals: embed,
        embedTarget,
        saveExternals,
        onlyEdited,
        lossy: lossy
          ? [
              { kind: "img-webp", quality: 75 },
              { kind: "png-quantize", colors: 256 },
              { kind: "css-purge" },
              { kind: "minify-css" },
              { kind: "minify-js" },
            ]
          : null,
      });
      start.then((id) => (job = id));
      report = await api.runJob<ExportReport>(start, (done, total, item) => {
        progress = { done, total, item };
      });
      const ok = report.files.filter((f) => f.checkOk).length;
      if (report.files.length === 0) {
        store.toast("info", "没有需要导出的内容：无编辑，且未勾选有损副本/外部嵌入");
      } else {
        store.toast(report.ok ? "ok" : "info", `导出完成：${ok}/${report.files.length} 项校验通过`);
      }
      exportedMdx = report.files
        .filter((f) => f.path.toLowerCase().endsWith(".mdx"))
        .map((f) => f.path);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      store.toast(msg === "已取消" ? "info" : "error", msg);
    } finally {
      running = false;
      job = null;
      progress = null;
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
      <div class="dir-row">
        <input class="dir" bind:value={outDir} placeholder="例如 D:\\export\\dict" />
        <button class="browse" onclick={pickDir}>浏览…</button>
      </div>
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
      <label>
        <input type="checkbox" bind:checked={lossy} /> 同时生成有损压缩副本 (.lossy.mdd)
      </label>
      <p class="note">
        有损副本独立于原始版本：原始产物照常导出，改写层与源文件不受影响。
        当前链：图片转有损 WebP（保透明）+ PNG 调色板量化 + CSS 死规则清除 +
        CSS/JS 压缩；Opus 音频 / 字体子集化将随后续版本接入。
      </p>
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
        <p class="note">
          所有输出写入改写层之上的构建结果；每个文件导出后自动重新打开校验（条目数 + 抽查内容）。
          导出的词典可一键导入 <b>AALookup</b>（与本应用同构的阅读器：同一 mdictlib 解析核心、同一资源解析顺序，编辑所见即阅读所得）。
        </p>
      {/if}
    </section>

    {#if running}
      <div class="job-progress">
        <div class="bar"><div class="fill" style:width={`${progress && progress.total ? (progress.done / progress.total) * 100 : 0}%`}></div></div>
        <span>{progress ? `${progress.done}/${progress.total} · ${progress.item}` : "准备中…"}</span>
        <button class="cancel" onclick={() => job && api.cancelJob(job)}>取消</button>
      </div>
    {/if}
    <footer>
      <button
        class="aalookup"
        disabled={running || exportedMdx.length === 0}
        title={exportedMdx.length === 0 ? "完成一次包含 MDX 的导出后可用" : "把本次导出的词典交给 AALookup 打开"}
        onclick={async () => {
          try {
            const msg = await api.importToAALookup(exportedMdx);
            store.toast("ok", msg);
          } catch (e) {
            store.toast("error", String(e));
          }
        }}
      >
        ⬇ 一键导入 AALookup
      </button>
      <span class="grow"></span>
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
  .dir-row { display: flex; gap: 6px; }
  .browse {
    flex-shrink: 0;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 12px;
    padding: 6px 12px;
    cursor: pointer;
  }
  .browse:hover { background: var(--bg-hover); }
  .dir {
    flex: 1;
    min-width: 0;
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
  footer .grow { flex: 1; }
  footer .aalookup {
    border-color: #30a14e;
    color: #30a14e;
    background: var(--bg-app);
  }
  footer .aalookup:hover:not(:disabled) { background: #e9f9ee; }
  .job-progress {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
    color: var(--text-2);
  }
  .job-progress .bar {
    flex: 1;
    height: 8px;
    background: var(--bg-active);
    border-radius: 4px;
    overflow: hidden;
  }
  .job-progress .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s;
  }
  .job-progress span {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .job-progress .cancel {
    border: 1px solid var(--danger);
    border-radius: 6px;
    background: var(--bg-pane);
    color: var(--danger);
    font-size: 12px;
    padding: 3px 10px;
    cursor: pointer;
  }
</style>
