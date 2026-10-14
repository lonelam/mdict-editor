import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppStore } from "./store.svelte";
import * as api from "./api";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { idKey, type ResourceId, type SourceInfo } from "./types";

vi.mock("./api", () => ({
  openProject: vi.fn(), saveProject: vi.fn(), writeResource: vi.fn(),
  resourceStats: vi.fn(), resourceMeta: vi.fn(), removeSource: vi.fn(), deleteResource: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(), open: vi.fn(), save: vi.fn() }));

const id: ResourceId = { t: "mdx", source: 0, ordinal: 2 };
const sources: SourceInfo[] = [{ id: 0, kind: "mdx", name: "ocean.mdx", title: null, entryCount: 6, dir: "F:/dict" }];

describe("project workflow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.useFakeTimers();
    vi.mocked(api.resourceStats).mockResolvedValue([]);
    vi.mocked(api.resourceMeta).mockResolvedValue({ id, key: "hello", category: "entry", mime: "text/html", edited: false, deleted: false, sourceName: "ocean.mdx", sizeCurrent: null });
  });
  afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

  it("includes closed-tab drafts and insertion text in the disk snapshot", async () => {
    const store = new AppStore();
    store.sources = sources;
    await store.openResource(id);
    store.setDraft(idKey(id), "<p>校对</p>");
    store.closeTab(idKey(id));
    const insertion = vi.fn(async () => { await api.writeResource(id, new Uint8Array()); });
    store.saveInsertionDraft = insertion;
    vi.mocked(save).mockResolvedValue("F:/校对");
    await store.saveProject();
    expect(insertion).toHaveBeenCalledOnce();
    expect(api.writeResource).toHaveBeenCalledWith(id, new TextEncoder().encode("<p>校对</p>"));
    expect(api.saveProject).toHaveBeenCalledWith("F:/校对.mdict-project");
    expect(vi.mocked(api.writeResource).mock.invocationCallOrder[1]).toBeLessThan(vi.mocked(api.saveProject).mock.invocationCallOrder[0]);
    expect(store.projectDirty).toBe(false);
    expect(store.projectBusy).toBe(false);
    await store.saveCurrent();
    expect(save).toHaveBeenCalledOnce();
    expect(api.saveProject).toHaveBeenCalledTimes(2);
  });

  it("Ctrl+S saves multiple entries in one session without a project dialog", async () => {
    const store = new AppStore();
    store.setAutoSave(false);
    const second: ResourceId = { t: "mdx", source: 0, ordinal: 4 };
    store.setDraft(idKey(id), "first, edited in multiple places");
    store.setDraft(idKey(second), "second");
    store.activeTabKey = idKey(second);
    await store.saveCurrent();
    expect(api.writeResource).toHaveBeenCalledTimes(2);
    expect(store.drafts.keys()).toEqual([]);
    expect(save).not.toHaveBeenCalled();
  });

  it("autosave coalesces typing and saves all drafts after the pause", async () => {
    const store = new AppStore();
    store.setDraft(idKey(id), "old");
    await vi.advanceTimersByTimeAsync(400);
    store.setDraft(idKey(id), "new");
    store.setDraft(idKey({ t: "mdx", source: 0, ordinal: 4 }), "other");
    await vi.advanceTimersByTimeAsync(799);
    expect(api.writeResource).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(api.writeResource).toHaveBeenCalledTimes(2);
    expect(api.writeResource).toHaveBeenCalledWith(id, new TextEncoder().encode("new"));
    expect(store.toasts).toEqual([]);
    await vi.advanceTimersByTimeAsync(1600);
    expect(api.writeResource).toHaveBeenCalledTimes(2);
  });

  it("autosaves a chosen project without disabling the editor and keeps newer drafts dirty", async () => {
    const store = new AppStore();
    store.projectPath = "F:/test.mdict-project";
    store.setDraft(idKey(id), "old");
    vi.mocked(api.writeResource).mockImplementationOnce(async () => {
      expect(store.projectBusy).toBe(false);
      store.setDraft(idKey(id), "newer");
      return { historyLen: 1, sizeOriginal: 0, sizeCurrent: 3 };
    });
    await store.saveCurrent(true);
    expect(api.saveProject).toHaveBeenCalledOnce();
    expect(store.drafts.get(idKey(id))).toBe("newer");
    expect(store.projectDirty).toBe(true);
    await vi.advanceTimersByTimeAsync(800);
    expect(store.projectDirty).toBe(false);
    expect(store.drafts.keys()).toEqual([]);
  });

  it("disabled or failed autosave retains drafts without repeated error loops", async () => {
    const store = new AppStore();
    store.setAutoSave(false);
    store.setDraft(idKey(id), "text");
    await vi.advanceTimersByTimeAsync(1000);
    expect(api.writeResource).not.toHaveBeenCalled();
    vi.mocked(api.writeResource).mockRejectedValue(new Error("failed"));
    store.setAutoSave(true);
    await vi.advanceTimersByTimeAsync(5000);
    expect(api.writeResource).toHaveBeenCalledOnce();
    expect(store.drafts.get(idKey(id))).toBe("text");
    expect(store.toasts).toHaveLength(1);
  });

  it("serializes overlapping draft writes and does not duplicate undo revisions", async () => {
    const store = new AppStore();
    store.setDraft(idKey(id), "text");
    await Promise.all([store.saveDrafts(), store.saveDrafts()]);
    expect(api.writeResource).toHaveBeenCalledOnce();
  });

  it("cancelled or failed saves keep the project dirty and release busy state", async () => {
    const store = new AppStore();
    store.setDraft(idKey(id), "text");
    vi.mocked(save).mockResolvedValue(null);
    await store.saveProject();
    expect(api.writeResource).not.toHaveBeenCalled();
    expect(store.drafts.get(idKey(id))).toBe("text");
    vi.mocked(save).mockResolvedValue("F:/test.mdict-project");
    vi.mocked(api.saveProject).mockRejectedValue(new Error("disk full"));
    await store.saveProject();
    expect(store.projectPath).toBeNull();
    expect(store.projectDirty).toBe(true);
    expect(store.projectBusy).toBe(false);
  });

  it("a failed draft write prevents saving an incomplete project", async () => {
    const store = new AppStore();
    store.projectPath = "F:/test.mdict-project";
    store.setDraft(idKey(id), "text");
    vi.mocked(api.writeResource).mockRejectedValue(new Error("write failed"));
    await store.saveProject();
    expect(api.saveProject).not.toHaveBeenCalled();
    expect(store.drafts.get(idKey(id))).toBe("text");
    expect(store.projectDirty).toBe(true);
  });

  it("failed restores retain tabs and drafts; success resets session state", async () => {
    const store = new AppStore();
    store.sources = sources;
    await store.openResource(id);
    store.setDraft(idKey(id), "keep me");
    store.selection = [idKey(id)];
    vi.mocked(open).mockResolvedValue("F:/test.mdict-project");
    vi.mocked(ask).mockResolvedValue(false);
    await store.openProject();
    expect(api.openProject).not.toHaveBeenCalled();
    vi.mocked(ask).mockResolvedValue(true);
    vi.mocked(api.openProject).mockRejectedValue(new Error("missing source"));
    await store.openProject();
    expect(store.tabs).toHaveLength(1);
    expect(store.drafts.get(idKey(id))).toBe("keep me");
    vi.mocked(api.openProject).mockResolvedValue(sources);
    await store.openProject();
    expect(store.tabs).toHaveLength(0);
    expect(store.drafts.keys()).toEqual([]);
    expect(store.selection).toEqual([]);
    expect(store.sessionVersion).toBe(1);
    expect(store.projectPath).toBe("F:/test.mdict-project");
    expect(store.projectDirty).toBe(false);
  });

  it("saves a draft before deleting and does not resurrect it on project save", async () => {
    const store = new AppStore();
    await store.openResource(id);
    store.setDraft(idKey(id), "edited");
    await store.markDeleted(id);
    expect(vi.mocked(api.writeResource).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(api.deleteResource).mock.invocationCallOrder[0]);
    expect(store.drafts.keys()).toEqual([]);
    expect(store.tabs).toEqual([]);
    store.projectPath = "F:/test.mdict-project";
    await store.saveProject();
    expect(api.writeResource).toHaveBeenCalledOnce();
  });

  it("keeps jump history valid when closing the middle active tab", async () => {
    const store = new AppStore();
    for (const ordinal of [0, 1, 2]) await store.openResource({ t: "mdx", source: 0, ordinal });
    store.jumpBack();
    store.closeTab(idKey({ t: "mdx", source: 0, ordinal: 1 }));
    expect(store.activeTabKey).toBe(idKey({ t: "mdx", source: 0, ordinal: 0 }));
    expect(store.jump.pos).toBe(0);
    store.jumpForward();
    expect(store.activeTabKey).toBe(idKey(id));
  });

  it("preserves repeated visits in jump history when an unrelated tab closes", async () => {
    const store = new AppStore();
    await store.openResource(id);
    const other: ResourceId = { t: "mdx", source: 0, ordinal: 1 };
    await store.openResource(other);
    await store.openResource(id);
    store.closeTab(idKey(other));
    expect(store.jump.stack).toEqual([idKey(id), idKey(id)]);
    expect(store.jump.pos).toBe(1);
  });
});
