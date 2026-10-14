// Global application state (Svelte 5 runes). Tab management, the jump stack
// (Alt+←/→), selection for the pipeline, toasts and overlay versioning.
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import * as api from "./api";
import { Drafts } from "./drafts";
import {
  idKey, parseIdKey,
  type Category, type CategoryStat, type ResourceId, type ResourceMeta,
  type SourceInfo,
} from "./types";

export interface Tab {
  key: string;
  meta: ResourceMeta;
  dirty: boolean;
}

export interface Toast {
  id: number;
  kind: "info" | "error" | "ok";
  text: string;
}

const PAGE = 100;

export class AppStore {
  sources = $state<SourceInfo[]>([]);
  /** Per-source category stats, keyed by source id. */
  stats = $state<Record<number, CategoryStat[]>>({});
  tabs = $state<Tab[]>([]);
  activeTabKey = $state<string | null>(null);
  /** Increments on every overlay write; drives list/inspector refresh. */
  overlayVersion = $state(0);
  jump = $state<{ stack: string[]; pos: number }>({ stack: [], pos: -1 });
  toasts = $state<Toast[]>([]);
  /** Explorer selection (serialized ids) for batch pipeline scopes. */
  selection = $state<string[]>([]);
  /** Explorer search prefix. */
  search = $state("");
  loading = $state(false);
  projectPath = $state<string | null>(null);
  projectBusy = $state(false);
  changeVersion = $state(0);
  savedVersion = $state(0);
  sessionVersion = $state(0);
  drafts = new Drafts();
  autoSave = $state(true);
  saving = $state(false);
  codeSelection = $state<{ key: string; from: number; to: number; text: string } | null>(null);
  #autoSaveTimer: ReturnType<typeof setTimeout> | undefined;
  #writes: Promise<unknown> = Promise.resolve();
  /** The pending-insertion dialog supplies its current text before a save. */
  saveInsertionDraft: (() => Promise<void>) | null = null;

  get projectDirty(): boolean { return this.changeVersion !== this.savedVersion; }

  #toastId = 0;

  get activeTab(): Tab | null {
    return this.tabs.find((t) => t.key === this.activeTabKey) ?? null;
  }

