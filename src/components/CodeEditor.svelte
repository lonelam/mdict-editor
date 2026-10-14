<script lang="ts">
  import { onMount, tick } from "svelte";
  import { EditorView } from "@codemirror/view";
  import { Compartment, EditorState, type Extension } from "@codemirror/state";
  import { basicSetup } from "codemirror";
  import { html } from "@codemirror/lang-html";
  import { css } from "@codemirror/lang-css";
  import { javascript } from "@codemirror/lang-javascript";
  import { keymap } from "@codemirror/view";
  import { indentWithTab } from "@codemirror/commands";
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { referenceLinks } from "../lib/cm/reflinks";
  import { buildPreview } from "../lib/preview";
  import EntityTextDialog from "./EntityTextDialog.svelte";
  import { idKey, type ResourceId } from "../lib/types";

  let {
    id,
    language = "text",
    withPreview = false,
  }: { id: ResourceId; language?: "html" | "css" | "js" | "text"; withPreview?: boolean } =
    $props();

  let container = $state.raw<HTMLDivElement>();
  let view = $state.raw<EditorView | null>(null);
  let loading = $state(true);
  let error = $state("");
  let documentText = $state("");
  let previewHtml = $state("");
  let previewFrame = $state.raw<HTMLIFrameElement>();
  let previewSource = $state<api.PreviewContent | null>(null);
  let previewToken = "";
  let previewError = $state("");
  let locatedAt = $state("");
  let hasSelection = $state(false);
  let entitySelection = $state<{ from: number; to: number; original: string } | null>(null);
  let splitPos = $state(50); // percentage
  let keyPath = $state("");
  let disposed = false;
  const readOnly = new Compartment();
  const editable = (deleted: boolean) => [EditorState.readOnly.of(deleted), EditorView.editable.of(!deleted)];

  const editorKey = $derived(idKey(id));

  onMount(() => {
    void (async () => {
      try {
        const content = await api.readResource(id);
        if (disposed) return;
        const text = store.drafts.get(editorKey) ?? content.text ?? "";
        documentText = text;
        keyPath = content.meta.key;
        loading = false;
        // Wait for the {#else} branch to mount `container` before attaching.
        await tick();
        if (disposed || !container) return;

        const langExt: Extension[] =
          language === "html"
            ? [html()]
            : language === "css"
              ? [css()]
              : language === "js"
                ? [javascript()]
                : [];

        view = new EditorView({
          parent: container,
          state: EditorState.create({
            doc: text,
            extensions: [
              basicSetup,
              readOnly.of(editable(content.meta.deleted)),
              ...langExt,
              keymap.of([indentWithTab]),
              referenceLinks((ref) => store.navigateRef(ref, id)),
              EditorView.lineWrapping,
              EditorView.updateListener.of((u) => {
                if (u.docChanged) {
                  locatedAt = "";
                  documentText = u.state.doc.toString();
                  if (!refreshing) store.setDraft(editorKey, documentText);
                }
                hasSelection = !u.state.selection.main.empty;
              }),
            ],
          }),
        });
      } catch (e) {
        error = String(e);
        loading = false;
      }
    })();

    return () => {
      disposed = true;
      view?.destroy();
      view = null;
    };
  });

  // Saved changes in any tab refresh preview resources. Undo/revert also
  // refresh the text when there is no unsaved draft to preserve.
  $effect(() => {
    void store.overlayVersion;
    let cancelled = false;
    void api.readResource(id).then((content) => {
      if (cancelled || disposed || !view || store.drafts.get(editorKey) !== undefined) return;
      const text = content.text ?? "";
      view.dispatch({ effects: readOnly.reconfigure(editable(content.meta.deleted)) });
      if (view.state.doc.toString() !== text) {
        refreshing = true;
        view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
        refreshing = false;
      }
    }).catch(() => {});
    return () => { cancelled = true; };
  });
  let refreshing = false;

  $effect(() => {
    const text = documentText;
    void store.overlayVersion;
    if (!withPreview || loading) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      void api.previewContent(id, text).then((content) => {
        if (cancelled || disposed) return;
        const base = api.mdresUrl(content.target, content.key);
        const origin = base.split("/preview/")[0];
        const styles = content.styles.map((key) => `${origin}/${key.replace(/\\/g, "/").replace(/^\/+/, "").split("/").map(encodeURIComponent).join("/")}`);
        const token = crypto.randomUUID();
        const prepared = buildPreview(content.text, base, styles, token);
        previewToken = token;
        previewSource = content;
        previewHtml = prepared.html;
        previewError = "";
      }).catch((e) => { if (!cancelled) previewError = String(e); });
    }, 200);
    return () => { cancelled = true; clearTimeout(timer); };
  });

  function selectCode(from: number, to: number, expectedText: string) {
    if (!view) return;
    if (view.state.doc.toString() !== expectedText) {
      store.toast("info", "代码已变化，请等待预览更新后重新选择文字"); return;
    }
    view.dispatch({ selection: { anchor: from, head: to }, effects: EditorView.scrollIntoView(from, { y: "center" }) });
    const line = view.state.doc.lineAt(from);
    locatedAt = `已定位：第 ${line.number} 行，第 ${from - line.from + 1} 列`;
  }

  $effect(() => {
    const selection = store.codeSelection;
    if (!view || selection?.key !== editorKey) return;
    selectCode(selection.from, selection.to, selection.text);
    store.codeSelection = null;
  });

  function previewMessage(event: MessageEvent) {
    const data = event.data;
    if (event.source !== previewFrame?.contentWindow || !data || data.nonce !== previewToken || !previewSource) return;
    if (data.type === "mdict-preview-save") { void store.saveCurrent(); return; }
    if (data.type === "mdict-preview-unmapped") {
      store.toast("info", "选中的文字由脚本生成或页面结构已改变，无法精确定位；请在代码中搜索"); return;
    }
    const { from, to } = data;
    if (data.type !== "mdict-preview-selection" || !Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to <= from || to > previewSource.text.length) return;
    if (idKey(previewSource.target) === editorKey) selectCode(from, to, previewSource.text);
    else void store.locateCode(previewSource.target, from, to, previewSource.text);
  }

  function openEntityTool() {
    if (!view || view.state.readOnly) return;
    const { from, to } = view.state.selection.main;
    if (from === to) return;
    const original = view.state.doc.sliceString(from, to);
    if (/<\/?[a-z][^>]*>/i.test(original)) {
      store.toast("info", "请选择文字或属性值，不要包含 HTML 标签"); return;
    }
    entitySelection = { from, to, original };
  }

  function applyEntityText(text: string) {
    if (!view || !entitySelection) return;
    const { from, to, original } = entitySelection;
    if (view.state.doc.sliceString(from, to) !== original) {
      store.toast("info", "原选择已变化，请重新打开实体文本工具"); entitySelection = null; return;
    }
    view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from, head: from + text.length } });
    entitySelection = null;
    view.focus();
  }

  let dragging = false;
  function startDrag(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
  }
  function onMove(e: MouseEvent) {
    if (!dragging) return;
    const rect = container!.parentElement!.getBoundingClientRect();
    splitPos = Math.min(90, Math.max(10, ((e.clientX - rect.left) / rect.width) * 100));
  }
  function stopDrag() {
    dragging = false;
  }

