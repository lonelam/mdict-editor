// Mirrors the Rust types in src-tauri/src (lib.rs, registry.rs, resolver.rs,
// pipeline.rs, export.rs). Keep in sync.

export type ResourceId =
  | { t: "mdx"; source: number; ordinal: number }
  | { t: "mdd"; source: number; ordinal: number }
  | { t: "ext"; file: number };

export type Category =
  | "entry" | "html" | "css" | "js" | "image"
  | "audio" | "video" | "font" | "text" | "other";

export interface SourceInfo {
  id: number;
  kind: "mdx" | "mdd" | "ext";
  name: string;
  title: string | null;
  entryCount: number;
}

export interface ResourceMeta {
  id: ResourceId;
  key: string;
  category: Category;
  mime: string;
  edited: boolean;
  deleted: boolean;
  sourceName: string;
  sizeCurrent: number | null;
}

export interface CategoryStat {
  category: Category;
  count: number;
  edited: number;
}

export interface ResourceContent {
  meta: ResourceMeta;
  text: string | null;
  dataB64: string | null;
  sizeCurrent: number;
  sizeOriginal: number | null;
  historyLen: number;
}

export interface RevisionInfo {
  historyLen: number;
  sizeOriginal: number;
  sizeCurrent: number;
}

export type ResolveOutcome =
  | { status: "found"; target: ResourceId; basis: string }
  | { status: "ambiguous"; candidates: ResourceId[]; keys: string[] }
  | { status: "externalRef" }
  | { status: "notFound" };

export type Processor =
  | { kind: "minify-js" }
  | { kind: "minify-css" }
  | { kind: "minify-html" }
  | { kind: "png-optimize"; level?: number | null }
  | { kind: "img-convert"; format: string; quality?: number | null }
  | { kind: "img-resize"; width?: number | null; height?: number | null };

export interface Scope {
  all?: boolean;
  source?: number | null;
  category?: Category | null;
  ids?: ResourceId[] | null;
}

export interface StepReport {
  id: ResourceId;
  key: string;
  before: number;
  after: number;
  delta: number;
  status: "ok" | "skip" | "error";
  message: string;
}

export interface ExportConfig {
  outDir: string;
  mdx: boolean;
  mdd: boolean;
  embedExternals: boolean;
  embedTarget: number | null;
  saveExternals: boolean;
  /** Lossy chain applied only to the `.lossy.mdd` copy; originals unaffected. */
  lossy: Processor[] | null;
}

export interface ExportedFile {
  path: string;
  entries: number;
  bytes: number;
  checkOk: boolean;
  message: string;
}

export interface ExportReport {
  files: ExportedFile[];
  skippedExternals: string[];
  ok: boolean;
}

export function idKey(id: ResourceId): string {
  return id.t === "ext" ? `ext-${id.file}` : `${id.t}-${id.source}-${id.ordinal}`;
}

export function parseIdKey(key: string): ResourceId {
  const [t, a, b] = key.split("-");
  if (t === "ext") return { t: "ext", file: Number(a) };
  if (t === "mdx") return { t: "mdx", source: Number(a), ordinal: Number(b) };
  return { t: "mdd", source: Number(a), ordinal: Number(b) };
}