  toast(kind: Toast["kind"], text: string) {
    const id = ++this.#toastId;
    this.toasts = [...this.toasts, { id, kind, text }];
    setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    }, kind === "error" ? 8000 : 3500);
  }

  // ---- sources ----

  /** Rehydrates sources after an HMR refresh / app reload: the backend keeps
   *  its pool (and overlay), so the frontend just resyncs the list. */
  async init() {
    try {
      const sources = await api.listSources();
      if (sources.length > 0) {
        this.sources = sources;
        this.changeVersion += 1;
        for (const s of sources) void this.refreshStats(s.id);
      }
    } catch {
      // backend not ready (first boot) — openFiles will drive state
    }
  }

  async openFiles() {
    try {
      const paths = await open({
        multiple: true,
        filters: [
          { name: "词典与资源", extensions: ["mdx", "mdd", "js", "css"] },
          { name: "所有文件", extensions: ["*"] },
        ],
      });
      if (!paths) return;
      await this.addSources(paths as string[]);
    } catch (e) {
      this.toast("error", `打开文件失败: ${e}`);
    }
  }

  async addSources(paths: string[]) {
    this.loading = true;
    try {
      const res = await api.openSources(paths);
      this.sources = [...this.sources, ...res.added];
      if (res.skipped.length > 0) {
        this.toast("info", `跳过重复文件: ${res.skipped.join(", ")}`);
      }
      for (const e of res.errors) {
        this.toast("error", e);
      }
      if (res.added.length > 0) {
        this.changeVersion += 1;
        this.toast("ok", `已加载 ${res.added.length} 个文件`);
      }
      for (const s of res.added) void this.refreshStats(s.id);
    } catch (e) {
      this.toast("error", String(e));
    } finally {
      this.loading = false;
      this.scheduleAutoSave();
    }
  }

  /** Creates a new empty .mdx/.mdd at `path` and loads it as a source. */
  async createSource(kind: "mdx" | "mdd", path: string) {
    try {
      const s = await api.createSource(kind, path);
      this.sources = [...this.sources, s];
      this.changeVersion += 1;
      this.scheduleAutoSave();
      void this.refreshStats(s.id);
      this.toast("ok", `已创建并加载 ${s.name}`);
      return s;
    } catch (e) {
      this.toast("error", String(e));
      return null;
    }
  }

  /** Marks a resource deleted (export effect; revert to undo). */
  async markDeleted(id: ResourceId) {
    try {
      await this.saveDrafts(idKey(id));
      await api.deleteResource(id);
      this.closeTab(idKey(id));
      this.toast("ok", "已标记删除（导出生效；可还原）");
      this.bumpOverlay();
    } catch (e) {
      this.toast("error", String(e));
    }
  }

  /** Clears the whole edit chain for one resource. */
  async restoreResource(id: ResourceId) {
    try {
      await api.revertResource(id);
      this.drafts.delete(idKey(id));
      this.markDirty(idKey(id), false);
      this.toast("ok", "已还原");
      this.bumpOverlay();
    } catch (e) {
      this.toast("error", String(e));
    }
  }

  /** Unloads a source (tombstone); closes its tabs, keeps ids stable. */
  async removeSource(id: number) {
    try {
      await api.removeSource(id);
      for (const key of this.drafts.keys()) {
        const resource = parseIdKey(key);
        if ((resource.t === "ext" ? resource.file : resource.source) === id) this.drafts.delete(key);
      }
      this.sources = this.sources.filter((s) => s.id !== id);
      const closed = this.tabs.filter(
        (t) =>
          (t.meta.id.t !== "ext" && t.meta.id.source === id) ||
          (t.meta.id.t === "ext" && t.meta.id.file === id)
      );
      for (const t of closed) this.closeTab(t.key);
      delete this.stats[id];
      this.bumpOverlay();
      this.toast("ok", "已移除（源文件未改动，可重新打开）");
    } catch (e) {
      this.toast("error", String(e));
    }
  }

  async refreshStats(sourceId: number) {
    const session = this.sessionVersion;
    try {
      const stats = await api.resourceStats(sourceId);
      if (session !== this.sessionVersion || !this.sources.some((s) => s.id === sourceId)) return;
      this.stats = { ...this.stats, [sourceId]: stats };
    } catch (e) {
      this.toast("error", String(e));
    }
  }

  async refreshAllStats() {
    for (const s of this.sources) void this.refreshStats(s.id);
  }

  // ---- tabs & jumps ----

  async openResource(id: ResourceId, pushJump = true) {
    if (this.projectBusy) return;
    const session = this.sessionVersion;
    const key = idKey(id);
    let tab = this.tabs.find((t) => t.key === key);
    if (!tab) {
      try {
        const meta = await api.resourceMeta(id);
        if (session !== this.sessionVersion || this.projectBusy) return;
        const newTab: Tab = { key, meta, dirty: this.drafts.get(key) !== undefined };
        this.tabs = [...this.tabs, newTab];
        tab = newTab;
      } catch (e) {
        this.toast("error", String(e));
        return;
      }
    }
    this.activeTabKey = key;
    if (pushJump) this.pushJump(key);
  }

  pushJump(key: string) {
    const { stack, pos } = this.jump;
    if (stack[pos] === key) return;
    this.jump = { stack: [...stack.slice(0, pos + 1), key], pos: pos + 1 };
  }

  jumpBack() {
    if (this.jump.pos > 0) {
      this.jump = { ...this.jump, pos: this.jump.pos - 1 };
      this.activeTabKey = this.jump.stack[this.jump.pos];
    }
  }

  jumpForward() {
    if (this.jump.pos < this.jump.stack.length - 1) {
      this.jump = { ...this.jump, pos: this.jump.pos + 1 };
      this.activeTabKey = this.jump.stack[this.jump.pos];
    }
  }

  closeTab(key: string) {
    const idx = this.tabs.findIndex((t) => t.key === key);
    this.tabs = this.tabs.filter((t) => t.key !== key);
    if (this.activeTabKey === key) {
      this.activeTabKey =
        this.tabs[Math.max(0, idx - 1)]?.key ?? null;
    }
    const stack = this.jump.stack.filter((k) => k !== key);
    const pos = this.jump.stack.slice(0, this.jump.pos + 1).filter((k) => k !== key).length - 1;
    this.jump = { stack, pos };
  }

  markDirty(key: string, dirty: boolean) {
    const tab = this.tabs.find((t) => t.key === key);
    if (tab && tab.dirty !== dirty) tab.dirty = dirty;
  }

  bumpOverlay() {
    this.overlayVersion += 1;
    this.changeVersion += 1;
    void this.refreshAllStats();
    void this.refreshTabs();
    this.scheduleAutoSave();
  }

  setDraft(key: string, text: string) {
    this.drafts.set(key, text);
    this.markDirty(key, true);
    this.changeVersion += 1;
    this.scheduleAutoSave();
  }

  async refreshTabs() {
    const version = this.overlayVersion, session = this.sessionVersion;
    await Promise.all(this.tabs.map(async (tab) => {
      try {
        const meta = await api.resourceMeta(tab.meta.id);
        if (version !== this.overlayVersion || session !== this.sessionVersion || !this.tabs.includes(tab)) return;
        tab.meta.edited = meta.edited;
        tab.meta.deleted = meta.deleted;
        tab.meta.sizeCurrent = meta.sizeCurrent;
      } catch { /* A removed source may finish refreshing after its tabs close. */ }
    }));
  }

  scheduleAutoSave() {
    clearTimeout(this.#autoSaveTimer);
    if (!this.autoSave) return;
    this.#autoSaveTimer = setTimeout(() => { void this.saveCurrent(true); }, 800);
  }

  setAutoSave(enabled: boolean) {
    this.autoSave = enabled;
    this.scheduleAutoSave();
  }

  async saveDrafts(key?: string) {
    const operation = this.#writes.then(async () => {
      try {
        return await this.drafts.save(async (id, text) => {
          await api.writeResource(parseIdKey(id), new TextEncoder().encode(text));
          this.markDirty(id, this.drafts.get(id) !== text);
          this.bumpOverlay();
        }, key);
      } finally {
        for (const tab of this.tabs) this.markDirty(tab.key, this.drafts.get(tab.key) !== undefined);
      }
    });
    this.#writes = operation.catch(() => {});
    return operation;
  }

  /** Ctrl+S and autosave flush all tabs, including closed-tab drafts. */
  async saveCurrent(quiet = false) {
    clearTimeout(this.#autoSaveTimer);
    if (this.projectBusy || this.loading || this.saving) return;
    if (this.projectPath) return this.saveProject(false, quiet);
    this.saving = true;
    let failed = false;
    try {
      await this.saveInsertionDraft?.();
      await this.saveDrafts();
      if (!quiet) this.toast("ok", "全部词条已保存到本次会话，可导出；保存项目可在下次继续编辑");
    } catch (e) { failed = true; this.toast("error", `保存失败: ${e}`); }
    finally {
      this.saving = false;
      clearTimeout(this.#autoSaveTimer);
      if (!failed && this.drafts.keys().length) this.scheduleAutoSave();
    }
  }

  async saveProject(saveAs = false, quiet = false) {
    clearTimeout(this.#autoSaveTimer);
    if (this.projectBusy || this.loading || this.saving) return;
    this.projectBusy = !quiet;
    this.saving = true;
    let failed = false;
    try {
      let path = saveAs ? null : this.projectPath;
      if (!path) {
        path = await save({
          title: "保存编辑项目", defaultPath: this.projectPath ?? "词典校对.mdict-project",
          filters: [{ name: "MDict 编辑项目", extensions: ["mdict-project"] }],
        });
        if (!path) return;
        if (!path.toLowerCase().endsWith(".mdict-project")) path += ".mdict-project";
      }
      await this.saveInsertionDraft?.();
      await this.saveDrafts();
      const version = this.changeVersion;
      const hasNewDrafts = this.drafts.keys().length > 0;
      await api.saveProject(path);
      this.projectPath = path;
      if (!hasNewDrafts) this.savedVersion = version;
      if (!quiet) this.toast("ok", "项目已保存，下次打开项目即可继续编辑");
    } catch (e) { failed = true; this.toast("error", `保存项目失败: ${e}`); }
    finally {
      this.projectBusy = false;
      this.saving = false;
      clearTimeout(this.#autoSaveTimer);
      if (!failed && this.drafts.keys().length) this.scheduleAutoSave();
    }
  }

  async openProject() {
    if (this.projectBusy || this.loading || this.saving) return;
    clearTimeout(this.#autoSaveTimer);
    this.projectBusy = true;
    try {
      const path = await open({
        title: "打开编辑项目", multiple: false,
        filters: [{ name: "MDict 编辑项目", extensions: ["mdict-project"] }],
      });
      if (!path || typeof path !== "string") return;
      if (this.projectDirty && !await ask("当前项目有未保存的修改。打开项目会替换当前会话，是否继续？", {
        title: "打开项目", kind: "warning", okLabel: "丢弃并打开", cancelLabel: "继续编辑",
      })) return;
      const sources = await api.openProject(path);
      this.tabs = [];
      this.activeTabKey = null;
      this.jump = { stack: [], pos: -1 };
      this.selection = [];
      this.search = "";
      this.stats = {};
      this.drafts.clear();
      this.codeSelection = null;
      this.sources = sources;
      this.projectPath = path;
      this.sessionVersion += 1;
      this.bumpOverlay();
      this.savedVersion = this.changeVersion;
      this.toast("ok", "项目已恢复，可以继续编辑或导出词典");
    } catch (e) { this.toast("error", `打开项目失败: ${e}`); }
    finally {
      this.projectBusy = false;
      if (this.drafts.keys().length) this.scheduleAutoSave();
    }
  }

  async locateCode(id: ResourceId, from: number, to: number, text: string) {
    await this.openResource(id);
    this.codeSelection = { key: idKey(id), from, to, text };
  }

  /** Ctrl+Click navigation: resolve then open, with dead-link feedback. */
  async navigateRef(reference: string, context: ResourceId) {
    try {
      const outcome = await api.resolveReference(reference, context);
      if (outcome.status === "found") {
        await this.openResource(outcome.target);
      } else if (outcome.status === "ambiguous") {
        // Same name in several sources (e.g. shipped in the MDD and opened
        // from disk): jump to the first hit and say so.
        await this.openResource(outcome.candidates[0]);
        this.toast(
          "info",
          `“${reference}” 有 ${outcome.keys.length} 处匹配，已打开第 1 处: ${outcome.keys[0]}`
        );
      } else if (outcome.status === "externalRef") {
        this.toast("info", `外部链接，应用内不跳转: ${reference}`);
      } else {
        this.toast("error", `未找到引用目标: ${reference}`);
      }
    } catch (e) {
      this.toast("error", String(e));
    }
  }

  // ---- selection ----

  toggleSelection(key: string) {
    this.selection = this.selection.includes(key)
      ? this.selection.filter((k) => k !== key)
      : [...this.selection, key];
  }

  selectedIds(): ResourceId[] {
    return this.selection.map(parseIdKey);
  }
}

export const store = new AppStore();
export { PAGE };
