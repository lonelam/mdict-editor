import { describe, expect, it } from "vitest";
import {
  classifyReference,
  cleanRefText,
  extractHtmlRefs,
  looksLikePath,
} from "./refs";

describe("classifyReference", () => {
  it("classifies entry:// and mdx:// as entry jumps", () => {
    expect(classifyReference("entry://world")).toBe("entry");
    expect(classifyReference("entry://苹果")).toBe("entry");
    expect(classifyReference("mdx://some word")).toBe("entry");
  });

  it("classifies sound:// as audio", () => {
    expect(classifyReference("sound://audio/hello.wav")).toBe("sound");
  });

  it("treats web links as external", () => {
    expect(classifyReference("https://example.com/a.png")).toBe("external");
    expect(classifyReference("http://x")).toBe("external");
    expect(classifyReference("data:image/png;base64,AAA")).toBe("external");
    expect(classifyReference("mailto:a@b.c")).toBe("external");
  });

  it("recognizes resource paths", () => {
    expect(classifyReference("/img/logo.png")).toBe("path");
    expect(classifyReference("css/style.css")).toBe("path");
    expect(classifyReference("../img/heart.png")).toBe("path");
    expect(classifyReference("/js/app.js")).toBe("path");
    expect(classifyReference("\\yd\\YD_apple.png")).toBe("path");
    expect(classifyReference("docs/my%20file.txt")).toBe("path");
    expect(classifyReference("hello.wav")).toBe("path");
  });

  it("rejects non-path strings", () => {
    expect(classifyReference("hello world")).toBe("none");
    expect(classifyReference("42")).toBe("none");
    expect(classifyReference("")).toBe("none");
    expect(classifyReference("function")).toBe("none");
  });

  it("unknown schemes fall back to path (backend decides)", () => {
    expect(classifyReference("video://x.mp4")).toBe("path");
  });
});

describe("looksLikePath", () => {
  it("accepts slash forms and extensions", () => {
    expect(looksLikePath("/a/b.png")).toBe(true);
    expect(looksLikePath("./x.css")).toBe(true);
    expect(looksLikePath("name.htm")).toBe(true);
  });
  it("rejects prose and bare words", () => {
    expect(looksLikePath("hello")).toBe(false);
    expect(looksLikePath("two words.js")).toBe(false);
    expect(looksLikePath("")).toBe(false);
  });
});

describe("cleanRefText", () => {
  it("strips quotes and whitespace", () => {
    expect(cleanRefText('  "/img/a.png" ')).toBe("/img/a.png");
    expect(cleanRefText("'sound://a.wav'")).toBe("sound://a.wav");
    expect(cleanRefText("/a.png")).toBe("/a.png");
  });
});

describe("extractHtmlRefs", () => {
  it("pulls src/href/poster values", () => {
    const html = `<link href="css/style.css"><img src="/img/logo.png" alt="x">
      <video poster='./p.jpg' controls></video>
      <a href="entry://world">w</a>`;
    expect(extractHtmlRefs(html)).toEqual([
      "css/style.css",
      "/img/logo.png",
      "./p.jpg",
      "entry://world",
    ]);
  });
});
