<script lang="ts">
  import { store } from "../lib/store.svelte";
  import CodeEditor from "./CodeEditor.svelte";
  import ImageEditor from "./ImageEditor.svelte";
  import MediaViewer from "./MediaViewer.svelte";
  import FontViewer from "./FontViewer.svelte";
  import HexViewer from "./HexViewer.svelte";

  const tab = $derived(store.activeTab);
</script>

<section class="editor-host">
  {#if !tab}
    <div class="empty">
      <p>从左侧资源树选择一个资源开始</p>
      <p class="sub">文本类资源支持 Ctrl+点击 跳转引用（如 <code>src="/img/logo.png"</code>）</p>
      <p class="sub">
        编辑结果可与 <a
          href="https://github.com/lonelam/aalookup"
          target="_blank"
          rel="noreferrer">AALookup</a> 无缝衔接——同源同构的词典阅读器，
        导出后一键导入即可用阅读视角检验编辑效果。
      </p>
    </div>
  {:else if tab.meta.category === "entry" || tab.meta.category === "html"}
    {#key tab.key}
      <CodeEditor id={tab.meta.id} language="html" withPreview />
    {/key}
  {:else if tab.meta.category === "css"}
    {#key tab.key}
      <CodeEditor id={tab.meta.id} language="css" />
    {/key}
  {:else if tab.meta.category === "js"}
    {#key tab.key}
      <CodeEditor id={tab.meta.id} language="js" />
    {/key}
  {:else if tab.meta.category === "text"}
    {#key tab.key}
      <CodeEditor id={tab.meta.id} language="text" />
    {/key}
  {:else if tab.meta.category === "image"}
    {#key tab.key}
      <ImageEditor id={tab.meta.id} />
    {/key}
  {:else if tab.meta.category === "audio" || tab.meta.category === "video"}
    {#key tab.key}
      <MediaViewer id={tab.meta.id} kind={tab.meta.category} />
    {/key}
  {:else if tab.meta.category === "font"}
    {#key tab.key}
      <FontViewer id={tab.meta.id} />
    {/key}
  {:else}
    {#key tab.key}
      <HexViewer id={tab.meta.id} />
    {/key}
  {/if}
</section>

<style>
  .editor-host {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg-app);
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--text-3);
    font-size: 13px;
    gap: 4px;
  }
  .empty p { margin: 0; }
  .sub { font-size: 12px; }
  .sub code {
    background: var(--bg-hover);
    padding: 1px 5px;
    border-radius: 4px;
    font-family: var(--font-code);
    font-size: 11px;
  }
</style>
