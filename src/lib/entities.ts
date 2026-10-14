import { decodeHTML, encodeHTML } from "entities";
import { DecodingMode, EntityDecoder, htmlDecodeTree } from "entities/decode";

export type EntityFormat = "unicode" | "hex" | "decimal" | "named";
export const decodeEntityText = decodeHTML;
export function detectEntityFormat(text: string): EntityFormat {
  return /&#x[0-9a-f]+;/i.test(text) ? "hex" : /&#[0-9]+;/.test(text) ? "decimal" : /&[a-z][a-z0-9]+;/i.test(text) ? "named" : "unicode";
}

/** Encode plain text, including markup delimiters and attribute quotes. */
export function encodeEntityText(text: string, format: EntityFormat): string {
  if (format === "named") return encodeHTML(text);
  const escapes: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
  return Array.from(text, (char) => {
    if (escapes[char]) return escapes[char];
    const cp = char.codePointAt(0)!;
    return cp > 127 && format !== "unicode"
      ? format === "hex" ? `&#x${cp.toString(16)};` : `&#${cp};`
      : char;
  }).join("");
}

/** Map each rendered UTF-16 code unit to its complete source span. */
export function decodeWithLocations(raw: string, offset = 0) {
  let text = "";
  const starts: number[] = [], ends: number[] = [];
  const append = (value: string, from: number, to: number) => {
    text += value;
    for (let i = 0; i < value.length; i++) { starts.push(offset + from); ends.push(offset + to); }
  };
  for (let i = 0; i < raw.length;) {
    if (raw[i] === "&") {
      const decoded: [number, number][] = [];
      const decoder = new EntityDecoder(htmlDecodeTree, (cp, consumed) => decoded.push([cp, consumed]));
      decoder.startEntity(DecodingMode.Legacy);
      let consumed = decoder.write(raw, i + 1);
      if (consumed < 0) consumed = decoder.end();
      if (consumed > 0 && decoded.length) {
        for (const [cp, length] of decoded) append(String.fromCodePoint(cp), i, i + length);
        i += consumed;
        continue;
      }
    }
    if (raw[i] === "\r") {
      const length = raw[i + 1] === "\n" ? 2 : 1;
      append("\n", i, i + length); i += length;
    } else { append(raw[i], i, i + 1); i++; }
  }
  return { text, starts, ends };
}
