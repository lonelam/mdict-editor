<script lang="ts">
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import type { Category, Processor, Scope, StepReport } from "../lib/types";

  let { onclose }: { onclose: () => void } = $props();

  // Scope
  let scopeMode = $state<"selection" | "source" | "category" | "all">("all");
  let scopeSource = $state<number | null>(null);
  let scopeCategory = $state<Category | null>(null);

  const mddSources = $derived(store.sources.filter((s) => s.kind === "mdd"));

  // Steps (checkbox list; params inline)
  let doCss = $state(true);
  let doJs = $state(true);
  let doHtml = $state(false);
  let doPng = $state(true);
  let pngLevel = $state(2);
  let doWebp = $state(false);
  let webpQuality = $state(75);
  let doQuantize = $state(false);
  let doPurge = $state(false);
  let doOpus = $state(false);

  let running = $state(false);
  let reports = $state<StepReport[] | null>(null);
  let job = $state<string | null>(null);
  let progress = $state<{ done: number; total: number; item: string } | null>(null);

  function buildSteps(): Processor[] {
    const steps: Processor[] = [];
    if (doCss) steps.push({ kind: "minify-css" });
    if (doJs) steps.push({ kind: "minify-js" });
    if (doHtml) steps.push({ kind: "minify-html" });
    if (doPng) steps.push({ kind: "png-optimize", level: pngLevel });
    if (doQuantize) steps.push({ kind: "png-quantize", colors: 256 });
    if (doWebp) steps.push({ kind: "img-webp", quality: webpQuality });
    if (doPurge) steps.push({ kind: "css-purge" });
    if (doOpus) steps.push({ kind: "audio-opus", bitrateKbps: 24 });
    return steps;
  }

  function buildScope(): Scope {
    switch (scopeMode) {
      case "selection":
        return { ids: store.selectedIds() };
      case "source":
        return { source: scopeSource };
      case "category":
        return { category: scopeCategory };
      default:
        return { all: true };
    }
  }

  const canRun = $derived(
    (doCss || doJs || doHtml || doPng || doWebp || doQuantize || doPurge || doOpus) &&
      (scopeMode !== "selection" || store.selection.length > 0) &&
      (scopeMode !== "source" || scopeSource !== null) &&
      (scopeMode !== "category" || scopeCategory !== null)
  );

  async function run(dry: boolean) {
    running = true;
    reports = null;
    progress = { done: 0, total: 0, item: "启动中…" };
    try {
      reports = await api.runJob<StepReport[]>(
        () =>
          dry
            ? api.pipelineDryRunStart(buildSteps(), buildScope())
            : api.pipelineApplyStart(buildSteps(), buildScope()),
        (done, total, item) => {
          progress = { done, total, item };
        },
        (id) => (job = id)
      );
      if (!dry) {
        const ok = reports.filter((r) => r.status === "ok");
        const saved = ok.reduce((sum, r) => sum + r.delta, 0);
        store.toast("ok", `已应用 ${ok.length} 项，共节省 ${saved.toLocaleString()} 字节`);
        store.bumpOverlay();
      }
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
      <h2>处理管线</h2>
      <button class="x" onclick={onclose}>×</button>
    </header>

    <section>
      <h3>① 作用范围</h3>
      <div class="row">
        <label><input type="radio" bind:group={scopeMode} value="all" /> 全部资源</label>
        <label>
          <input type="radio" bind:group={scopeMode} value="selection" />
          当前选择 ({store.selection.length})
        </label>
        <label>
          <input type="radio" bind:group={scopeMode} value="source" /> 指定源
        </label>
        {#if scopeMode === "source"}
          <select bind:value={scopeSource}>
            {#each mddSources.concat(store.sources.filter(s => s.kind !== "mdx")) as s (s.id)}
              <option value={s.id}>{s.name}</option>
            {/each}
          </select>
        {/if}
        <label>
          <input type="radio" bind:group={scopeMode} value="category" /> 指定分类
        </label>
        {#if scopeMode === "category"}
          <select bind:value={scopeCategory}>
            <option value="css">CSS</option>
            <option value="js">JS</option>
            <option value="html">HTML</option>
            <option value="entry">词条</option>
            <option value="image">图片(png)</option>
          </select>
        {/if}
      </div>
    </section>

    <section>
      <h3>② 处理步骤</h3>
      <div class="row">
        <label><input type="checkbox" bind:checked={doCss} /> 压缩 CSS (lightningcss)</label>
        <label><input type="checkbox" bind:checked={doJs} /> 压缩 JS (swc)</label>
        <label><input type="checkbox" bind:checked={doHtml} /> 压缩 HTML/词条 (保守)</label>
        <label><input type="checkbox" bind:checked={doPng} /> PNG 优化 (oxipng)</label>
        {#if doPng}
          <label class="indented">级别
            <select bind:value={pngLevel}>
              <option value={1}>1 快</option>
              <option value={2}>2 默认</option>
              <option value={4}>4 强</option>
            </select>
          </label>
        {/if}
        <label><input type="checkbox" bind:checked={doWebp} /> 图片转有损 WebP（保透明，推荐）</label>
        {#if doWebp}
          <label class="indented">质量 {webpQuality}
            <input type="range" min="40" max="95" bind:value={webpQuality} />
          </label>
        {/if}
        <label><input type="checkbox" bind:checked={doQuantize} /> PNG 调色板量化（256 色）</label>
        <label><input type="checkbox" bind:checked={doPurge} /> CSS 死规则清除（按词条语料）</label>
        <label>
          <input type="checkbox" bind:checked={doOpus} />
          发音音频转 Opus 24kbps（WAV/MP3 → Ogg/Opus，约 5-10×；注意旧播放器兼容性）
        </label>
      </div>
      <p class="note">处理器自动跳过不适用的资源；结果写入改写层，可随时还原。</p>
    </section>

    <section class="grow">
      <h3>③ 预览与应用</h3>
      {#if reports}
        <div class="report">
          <table>
            <thead>
              <tr><th>资源</th><th>原</th><th>新</th><th>Δ</th><th>状态</th></tr>
            </thead>
            <tbody>
              {#each reports as r (r.status + r.key)}
                <tr>
                  <td class="break" title={r.key}>{r.key}</td>
                  <td>{fmt(r.before)}</td>
                  <td>{fmt(r.after)}</td>
                  <td class:pos={r.delta > 0}>{r.delta > 0 ? `−${fmt(r.delta)}` : "—"}</td>
                  <td class={r.status}>{r.status === "ok" ? "✓" : r.status === "skip" ? "跳过" : "错误"}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="note">先“试运行”查看每项前后体积；确认后“应用”写入改写层。</p>
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
      <button disabled={running || !canRun} onclick={() => run(true)}>
        {running ? "运行中…" : "试运行"}
      </button>
      <button
        class="primary"
        disabled={running || !canRun || !reports?.some((r) => r.status === "ok")}
        onclick={() => run(false)}
      >
        应用
      </button>
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
    width: min(760px, 92vw);
    height: min(560px, 88vh);
    background: var(--bg-pane);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    padding: 16px;
    gap: 12px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 { margin: 0; font-size: 15px; }
  h3 { margin: 0 0 6px; font-size: 12px; color: var(--text-2); }
  .x {
    border: none;
    background: transparent;
    font-size: 16px;
    cursor: pointer;
    color: var(--text-3);
  }
  section { font-size: 13px; }
  section.grow {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }
  .row label { display: flex; align-items: center; gap: 5px; }
  .row label.indented { margin-left: 20px; }
  select {
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 3px 6px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 12px;
  }
  .note { color: var(--text-3); font-size: 12px; margin: 8px 0 0; }
  .report {
    flex: 1;
    overflow: auto;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th, td {
    text-align: left;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border-subtle);
  }
  th { position: sticky; top: 0; background: var(--bg-pane); }
  td.break {
    max-width: 280px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-code);
    font-size: 11px;
  }
  td.pos { color: var(--ok); font-weight: 600; }
  td.ok { color: var(--ok); }
  td.skip { color: var(--text-3); }
  td.error { color: var(--danger); }
  footer {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
  }
  footer button {
    padding: 7px 18px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 13px;
    cursor: pointer;
  }
  footer .primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  footer button:disabled { opacity: 0.5; cursor: default; }
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
