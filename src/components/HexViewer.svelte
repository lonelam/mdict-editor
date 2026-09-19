<script lang="ts">
  import { onMount } from "svelte";
  import * as api from "../lib/api";
  import type { ResourceId } from "../lib/types";

  let { id }: { id: ResourceId } = $props();

  let key = $state("");
  let dump = $state("");
  let total = $state(0);
  let error = $state("");
  const CHUNK = 4096; // bytes per page shown

  function hexdump(bytes: Uint8Array, base: number): string {
    const lines: string[] = [];
    for (let off = 0; off < bytes.length; off += 16) {
      const row = bytes.subarray(off, off + 16);
      const hex = Array.from(row)
        .map((b) => b.toString(16).padStart(2, "0"))
        .join(" ")
        .padEnd(47, " ");
      const ascii = Array.from(row)
        .map((b) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : "."))
        .join("");
      lines.push(`${(base + off).toString(16).padStart(8, "0")}  ${hex}  ${ascii}`);
    }
    return lines.join("\n");
  }

  async function load(offset = 0) {
    try {
      const content = await api.readResource(id);
      key = content.meta.key;
      total = content.sizeCurrent;
      const raw = atob(content.dataB64 ?? "");
      const bytes = new Uint8Array(raw.length);
      for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
      const page = bytes.subarray(offset, offset + CHUNK);
      dump = hexdump(page, offset);
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => void load(0));
</script>

<div class="hex-viewer">
  <header>
    <h3>{key}</h3>
    <span>{total.toLocaleString()} 字节 · 显示前 {CHUNK} 字节 · 只读</span>
  </header>
  {#if error}
    <p class="error">{error}</p>
  {:else}
    <pre>{dump}</pre>
  {/if}
  <p class="note">二进制编辑器（占位）——可用右侧 Inspector 导出此资源</p>
</div>

<style>
  .hex-viewer {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 12px 16px;
    overflow: auto;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 8px;
  }
  h3 { margin: 0; font-size: 13px; }
  header span { color: var(--text-3); font-size: 12px; }
  pre {
    font-family: var(--font-code);
    font-size: 12px;
    line-height: 1.5;
    background: var(--bg-pane);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 12px;
    overflow: auto;
    margin: 0;
  }
  .error { color: var(--danger); font-size: 13px; }
  .note { color: var(--text-3); font-size: 12px; }
</style>
