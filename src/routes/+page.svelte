<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "../lib/store.svelte";
  import ExplorerPane from "../components/ExplorerPane.svelte";
  import TabBar from "../components/TabBar.svelte";
  import EditorHost from "../components/EditorHost.svelte";
  import InspectorPane from "../components/InspectorPane.svelte";
  import StatusBar from "../components/StatusBar.svelte";
  import Toasts from "../components/Toasts.svelte";
  import PipelineDialog from "../components/PipelineDialog.svelte";
  import ExportDialog from "../components/ExportDialog.svelte";

  let showPipeline = $state(false);
  let showExport = $state(false);

  function onKeydown(e: KeyboardEvent) {
    // Ctrl+S → save active editor (editors listen for the event too)
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      window.dispatchEvent(new CustomEvent("app-save"));
      return;
    }
    // Ctrl+W → close tab
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "w") {
      e.preventDefault();
      if (store.activeTabKey) store.closeTab(store.activeTabKey);
      return;
    }
    // Alt+←/→ → jump history
    if (e.altKey && e.key === "ArrowLeft") {
      e.preventDefault();
      store.jumpBack();
      return;
    }
    if (e.altKey && e.key === "ArrowRight") {
      e.preventDefault();
      store.jumpForward();
      return;
    }
  }

  onMount(() => {
    (window as unknown as { __store: typeof store }).__store = store;
    void store.init();
  });
</script>

<svelte:head>
  <title>MDict Editor</title>
</svelte:head>

<svelte:window onkeydown={onKeydown} />

<div class="shell">
  <header class="toolbar">
    <span class="logo">MDict Editor</span>
    <button onclick={() => store.openFiles()}>打开 / 追加文件</button>
    <span class="sep"></span>
    <button
      onclick={() => (showPipeline = true)}
      disabled={store.sources.length === 0}>处理管线</button>
    <button
      onclick={() => (showExport = true)}
      disabled={store.sources.length === 0}>导出</button>
    <span class="grow"></span>
    <span class="hint">Ctrl+S 保存 · Ctrl+点击 跳转引用 · Alt+←/→ 跳转历史</span>
  </header>

  <div class="main">
    <ExplorerPane />
    <div class="center">
      <TabBar />
      <EditorHost />
    </div>
    <InspectorPane />
  </div>

  <StatusBar />
</div>

{#if showPipeline}
  <PipelineDialog onclose={() => (showPipeline = false)} />
{/if}
{#if showExport}
  <ExportDialog onclose={() => (showExport = false)} />
{/if}
<Toasts />

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
  }
  :global(body) {
    font-family: var(--font-ui);
    font-size: 14px;
    color: var(--text-1);
    background: var(--bg-app);
  }
  :global(#app, .shell) {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-pane);
  }
  .logo {
    font-weight: 700;
    font-size: 13px;
    margin-right: 6px;
    color: var(--accent);
  }
  .toolbar button {
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-1);
    font-size: 12px;
    padding: 5px 12px;
    cursor: pointer;
  }
  .toolbar button:hover:not(:disabled) { background: var(--bg-hover); }
  .toolbar button:disabled { opacity: 0.5; cursor: default; }
  .sep {
    width: 1px;
    height: 18px;
    background: var(--border-subtle);
  }
  .grow { flex: 1; }
  .hint {
    color: var(--text-3);
    font-size: 11px;
  }
  .main {
    flex: 1;
    display: grid;
    grid-template-columns: 300px 1fr 260px;
    min-height: 0;
  }
  .center {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  /* ---- design tokens (§11 of docs/DESIGN.md) ---- */
  :global(:root) {
    --bg-app: #f5f5f7;
    --bg-pane: #ffffff;
    --bg-hover: #f0f0f4;
    --bg-active: #e6e6ec;
    --border-subtle: #e5e5ea;
    --text-1: #1c1c1e;
    --text-2: #48484a;
    --text-3: #8e8e93;
    --accent: #2f6fed;
    --accent-soft: #e6efff;
    --danger: #b3261e;
    --ok: #30a14e;
    --font-ui: system-ui, "Segoe UI", "Microsoft YaHei", sans-serif;
    --font-code: ui-monospace, Consolas, "Courier New", monospace;
    --radius: 6px;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg-app: #1e1e20;
      --bg-pane: #252528;
      --bg-hover: #2e2e32;
      --bg-active: #3a3a40;
      --border-subtle: #3a3a3e;
      --text-1: #f2f2f4;
      --text-2: #b8b8bc;
      --text-3: #7c7c82;
      --accent: #5b8ff5;
      --accent-soft: #2a3a58;
      --danger: #e5695f;
      --ok: #58b878;
    }
  }
</style>
