<script lang="ts">
  import { onMount, tick } from "svelte";
  import { EditorView } from "@codemirror/view";
  import { EditorState, type Extension } from "@codemirror/state";
  import { basicSetup } from "codemirror";
  import { html } from "@codemirror/lang-html";
  import { css } from "@codemirror/lang-css";
  import { javascript } from "@codemirror/lang-javascript";
  import { keymap } from "@codemirror/view";
  import { indentWithTab } from "@codemirror/commands";
  import * as api from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { referenceLinks } from "../lib/cm/reflinks";
  import { idKey, type ResourceId } from "../lib/types";

  let {
    id,
    language = "text",
    withPreview = false,
  }: { id: ResourceId; language?: "html" | "css" | "js" | "text"; withPreview?: boolean } =
    $props();

  let container: HTMLDivElement;
  let view: EditorView | null = null;
  let loading = $state(true);
  let error = $state("");
  let previewNonce = $state(0);
  let splitPos = $state(50); // percentage
  let keyPath = $state("");

  const editorKey = $derived(idKey(id));

  onMount(() => {
    void (async () => {
      try {
        const content = await api.readResource(id);
        const text = content.text ?? "";
        keyPath = content.meta.key;
        loading = false;
        // Wait for the {#else} branch to mount `container` before attaching.
        await tick();
        if (!container) return;

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
              ...langExt,
              keymap.of([indentWithTab]),
              referenceLinks((ref) => store.navigateRef(ref, id)),
              EditorView.lineWrapping,
              EditorView.updateListener.of((u) => {
                if (u.docChanged) store.markDirty(editorKey, true);
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
      view?.destroy();
      view = null;
    };
  });

  async function save() {
    if (!view || !store.activeTab?.dirty) return;
    try {
      const text = view.state.doc.toString();
      const bytes = new TextEncoder().encode(text);
      await api.writeResource(id, bytes);
      store.markDirty(editorKey, false);
      store.bumpOverlay();
      previewNonce += 1;
      store.toast("ok", `已保存 ${keyPath}`);
    } catch (e) {
      store.toast("error", `保存失败: ${e}`);
    }
  }

  function onAppSave(e: Event) {
    void e;
    void save();
  }

  // window-level custom event; svelte:window doesn't type custom events.
  $effect(() => {
    window.addEventListener("app-save", onAppSave);
    return () => window.removeEventListener("app-save", onAppSave);
  });

  let dragging = false;
  function startDrag(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
  }
  function onMove(e: MouseEvent) {
    if (!dragging) return;
    const rect = container.parentElement!.getBoundingClientRect();
    splitPos = Math.min(90, Math.max(10, ((e.clientX - rect.left) / rect.width) * 100));
  }
  function stopDrag() {
    dragging = false;
  }

  const previewUrl = $derived(withPreview ? api.mdresUrl(id, keyPath, previewNonce) : "");
</script>

<svelte:document
  onmousemove={onMove}
  onmouseup={stopDrag}
/>

<div class="code-editor" class:with-preview={withPreview}>
  {#if loading}
    <p class="status">加载中…</p>
  {:else if error}
    <p class="status error">{error}</p>
  {:else}
    <div class="code-pane" bind:this={container} style:width={withPreview ? `${splitPos}%` : "100%"}></div>
    {#if withPreview}
      <div class="divider" onmousedown={startDrag}></div>
      <div class="preview-pane">
        <iframe
          class="preview"
          title="preview"
          src={previewUrl}
          sandbox="allow-scripts allow-same-origin"
        ></iframe>
      </div>
    {/if}
  {/if}
</div>

<style>
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

  /* Reference link affordances */
  .code-pane :global(.cm-ctrl-down .cm-res-ref) {
    text-decoration: underline dashed var(--accent);
    text-underline-offset: 3px;
    cursor: pointer;
    color: var(--accent);
  }
  .code-pane :global(.cm-res-ref:hover) {
    background: var(--accent-soft);
  }
</style>
