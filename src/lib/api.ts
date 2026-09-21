// Typed invoke wrappers around the Tauri commands.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Category, ExportConfig, ExportReport, Processor, ResolveOutcome,
  ResourceContent, ResourceId, ResourceMeta, RevisionInfo, Scope, SourceInfo,
  CategoryStat, StepReport,
} from "./types";

export interface OpenResult {
  added: SourceInfo[];
  skipped: string[];
  errors: string[];
}

export async function openSources(paths: string[]): Promise<OpenResult> {
  return invoke("open_sources", { paths });
}

/** Creates a new empty dictionary file on disk and opens it as a source. */
export async function createSource(kind: "mdx" | "mdd", path: string): Promise<SourceInfo> {
  return invoke("create_source", { kind, path });
}

export async function removeSource(id: number): Promise<boolean> {
  return invoke("remove_source", { id });
}

export async function listSources(): Promise<SourceInfo[]> {
  return invoke("list_sources");
}

export interface SourceProps {
  id: number;
  kind: string;
  name: string;
  path: string;
  fileSize: number;
  title: string | null;
  entryCount: number;
  attributes: [string, string][];
}

export async function sourceProps(id: number): Promise<SourceProps> {
  return invoke("source_props", { id });
}

// ---- long-running jobs (progress + cancel via events) ----

interface JobProgress {
  job: string;
  done: number;
  total: number;
  item: string;
}
interface JobDone<T> {
  job: string;
  ok: boolean;
  value?: T;
  error?: string;
}

/**
 * Starts a background job and awaits its completion, surfacing per-item
 * progress. `start` is a THUNK: it is only invoked after the event listeners
 * are registered — an already-fired job can finish and emit `job-done`
 * before a listener attaches, losing the event and hanging the await (an
 * empty export completes in milliseconds). `onJobId` receives the id once
 * the start command settles.
 */
export async function runJob<T>(
  start: () => Promise<string>,
  onProgress?: (done: number, total: number, item: string) => void,
  onJobId?: (id: string) => void
): Promise<T> {
  let jobId: string | null = null;
  let settle: ((v: T) => void) | null = null;
  let fail: ((e: Error) => void) | null = null;
  const result = new Promise<T>((resolve, reject) => {
    settle = resolve;
    fail = reject;
  });
  // A no-op export can finish before `await start` delivers the job id, so
  // early done events are parked and replayed once the id is known.
  let earlyDone: JobDone<T> | null = null as JobDone<T> | null;
  const finish = (p: JobDone<T>, un: () => void) => {
    un();
    if (p.ok) settle?.(p.value as T);
    else fail?.(new Error(p.error ?? "job failed"));
  };
  const unP = await listen<JobProgress>("job-progress", (e) => {
    if (e.payload.job === jobId) {
      onProgress?.(e.payload.done, e.payload.total, e.payload.item);
    }
  });
  let finished = false;
  const unD = await listen<JobDone<T>>("job-done", (e) => {
    if (finished) return;
    if (jobId === null) {
      earlyDone = e.payload; // id not yet known — park it
      return;
    }
    if (e.payload.job !== jobId) return;
    finished = true;
    finish(e.payload, () => {
      unP();
      unD();
    });
  });
  try {
    jobId = await start();
    onJobId?.(jobId);
  } catch (e) {
    unP();
    unD();
    throw e;
  }
  const parked: JobDone<T> | null = earlyDone as JobDone<T> | null;
  if (!finished && parked !== null && parked.job === jobId) {
    finished = true;
    finish(parked, () => {
      unP();
      unD();
    });
  }
  return result;
}

export const pipelineDryRunStart = (steps: Processor[], scope: Scope) =>
  invoke<string>("pipeline_dry_run_start", { steps, scope });
export const pipelineApplyStart = (steps: Processor[], scope: Scope) =>
  invoke<string>("pipeline_apply_start", { steps, scope });
export const exportStart = (config: ExportConfig) =>
  invoke<string>("export_start", { config });
