<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../lib/api";
  import type { ResourceId } from "../lib/types";

  let { id }: { id: ResourceId } = $props();

  let url = $state("");
  let key = $state("");
  let size = $state(0);

  const SAMPLE = "AaBbCc 0123 中文字典\nThe quick brown fox jumps over the lazy dog.";
  const WEIGHTS = ["normal", "bold"] as const;

  onMount(async () => {
    const content = await api.readResource(id);
    key = content.meta.key;
    size = content.sizeCurrent;
    url = api.bytesToDataUrl(content.meta.mime, content.dataB64 ?? "");
  });

  const fontFaces = $derived(
    WEIGHTS.map((w) => `@font-face { font-family: "DictFont"; font-weight: ${w}; src: url("${url}"); }`)
  );
</script>

<svelte:head>{#key url}<style>{fontFaces.join("\n")}</style>{/key}</svelte:head>

<div class="font-viewer">
  <h3>{key}</h3>
  <p class="meta">{(size / 1024).toFixed(1)} KB · 字体查看器（占位）</p>
  {#each WEIGHTS as w (w)}
    <div class="sample" style:font-weight={w}>
      <div class="big">AaBbCc 字典 123</div>
      <div class="text">{SAMPLE}</div>
    </div>
  {/each}
</div>

<style>
  .font-viewer {
    flex: 1;
    padding: 24px;
    overflow-y: auto;
  }
  h3 { margin: 0 0 4px; font-size: 14px; }
  .meta { color: var(--text-3); font-size: 12px; margin: 0 0 20px; }
  .sample {
    padding: 16px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    margin-bottom: 16px;
    font-family: "DictFont";
  }
  .big { font-size: 40px; margin-bottom: 8px; }
  .text { font-size: 16px; line-height: 1.6; white-space: pre-wrap; }
</style>
