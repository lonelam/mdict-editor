<script lang="ts">
  import type { SourceProps } from "../lib/api";

  let { props, onclose }: { props: SourceProps; onclose: () => void } = $props();

  function fmtSize(n: number): string {
    return n > 1024 * 1024 * 1024
      ? `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
      : n > 1024 * 1024
        ? `${(n / 1024 / 1024).toFixed(1)} MB`
        : n > 1024
          ? `${(n / 1024).toFixed(1)} KB`
          : `${n} B`;
  }
  const kindLabel: Record<string, string> = {
    mdx: "MDX 词典（词条）",
    mdd: "MDD 资源包",
    ext: "外部文件",
  };
</script>

<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onclose()} role="presentation">
  <div class="dialog" role="dialog">
    <header>
      <h2>属性 — {props.name}</h2>
      <button class="x" onclick={onclose}>×</button>
    </header>
    <dl>
      <dt>类型</dt><dd>{kindLabel[props.kind] ?? props.kind}</dd>
      <dt>路径</dt><dd class="break">{props.path}</dd>
      <dt>文件大小</dt><dd>{fmtSize(props.fileSize)}</dd>
      {#if props.title}<dt>标题</dt><dd>{props.title}</dd>{/if}
      {#if props.kind !== "ext"}
        <dt>条目数</dt><dd>{props.entryCount.toLocaleString()}</dd>
      {/if}
    </dl>
    {#if props.attributes.length > 0}
      <h3>头部属性（MDict header）</h3>
      <table>
        <tbody>
          {#each props.attributes as [k, v] (k)}
            <tr><td class="k">{k}</td><td class="v">{v}</td></tr>
          {/each}
        </tbody>
      </table>
    {/if}
    <footer><button onclick={onclose}>关闭</button></footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 120;
  }
  .dialog {
    width: min(560px, 92vw);
    max-height: 80vh;
    overflow: auto;
    background: var(--bg-pane);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 16px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  header { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0; font-size: 14px; }
  h3 { margin: 14px 0 6px; font-size: 12px; color: var(--text-2); }
  .x { border: none; background: none; font-size: 16px; cursor: pointer; color: var(--text-3); }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 14px;
    margin: 12px 0 0;
    font-size: 12px;
  }
  dt { color: var(--text-3); }
  dd { margin: 0; word-break: break-all; }
  .break { font-family: var(--font-code); font-size: 11px; }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  td { padding: 3px 8px; border-bottom: 1px solid var(--border-subtle); vertical-align: top; }
  .k { color: var(--text-3); width: 40%; font-family: var(--font-code); font-size: 11px; }
  .v { font-family: var(--font-code); font-size: 11px; word-break: break-all; }
  footer { display: flex; justify-content: flex-end; margin-top: 14px; }
  footer button {
    padding: 6px 16px;
    border: none;
    border-radius: 8px;
    background: var(--accent);
    color: #fff;
    font-size: 12px;
    cursor: pointer;
  }
</style>
