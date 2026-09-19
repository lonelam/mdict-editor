<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../lib/api";
  import type { ResourceId } from "../lib/types";

  let { id, kind }: { id: ResourceId; kind: "audio" | "video" } = $props();

  let url = $state("");
  let key = $state("");
  let size = $state(0);

  onMount(async () => {
    const content = await api.readResource(id);
    key = content.meta.key;
    size = content.sizeCurrent;
    url = api.bytesToDataUrl(content.meta.mime, content.dataB64 ?? "");
  });

  function sizeLabel(n: number): string {
    return n > 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MB` : `${(n / 1024).toFixed(1)} KB`;
  }
</script>

<div class="media-viewer">
  <h3>{key}</h3>
  <p class="meta">{kind} · {sizeLabel(size)}</p>
  {#if kind === "audio"}
    <audio controls src={url}></audio>
  {:else}
    <video controls src={url}></video>
  {/if}
  <p class="note">音视频编辑器（占位）——当前仅支持预览</p>
</div>

<style>
  .media-viewer {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 24px;
  }
  h3 { margin: 0; font-size: 14px; word-break: break-all; text-align: center; }
  .meta { color: var(--text-3); font-size: 12px; margin: 0; }
  audio, video { max-width: 90%; }
  .note { color: var(--text-3); font-size: 12px; }
</style>
