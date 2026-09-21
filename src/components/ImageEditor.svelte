<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import type { ResourceId } from "../lib/types";

  let { id }: { id: ResourceId } = $props();

  let dataUrl = $state("");
  let error = $state("");
  let loading = $state(true);
  let width = $state(0);
  let height = $state(0);
  let key = $state("");
  let fit = $state(true);
  let zoom = $state(1);

  // Operation form
  let opResize = $state(false);
  let resizeW = $state(0);
  let resizeH = $state(0);
  let opConvert = $state("none"); // none | jpeg | webp | png
  let quality = $state(80);
  let opPng = $state(false);
  let busy = $state(false);

  function isPng(k: string): boolean {
    return k.toLowerCase().endsWith(".png");
  }

  async function load() {
    loading = true;
    try {
      const content = await api.readResource(id);
      key = content.meta.key;
      dataUrl = api.bytesToDataUrl(content.meta.mime, content.dataB64 ?? "");
      error = "";
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function onImgLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    width = img.naturalWidth;
    height = img.naturalHeight;
    resizeW = width;
    resizeH = height;
  }

  function wheelZoom(e: WheelEvent) {
    if (fit) return;
    e.preventDefault();
    zoom = Math.min(8, Math.max(0.1, zoom * (e.deltaY < 0 ? 1.15 : 0.87)));
  }

  async function applyOps() {
    const steps: import("../lib/types").Processor[] = [];
    if (opResize && (resizeW !== width || resizeH !== height)) {
      steps.push({ kind: "img-resize", width: resizeW || null, height: resizeH || null });
    }
    if (opConvert !== "none") {
      steps.push({ kind: "img-convert", format: opConvert, quality });
    }
    if (opPng && isPng(key)) {
      steps.push({ kind: "png-optimize", level: 2 });
    }
    if (steps.length === 0) {
      store.toast("info", "未选择任何操作");
      return;
    }
    busy = true;
    try {
      const reports = await api.runJob<import("../lib/types").StepReport[]>(
        () => api.pipelineApplyStart(steps as never, { ids: [id] })
      );
      const rep = reports[0];
      if (rep.status === "ok") {
        store.toast("ok", `${key}: ${rep.before} → ${rep.after} 字节（-Δ${rep.delta}）`);
        store.bumpOverlay();
        await load();
      } else {
        store.toast("error", rep.message || "无收益或不可应用");
      }
    } catch (e) {
      store.toast("error", String(e));
    } finally {
      busy = false;
    }
  }

  async function revert() {
    try {
      await api.revertResource(id);
      store.toast("ok", "已还原到原始内容");
      store.bumpOverlay();
      await load();
    } catch (e) {
      store.toast("error", String(e));
    }
  }

  onMount(load);
</script>

<div class="image-editor">
  <div class="canvas" class:fit onwheel={wheelZoom}>
    {#if loading}
      <p class="hint">加载中…</p>
    {:else if error}
      <p class="hint error">{error}</p>
    {:else}
      <img
        src={dataUrl}
        alt={key}
        onload={onImgLoad}
        style:width={fit ? "auto" : `${width * zoom}px`}
        style:max-width={fit ? "100%" : "none"}
        style:max-height={fit ? "100%" : "none"}
      />
    {/if}
  </div>
  <aside class="ops">
    <h3>{key}</h3>
    <p class="meta">{width} × {height} px</p>

    <label class="row">
      <input type="checkbox" bind:checked={opResize} />
      调整尺寸
    </label>
    {#if opResize}
      <div class="row indented">
        <label>宽 <input type="number" bind:value={resizeW} min="1" /></label>
        <label>高 <input type="number" bind:value={resizeH} min="1" /></label>
        <button onclick={() => { resizeH = Math.round((resizeW / width) * height); }}>等比</button>
      </div>
    {/if}

    <label class="row">
      <input type="checkbox" bind:checked={opPng} disabled={!isPng(key)} />
      PNG 无损优化 (oxipng)
    </label>

    <div class="row">
      <label>
        <input type="checkbox" checked={opConvert !== "none"}
          onchange={(e) => (opConvert = e.currentTarget.checked ? "jpeg" : "none")} />
        转换格式
      </label>
      {#if opConvert !== "none"}
        <select bind:value={opConvert}>
          <option value="jpeg">JPEG</option>
          <option value="webp">WebP（无损）</option>
          <option value="png">PNG</option>
        </select>
      {/if}
    </div>
    {#if opConvert === "jpeg"}
      <div class="row indented">
        <label>质量 {quality}</label>
        <input type="range" min="30" max="95" bind:value={quality} />
      </div>
    {/if}

    <div class="actions">
      <button class="primary" disabled={busy} onclick={applyOps}>{busy ? "处理中…" : "应用"}</button>
      <button disabled={busy} onclick={revert}>还原原始</button>
    </div>
    <p class="tips">Ctrl+滚轮缩放（非适应模式）</p>
  </aside>
</div>

<style>
  .image-editor {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .canvas {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: auto;
    background:
      repeating-conic-gradient(var(--bg-hover) 0% 25%, var(--bg-app) 0% 50%) 50% / 24px 24px;
    min-width: 0;
  }
  .canvas img { transition: max-width 0.15s, max-height 0.15s; }
  .ops {
    width: 240px;
    flex-shrink: 0;
    border-left: 1px solid var(--border-subtle);
    padding: 12px;
    overflow-y: auto;
    font-size: 13px;
  }
  .ops h3 {
    margin: 0 0 4px;
    font-size: 13px;
    word-break: break-all;
  }
  .meta { color: var(--text-3); margin: 0 0 12px; font-size: 12px; }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px 0;
  }
  .row.indented { margin-left: 24px; flex-wrap: wrap; }
  .row label { display: flex; align-items: center; gap: 4px; }
  .row input[type="number"] { width: 64px; }
  .ops input, .ops select, .ops button {
    font: inherit;
    font-size: 12px;
  }
  .ops input[type="text"], .ops input[type="number"], .ops select {
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    padding: 3px 6px;
    background: var(--bg-pane);
    color: var(--text-1);
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
  .actions button {
    flex: 1;
    padding: 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-pane);
    cursor: pointer;
  }
  .actions .primary {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }
  .actions button:disabled { opacity: 0.5; cursor: default; }
  .tips { color: var(--text-3); font-size: 11px; margin-top: 12px; }
  .hint { color: var(--text-3); }
  .hint.error { color: var(--danger); }
</style>
