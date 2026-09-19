// CodeMirror extension: finds resource references in the syntax tree (HTML
// attribute values, CSS url()/strings, JS string literals), decorates them
// while Ctrl is held, and turns Ctrl+Click into a resource jump.
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import type { Range } from "@codemirror/state";
import { classifyReference, cleanRefText } from "../refs";

/** Clickable-while-Ctrl decoration. */
const refMark = Decoration.mark({ class: "cm-res-ref" });

function isJumpable(text: string): boolean {
  const kind = classifyReference(text);
  return kind === "path" || kind === "entry" || kind === "sound";
}

/** Collects reference ranges from the current syntax tree. */
function collectRefs(view: EditorView): Range<Decoration>[] {
  const hits: Range<Decoration>[] = [];
  const push = (from: number, to: number, raw: string) => {
    const text = cleanRefText(raw);
    if (isJumpable(text)) hits.push(refMark.range(from, to));
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
  return hits;
}

export function referenceLinks(onRef: (reference: string) => void) {
  const refsPlugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = Decoration.set(collectRefs(view));
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged) {
          this.decorations = Decoration.set(collectRefs(update.view));
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
          const reference = cleanRefText(view.state.doc.sliceString(hit.from, hit.to));
          onRef(reference);
          return true;
        },
      },
    }
  );

  // Tracks Ctrl so CSS can show the underline affordance.
  const ctrlTracker = ViewPlugin.fromClass(
    class {
      ctrl = false;
      constructor(readonly view: EditorView) {}
      update() {}
      destroy() {
        this.view.contentDOM.classList.remove("cm-ctrl-down");
      }
    },
    {
      eventHandlers: {
        keydown: (event, view) => {
          if ((event as KeyboardEvent).key === "Control") {
            view.contentDOM.classList.add("cm-ctrl-down");
          }
          return false;
        },
        keyup: (event, view) => {
          if ((event as KeyboardEvent).key === "Control") {
            view.contentDOM.classList.remove("cm-ctrl-down");
          }
          return false;
        },
        blur: (_event, view) => {
          view.contentDOM.classList.remove("cm-ctrl-down");
          return false;
        },
      },
    }
  );

  return [refsPlugin, ctrlTracker];
}
