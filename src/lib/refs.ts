// Pure helpers for classifying reference strings found inside resources.
// Used by the CodeMirror reference decorations and tested with vitest.

export type RefKind =
  | "entry"    // entry://word — jump to an MDX entry
  | "sound"    // sound://path — audio resource
  | "path"     // relative or absolute resource path
  | "external" // http(s)/data/mailto — dead in-app, shown dimmed
  | "none";    // not a resource reference

/** Heuristic for bare strings (mostly JS): does this look like a path? */
export function looksLikePath(text: string): boolean {
  const t = text.trim();
  if (t.length === 0 || t.length > 512) return false;
  if (/\s/.test(t)) return false;
  if (t.startsWith("/") || t.startsWith("./") || t.startsWith("../")) return true;
  if (t.startsWith("\\") || /^[a-zA-Z]:\\/.test(t)) return true;
  // Ends with a resource extension and has at least one segment.
  return /\.[a-z0-9]{2,5}$/i.test(t) && /[./\\]/.test(t);
}

export function classifyReference(text: string): RefKind {
  const t = text.trim();
  if (t.length === 0) return "none";
  const scheme = t.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):/);
  if (scheme) {
    const s = scheme[1].toLowerCase();
    if (s === "entry" || s === "mdx") return "entry";
    if (s === "sound") return "sound";
    if (s === "http" || s === "https" || s === "data" || s === "mailto" || s === "ftp") {
      return "external";
    }
    // Single letter = Windows drive letter ("c:\a.png" is a path).
    if (s.length === 1) return looksLikePath(t) ? "path" : "none";
    // Unknown scheme — treat as a resource path; the backend decides.
    return "path";
  }
  return looksLikePath(t) ? "path" : "none";
}

/** Strips surrounding quotes and whitespace from a raw token. */
export function cleanRefText(raw: string): string {
  let t = raw.trim();
  if (
    (t.startsWith('"') && t.endsWith('"') && t.length >= 2) ||
    (t.startsWith("'") && t.endsWith("'") && t.length >= 2)
  ) {
    t = t.slice(1, -1);
  }
  return t.trim();
}

/** Extracts all candidate refs from plain HTML text (tests + fallbacks). */
export function extractHtmlRefs(html: string): string[] {
  const out: string[] = [];
  const re = /\b(src|href|poster|data-src)\s*=\s*["']([^"']+)["']/gi;
  let m: RegExpExecArray | null;
  while ((m = re.exec(html))) out.push(m[2]);
  return out;
}

export interface LinkRedirect {
  /** Target word with any `entry://` prefix stripped. */
  target: string;
  /** Ready-to-navigate reference (`entry://<target>`). */
  reference: string;
  /** Range of the (trimmed) target inside the original text. */
  from: number;
  to: number;
}

/** Matches a whole document that is nothing but an MDict redirect line. */
const REDIRECT_RE = /^([ \t\r\n\uFEFF]*)@@@LINK=([^\r\n]*)\r?\n?$/i;

/**
 * Parses an MDict `@@@LINK=word` redirect entry body: the entire text must be
 * one redirect line (plus optional surrounding whitespace/newline/BOM). The
 * optional `entry://` prefix on the word is accepted and normalized away.
 */
export function parseLinkRedirect(text: string): LinkRedirect | null {
  const m = REDIRECT_RE.exec(text);
  if (!m) return null;
  const group = m[2] ?? "";
  const shown = group.trim();
  const target = shown.replace(/^entry:\/\//i, "").trim();
  if (!target) return null;
  const from =
    (m.index ?? 0) +
    (m[1]?.length ?? 0) +
    "@@@LINK=".length +
    (group.length - group.trimStart().length);
  return { target, reference: `entry://${target}`, from, to: from + shown.length };
}