export const cancelJob = (job: string) => invoke<boolean>("cancel_job", { job });

/** Hands exported .mdx files to AALookup (imports + enables them there). */
export const importToAALookup = (paths: string[]) =>
  invoke<string>("import_to_aalookup", { paths });

export interface InsertionInfo {
  index: number;
  kind: "entry" | "resource";
  name: string;
  target: number;
  size: number;
}

export const insertEntry = (source: number, key: string, html: string) =>
  invoke<number>("insert_entry", { source, key, html });

/** Returns [addedCount, perFileErrors]. `pathPrefix` lands inside the MDD. */
export const insertResources = (source: number, paths: string[], pathPrefix: string | null) =>
  invoke<[number, string[]]>("insert_resources", { source, paths, pathPrefix });

export const listInsertions = (source: number | null) =>
  invoke<InsertionInfo[]>("list_insertions", { source: source ?? null });

export const updateInsertion = (index: number, html: string) =>
  invoke<void>("update_insertion", { index, html });

export interface InsertionContent {
  index: number;
  kind: "entry" | "resource";
  name: string;
  /** UTF-8 text for text categories; null for binary resources. */
  text: string | null;
}

export const readInsertion = (index: number) =>
  invoke<InsertionContent>("read_insertion", { index });

/** Renames an entry head (overlay delete+insert; materializes at export). */
export const renameEntry = (id: ResourceId, newKey: string) =>
  invoke<void>("rename_entry", { id, newKey });

export const removeInsertion = (index: number) =>
  invoke<boolean>("remove_insertion", { index });

export async function resourceStats(source: number | null): Promise<CategoryStat[]> {
  return invoke("resource_stats", { source });
}

export async function listResources(opts: {
  source?: number | null;
  category?: Category | null;
  prefix?: string;
  offset?: number;
  limit?: number;
}): Promise<ResourceMeta[]> {
  return invoke("list_resources", {
    source: opts.source ?? null,
    category: opts.category ?? null,
    prefix: opts.prefix ?? "",
    offset: opts.offset ?? 0,
    limit: opts.limit ?? 100,
  });
}

export async function resourceMeta(id: ResourceId): Promise<ResourceMeta> {
  return invoke("resource_meta", { id });
}

export async function readResource(id: ResourceId): Promise<ResourceContent> {
  return invoke("read_resource", { id });
}

export async function writeResource(id: ResourceId, bytes: Uint8Array): Promise<RevisionInfo> {
  return invoke("write_resource", { id, bytes: Array.from(bytes) });
}

export async function undoResource(id: ResourceId): Promise<boolean> {
  return invoke("undo_resource", { id });
}

export async function revertResource(id: ResourceId): Promise<boolean> {
  return invoke("revert_resource", { id });
}

export async function deleteResource(id: ResourceId): Promise<void> {
  return invoke("delete_resource", { id });
}

export async function resolveReference(
  reference: string,
  context: ResourceId | null
): Promise<ResolveOutcome> {
  return invoke("resolve_reference", { reference, context });
}


/**
 * URL for the mdres:// preview protocol. WebView2 (Windows) serves custom
 * schemes at http(s)://<scheme>.localhost; other platforms use the scheme
 * directly. The backend only parses the path, so both forms are equivalent.
 */
export function mdresUrl(id: ResourceId, keyPath: string, nonce = 0): string {
  const token =
    id.t === "ext" ? `ext-${id.file}-0` : `${id.t}-${id.source}-${id.ordinal}`;
  // MDX entries have no storage path — an empty rel makes the preview
  // protocol serve the entry HTML itself.
  const rel = id.t === "mdx" ? "" : keyPath.replace(/^[/\\]+/, "");
  const host = navigator.userAgent.includes("Windows")
    ? "http://mdres.localhost"
    : "mdres://localhost";
  return `${host}/preview/${token}/${encodeURI(rel)}${nonce ? `?v=${nonce}` : ""}`;
}

export function bytesToDataUrl(mime: string, b64: string): string {
  return `data:${mime};base64,${b64}`;
}
