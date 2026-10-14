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
                if (u.docChanged && !refreshing) store.setDraft(editorKey, u.state.doc.toString());
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
    previewNonce = store.overlayVersion;
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
