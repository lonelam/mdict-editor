export interface TextMapping {
  parent: string;
  child: number;
  text: string;
  starts: number[];
  ends: number[];
}

/** An independent script string stays intact through production minification. */
export const selectionBridge = String.raw`function(config) {
  const mappings = new Map(config.mappings.map((m) => [m.parent + ":" + m.child, m]));
  const post = (data) => window.parent.postMessage({ ...data, nonce: config.nonce }, "*");
  function report() {
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || !selection.rangeCount || !document.body) return;
    const range = selection.getRangeAt(0);
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    let from, to;
    let previousEnd = -1;
    while (walker.nextNode()) {
      const node = walker.currentNode;
      if (!range.intersectsNode(node) || !node.data.length) continue;
      const start = node === range.startContainer ? range.startOffset : 0;
      const end = node === range.endContainer ? range.endOffset : node.length;
      if (start >= end) continue;
      const parent = node.parentElement;
      const child = parent ? Array.from(parent.childNodes).indexOf(node) : -1;
      const mapping = mappings.get(parent?.getAttribute("data-mdict-preview-node") + ":" + child);
      if (!mapping || mapping.text !== node.data || mapping.starts[start] < previousEnd) {
        post({ type: "mdict-preview-unmapped" }); return;
      }
      from ??= mapping.starts[start];
      to = mapping.ends[end - 1];
      previousEnd = to;
    }
    if (from !== undefined && to !== undefined) post({ type: "mdict-preview-selection", from, to });
  }
  document.addEventListener("mouseup", report);
  document.addEventListener("keyup", (event) => { if (event.shiftKey) report(); });
  document.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault(); post({ type: "mdict-preview-save" });
    }
  });
}`;
