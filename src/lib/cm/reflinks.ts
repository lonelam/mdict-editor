// CodeMirror extension: finds resource references in the syntax tree (HTML
// attribute values, CSS url()/strings, JS string literals) and in whole-body
// `@@@LINK=word` redirect entries, decorates them at all times, and turns
// Ctrl+Click into a resource jump.
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { classifyReference, cleanRefText, parseLinkRedirect } from "../refs";

/** Clickable reference decoration. */
const refMark = Decoration.mark({ class: "cm-res-ref" });

/** One decorated range plus the reference string a click navigates to. */
interface RefHit {
  from: number;
  to: number;
  reference: string;
}

function isJumpable(text: string): boolean {
  const kind = classifyReference(text);
  return kind === "path" || kind === "entry" || kind === "sound";
}

/** Collects reference ranges from the current syntax tree. */
function collectRefs(view: EditorView): RefHit[] {
  const hits: RefHit[] = [];
  const push = (from: number, to: number, raw: string) => {
    const reference = cleanRefText(raw);
    if (isJumpable(reference)) hits.push({ from, to, reference });
  };

  syntaxTree(view.state).iterate({
    enter(node) {
      const name = node.name;
      // HTML attribute values (quoted node includes the quotes).
      if (name === "AttributeValue") {
        push(node.from, node.to, view.state.doc.sliceString(node.from, node.to));
      } else if (name === "String" || name === "StringLiteral") {
        // CSS + JS string literals; filtered to path-looking contents.
        push(node.from, node.to, view.state.doc.sliceString(node.from, node.to));
      } else if (name === "CallExpression" || name === "CallLiteral") {
        // CSS url(...) with an unquoted argument.
        const text = view.state.doc.sliceString(node.from, node.to);
        if (/^url\s*\(/i.test(text.trim())) {
          const inner = text.replace(/^\s*url\s*\(/i, "").replace(/\)\s*$/, "");
          const trimmed = inner.trim();
          if (trimmed && !trimmed.startsWith('"') && !trimmed.startsWith("'")) {
            const offset = text.indexOf(trimmed);
            push(node.from + offset, node.from + offset + trimmed.length, trimmed);
          }
        }
      }
      return undefined;
    },
  });

  // A whole-body `@@@LINK=word` redirect: the target word is jumpable.
  const redirect = parseLinkRedirect(view.state.doc.toString());
  if (redirect) {
    hits.push({ from: redirect.from, to: redirect.to, reference: redirect.reference });
  }
  return hits;
}

export function referenceLinks(onRef: (reference: string) => void) {
  const refsPlugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = decorationsOf(view);
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged) {
          this.decorations = decorationsOf(update.view);
        }
      }
    },
    {
      decorations: (v) => v.decorations,
      eventHandlers: {
        mousedown: (event, view) => {
          const me = event as MouseEvent;
          if (!me.ctrlKey) return false;
          const pos = view.posAtCoords({ x: me.clientX, y: me.clientY });
          if (pos == null) return false;
          const hit = collectRefs(view).find((d) => pos >= d.from && pos <= d.to);
          if (!hit) return false;
          me.preventDefault();
          onRef(hit.reference);
          return true;
        },
      },
    }
  );

  return refsPlugin;
}

function decorationsOf(view: EditorView): DecorationSet {
  return Decoration.set(collectRefs(view).map((h) => refMark.range(h.from, h.to)));
}