</script>

<svelte:window onmessage={previewMessage} />

<svelte:document
  onmousemove={onMove}
  onmouseup={stopDrag}
/>

<div class="editor-workspace">
  <div class="editor-tools">
    <button onclick={() => store.saveCurrent()} disabled={loading || store.saving || store.projectBusy}>保存全部 (Ctrl+S)</button>
    {#if language === "html"}
      <button onclick={openEntityTool} disabled={!hasSelection || view?.state.readOnly}>实体文本工具</button>
    {/if}
    {#if withPreview}<span>在右侧选中文字，左侧自动定位代码</span>{/if}
    {#if locatedAt}<span>{locatedAt}</span>{/if}
    {#if previewSource?.redirects.length}<span>重定向预览：{previewSource.redirects.join(" → ")}</span>{/if}
  </div>
<div class="code-editor" class:with-preview={withPreview}>
  {#if loading}
    <p class="status">加载中…</p>
  {:else if error}
    <p class="status error">{error}</p>
  {:else}
    <div class="code-pane" bind:this={container} style:width={withPreview ? `${splitPos}%` : "100%"}></div>
    {#if withPreview}
      <button class="divider" aria-label="调整代码和预览宽度" onmousedown={startDrag} onkeydown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault(); splitPos = Math.min(90, Math.max(10, splitPos + (e.key === "ArrowLeft" ? -5 : 5)));
        }
      }}></button>
      <div class="preview-pane">
        {#if previewError}<p class="status error">{previewError}</p>{/if}
        <iframe
          bind:this={previewFrame}
          class="preview"
          title="preview"
          srcdoc={previewHtml}
          sandbox="allow-scripts"
        ></iframe>
      </div>
    {/if}
  {/if}
</div>
</div>

{#if entitySelection}
  <EntityTextDialog original={entitySelection.original} onapply={applyEntityText} onclose={() => (entitySelection = null)} />
{/if}

<style>
  .editor-workspace { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .editor-tools { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding: 6px 10px; border-bottom: 1px solid var(--border-subtle); }
  .editor-tools button { padding: 4px 10px; border: 1px solid var(--border-subtle); border-radius: 4px; background: var(--bg-pane); color: var(--text-1); cursor: pointer; font-size: 12px; }
  .editor-tools button:disabled { opacity: 0.5; cursor: default; }
  .editor-tools span { font-size: 11px; color: var(--text-2); }
  .code-editor {
    display: flex;
    flex: 1;
    min-height: 0;
    background: var(--bg-app);
  }
  .code-pane {
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .code-pane :global(.cm-editor) {
    flex: 1;
    height: 100%;
    font-size: 13px;
  }
  .code-pane :global(.cm-scroller) {
    font-family: var(--font-code);
    line-height: 1.6;
  }
  .divider {
    border: 0;
    padding: 0;
    width: 4px;
    cursor: col-resize;
    background: var(--border-subtle);
    flex-shrink: 0;
  }
  .divider:hover { background: var(--accent); }
  .preview-pane {
    flex: 1;
    min-width: 0;
    display: flex;
    background: #ffffff;
  }
  .preview {
    flex: 1;
    border: none;
    width: 100%;
  }
  .status {
    padding: 24px;
    color: var(--text-3);
    font-size: 13px;
  }
  .status.error { color: var(--danger); }

  /* Reference link affordances: always visible; Ctrl+Click to jump. */
  .code-pane :global(.cm-res-ref) {
    text-decoration: underline dashed var(--accent);
    text-underline-offset: 3px;
    cursor: pointer;
    color: var(--accent);
  }
  .code-pane :global(.cm-res-ref:hover) {
    background: var(--accent-soft);
  }
</style>
