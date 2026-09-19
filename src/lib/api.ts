// Typed invoke wrappers around the Tauri commands.
import { invoke } from "@tauri-apps/api/core";
import type {
  Category, ExportConfig, ExportReport, Processor, ResolveOutcome,
  ResourceContent, ResourceId, ResourceMeta, RevisionInfo, Scope, SourceInfo,
  CategoryStat, StepReport,
} from "./types";

export async function openSources(paths: string[]): Promise<SourceInfo[]> {
  return invoke("open_sources", { paths });
}

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

export async function pipelineDryRun(
  steps: Processor[],
  scope: Scope
): Promise<StepReport[]> {
  return invoke("pipeline_dry_run", { steps, scope });
}

export async function pipelineApply(
  steps: Processor[],
  scope: Scope
): Promise<StepReport[]> {
  return invoke("pipeline_apply", { steps, scope });
}

export async function exportBuild(config: ExportConfig): Promise<ExportReport> {
  return invoke("export_build", { config });
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
