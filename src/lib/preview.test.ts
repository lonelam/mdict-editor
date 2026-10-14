import { describe, expect, it, vi } from "vitest";
import { buildPreview } from "./preview";
import { selectionBridge } from "./preview-bridge";
import { decodeEntityText, decodeWithLocations, detectEntityFormat, encodeEntityText } from "./entities";

describe("entity text editing", () => {
  it("decodes named, decimal and hex entities and preserves unknown names", () => {
    expect(decodeEntityText("&#20013;&#x6587;&amp;&nbsp;&unknown;")).toBe("中文&\u00a0&unknown;");
    expect(detectEntityFormat("&#x4E2D;")).toBe("hex");
    expect(detectEntityFormat("&#20013;")).toBe("decimal");
  });
  it("roundtrips Chinese, emoji, ampersands and attribute quotes without creating tags", () => {
    const text = '中文😀 <b> & " \' \u00a0';
    for (const format of ["unicode", "hex", "decimal", "named"] as const) {
      const encoded = encodeEntityText(text, format);
      expect(decodeEntityText(encoded)).toBe(text);
      expect(encoded).not.toContain("<b>");
    }
    expect(encodeEntityText("😀", "hex")).toBe("&#x1f600;");
    expect(encodeEntityText("中", "decimal")).toBe("&#20013;");
  });
  it("maps whole entities, surrogate pairs, missing semicolons and CRLF to source spans", () => {
    const raw = "A&#x4e2d;&#128512;\r\n&amp &notit;";
    const mapped = decodeWithLocations(raw, 10);
    expect(mapped.text).toBe(decodeEntityText(raw).replace("\r\n", "\n"));
    expect(raw.slice(mapped.starts[1] - 10, mapped.ends[1] - 10)).toBe("&#x4e2d;");
    expect(mapped.starts[2]).toBe(mapped.starts[3]);
    expect(mapped.ends[2]).toBe(mapped.ends[3]);
    expect(raw.slice(mapped.starts[4] - 10, mapped.ends[4] - 10)).toBe("\r\n");
  });
});

describe("preview source mapping", () => {
  it("runs as independent JavaScript and forwards Ctrl+S from the iframe", () => {
    const listeners = new Map<string, (event: unknown) => void>();
    const postMessage = vi.fn(), preventDefault = vi.fn();
    const run = new Function("window", "document", `(${selectionBridge})({nonce: 'test', mappings: []});`);
    run({ parent: { postMessage } }, { addEventListener: (name: string, callback: (e: unknown) => void) => listeners.set(name, callback) });
    listeners.get("keydown")!({ ctrlKey: true, key: "s", preventDefault });
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(postMessage).toHaveBeenCalledWith({ type: "mdict-preview-save", nonce: "test" }, "*");
  });
  it("distinguishes duplicate text nodes and excludes scripts and comments", () => {
    const text = '<p>相同</p><!--相同--><script>const x="相同";</script><p>相同</p>';
    const preview = buildPreview(text, "http://mdres.localhost/preview/mdx-0-0/", [], "test");
    const nodes = preview.mappings.filter((m) => m.text === "相同");
    expect(nodes).toHaveLength(2);
    expect(nodes[0].starts[0]).toBe(text.indexOf("相同"));
    expect(nodes[1].starts[0]).toBe(text.lastIndexOf("相同"));
    expect(nodes[0].parent).not.toBe(nodes[1].parent);
  });
  it("maps nested text and encoded characters back to the unchanged source", () => {
    const text = "<p>前<b>&#x4e2d;&#25991;😀</b>后</p>";
    const preview = buildPreview(text, "mdres://localhost/preview/mdx-0-0/", ["mdres://localhost/style.css"], "n");
    const mapping = preview.mappings.find((m) => m.text === "中文😀")!;
    expect(text.slice(mapping.starts[0], mapping.ends[1])).toBe("&#x4e2d;&#25991;");
    expect(text.slice(mapping.starts[2], mapping.ends[3])).toBe("😀");
    expect(preview.html).toContain('<base href="mdres://localhost/preview/mdx-0-0/">');
    expect(preview.html).toContain('href="mdres://localhost/style.css"');
    expect(text).not.toContain("data-mdict-preview-node");
  });
  it("supports browser-inserted table structure and preformatted newlines", () => {
    const text = "<table><tr><td>词条</td></tr></table><pre>\nA\r\nB</pre>";
    const preview = buildPreview(text, "http://mdres.localhost/", [], "n");
    expect(preview.html).toContain("<tbody");
    expect(preview.mappings.some((m) => m.text === "词条")).toBe(true);
    const pre = preview.mappings.find((m) => m.text === "A\nB")!;
    expect(text.slice(pre.starts[1], pre.ends[1])).toBe("\r\n");
  });
  it("does not let source text close the injected bridge script", () => {
    const preview = buildPreview('<p>&lt;/script&gt;&lt;script&gt;alert(1)</p>', "http://mdres.localhost/", [], "n");
    expect(preview.html).toContain("\\u003c/script>");
    expect(preview.html.match(/<script>/g)).toHaveLength(1);
  });
});
