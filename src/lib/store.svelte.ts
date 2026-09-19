// Global application state (Svelte 5 runes). Tab management, the jump stack
// (Alt+←/→), selection for the pipeline, toasts and overlay versioning.
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "./api";
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

class AppStore {
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
      const added = await api.openSources(paths);
      this.sources = [...this.sources, ...added];
      this.toast("ok", `已加载 ${added.length} 个文件`);
      for (const s of added) void this.refreshStats(s.id);
    } catch (e) {
      this.toast("error", String(e));
    } finally {
      this.loading = false;
    }
  }

  async refreshStats(sourceId: number) {
    try {
      const stats = await api.resourceStats(sourceId);
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
    const key = idKey(id);
    let tab = this.tabs.find((t) => t.key === key);
    if (!tab) {
      try {
        const meta = await api.resourceMeta(id);
        const newTab: Tab = { key, meta, dirty: false };
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
    this.tabs = this.tabs.filter((t) => t.key !== key);
    if (this.activeTabKey === key) {
      const idx = this.tabs.findIndex((t) => t.key === key);
      this.activeTabKey =
        this.tabs[Math.max(0, idx - 1)]?.key ?? null;
    }
    this.jump = {
      stack: this.jump.stack.filter((k) => k !== key),
      pos: Math.max(-1, this.jump.pos),
    };
  }

  markDirty(key: string, dirty: boolean) {
    const tab = this.tabs.find((t) => t.key === key);
    if (tab && tab.dirty !== dirty) tab.dirty = dirty;
  }

  bumpOverlay() {
    this.overlayVersion += 1;
    void this.refreshAllStats();
  }

  /** Ctrl+Click navigation: resolve then open, with dead-link feedback. */
  async navigateRef(reference: string, context: ResourceId) {
    try {
      const outcome = await api.resolveReference(reference, context);
      if (outcome.status === "found") {
        await this.openResource(outcome.target);
      } else if (outcome.status === "ambiguous") {
        this.toast("error", `引用有歧义（${outcome.keys.length} 处匹配）: ${outcome.keys.join(", ")}`);
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
