<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { decodeEntityText, detectEntityFormat, encodeEntityText, type EntityFormat } from "../lib/entities";
  let { original, onapply, onclose }: { original: string; onapply: (text: string) => void; onclose: () => void } = $props();
  let decoded = $state(untrack(() => decodeEntityText(original)));
  let format = $state<EntityFormat>(untrack(() => detectEntityFormat(original)));
  const output = $derived(encodeEntityText(decoded, format));
  let input = $state.raw<HTMLTextAreaElement>();
  onMount(() => input?.focus());
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") onclose(); }} />

<div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="entity-title" tabindex="-1">
    <h2 id="entity-title">实体文本工具</h2>
    <p>选中的实体已解码为文字。修改后选择编码格式，写回代码，再按 Ctrl+S。</p>
    <label>解码后的文字<textarea bind:this={input} rows="5" bind:value={decoded} spellcheck="false"></textarea></label>
    <label>写回格式<select bind:value={format}>
      <option value="unicode">可读文字（Unicode）</option>
      <option value="hex">十六进制实体（&#38;#x4e2d;）</option>
      <option value="decimal">十进制实体（&#38;#20013;）</option>
      <option value="named">HTML 实体（命名优先）</option>
    </select></label>
    <label>写回代码预览<textarea rows="4" value={output} readonly spellcheck="false"></textarea></label>
    <footer><button onclick={onclose}>取消</button><button class="primary" onclick={() => onapply(output)}>写回代码</button></footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; z-index: 100; background: #0006; display: grid; place-items: center; }
  .dialog { width: min(640px, 90vw); padding: 20px; border-radius: 8px; background: var(--bg-pane); box-shadow: 0 8px 32px #0003; }
  h2 { margin: 0 0 8px; font-size: 16px; } p { font-size: 12px; color: var(--text-2); }
  label { display: flex; flex-direction: column; gap: 6px; margin-top: 12px; font-size: 12px; }
  textarea, select { padding: 8px; border: 1px solid var(--border-subtle); border-radius: 4px; background: var(--bg-app); color: var(--text-1); }
  textarea { resize: vertical; font-family: var(--font-code); }
  footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  button { padding: 6px 14px; cursor: pointer; border: 1px solid var(--border-subtle); border-radius: 4px; background: var(--bg-app); color: var(--text-1); }
  .primary { background: var(--accent); color: white; }
</style>
