import { parse, serialize, defaultTreeAdapter as adapter, type DefaultTreeAdapterTypes as Tree } from "parse5";
import { decodeWithLocations } from "./entities";
import { selectionBridge, type TextMapping } from "./preview-bridge";

/** Source annotations affect the preview only. Original/exported HTML stays intact. */
export function buildPreview(text: string, base: string, styles: string[], nonce: string) {
  const doc = parse(text, { sourceCodeLocationInfo: true });
  const mappings: TextMapping[] = [];
  let nextId = 0;
  let head: Tree.Element | undefined;
  const walk = (node: Tree.Node, excluded = false) => {
    if (!("childNodes" in node)) return;
    if ("tagName" in node) {
      if (node.tagName === "head") head = node;
      excluded ||= ["head", "script", "style", "noscript", "template"].includes(node.tagName);
      const parent = String(nextId++);
      node.attrs = node.attrs.filter((a) => a.name !== "data-mdict-preview-node");
      node.attrs.push({ name: "data-mdict-preview-node", value: parent });
      if (!excluded) node.childNodes.forEach((child, index) => {
        if (!("value" in child) || !child.sourceCodeLocation) return;
        const { startOffset, endOffset } = child.sourceCodeLocation;
        const mapped = decodeWithLocations(text.slice(startOffset, endOffset), startOffset);
        // Browsers drop the first newline in pre/textarea. Do not guess at
        // other parser corrections or generated text that has no source span.
        if (mapped.text.startsWith("\n") && mapped.text.slice(1) === child.value) {
          mapped.text = mapped.text.slice(1); mapped.starts.shift(); mapped.ends.shift();
        }
        if (mapped.text === child.value) mappings.push({ parent, child: index, ...mapped });
      });
    }
    for (const child of node.childNodes) walk(child, excluded);
  };
  walk(doc);
  if (head) {
    const make = (tag: string, attrs: { name: string; value: string }[]) => adapter.createElement(tag, head!.namespaceURI, attrs);
    // An opaque sandbox prevents dictionary scripts from accessing Tauri IPC.
    // The base URL still routes CSS/images/scripts through the mdres protocol.
    const baseNode = make("base", [{ name: "href", value: base }]);
    const script = make("script", []);
    const payload = JSON.stringify({ nonce, mappings }).replace(/</g, "\\u003c");
    adapter.insertText(script, `(${selectionBridge})(${payload});`);
    adapter.insertBefore(head, script, head.childNodes[0]);
    adapter.insertBefore(head, baseNode, head.childNodes[0]);
    for (const href of styles) adapter.appendChild(head, make("link", [{ name: "rel", value: "stylesheet" }, { name: "href", value: href }]));
  }
  return { html: serialize(doc), mappings };
}
