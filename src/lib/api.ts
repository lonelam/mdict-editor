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

export async function removeSource(id: number): Promise<boolean> {
  return invoke("remove_source", { id });
}

export async function listSources(): Promise<SourceInfo[]> {
  return invoke("list_sources");
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
 * progress. `start` invokes a `*_start` command that returns the job id;
 * progress and completion arrive as `job-progress` / `job-done` events.
 */
export async function runJob<T>(
  start: Promise<string>,
  onProgress?: (done: number, total: number, item: string) => void
): Promise<T> {
  // Listeners are registered before the job id arrives: the backend emits
  // immediately, and `job-done` may even land before `await start` settles.
  let jobId: string | null = null;
  let settle: ((v: T) => void) | null = null;
  let fail: ((e: Error) => void) | null = null;
  const result = new Promise<T>((resolve, reject) => {
    settle = resolve;
    fail = reject;
  });
  const unP = await listen<JobProgress>("job-progress", (e) => {
    if (e.payload.job === jobId) {
      onProgress?.(e.payload.done, e.payload.total, e.payload.item);
    }
  });
  let finished = false;
  const unD = await listen<JobDone<T>>("job-done", (e) => {
    if (e.payload.job !== jobId || finished) return;
    finished = true;
    unP();
    unD();
    if (e.payload.ok) settle?.(e.payload.value as T);
    else fail?.(new Error(e.payload.error ?? "job failed"));
  });
  try {
    jobId = await start;
  } catch (e) {
    unP();
    unD();
    throw e;
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

export interface InsertionInfo {
  index: number;
  kind: "entry" | "resource";
  name: string;
  target: number;
  size: number;
}

export const insertEntry = (source: number, key: string, html: string) =>
  invoke<number>("insert_entry", { source, key, html });

/** Returns [addedCount, perFileErrors]. */
export const insertResources = (source: number, paths: string[]) =>
  invoke<[number, string[]]>("insert_resources", { source, paths });

export const listInsertions = (source: number | null) =>
  invoke<InsertionInfo[]>("list_insertions", { source: source ?? null });

export const updateInsertion = (index: number, html: string) =>
  invoke<void>("update_insertion", { index, html });

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
