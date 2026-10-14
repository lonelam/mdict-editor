import { describe, expect, it } from "vitest";
import { Drafts } from "./drafts";

describe("editing drafts", () => {
  it("keeps drafts for inactive/closed tabs and saves all of them", async () => {
    const drafts = new Drafts();
    drafts.set("mdx:0:2", "校对");
    drafts.set("ext:1", "p{}");
    const writes: string[] = [];
    await drafts.save(async (key, text) => { writes.push(`${key}=${text}`); });
    expect(writes).toEqual(["mdx:0:2=校对", "ext:1=p{}"]);
    expect(drafts.keys()).toEqual([]);
  });

  it("retains failed saves and newer text typed while saving", async () => {
    const drafts = new Drafts();
    drafts.set("entry", "first");
    await expect(drafts.save(async () => { throw new Error("disk full"); })).rejects.toThrow("disk full");
    expect(drafts.get("entry")).toBe("first");
    await drafts.save(async () => { drafts.set("entry", "newer"); });
    expect(drafts.get("entry")).toBe("newer");
  });

  it("saving the active tab leaves other drafts intact", async () => {
    const drafts = new Drafts();
    drafts.set("one", "1");
    drafts.set("two", "2");
    await drafts.save(async () => {}, "one");
    expect(drafts.get("one")).toBeUndefined();
    expect(drafts.get("two")).toBe("2");
  });
});
