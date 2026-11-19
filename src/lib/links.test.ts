import { describe, expect, it } from "vitest";
import { displayUrl, isOpenableUrl, normalizeUrl } from "./links";

describe("normalizeUrl", () => {
  it("keeps full web, mail and phone links", () => {
    expect(normalizeUrl(" https://example.com/a?b=1 ")).toBe("https://example.com/a?b=1");
    expect(normalizeUrl("http://localhost:8080")).toBe("http://localhost:8080");
    expect(normalizeUrl("mailto:me@example.com")).toBe("mailto:me@example.com");
    expect(normalizeUrl("tel:+491234")).toBe("tel:+491234");
  });
  it("adds the missing part for bare domains and emails", () => {
    expect(normalizeUrl("tuxnotes.org")).toBe("https://tuxnotes.org");
    expect(normalizeUrl("github.com/Rick8509/tuxnotes")).toBe("https://github.com/Rick8509/tuxnotes");
    expect(normalizeUrl("example.com:8443/x")).toBe("https://example.com:8443/x");
    expect(normalizeUrl("localhost:3000")).toBe("https://localhost:3000");
    expect(normalizeUrl("me@example.com")).toBe("mailto:me@example.com");
  });
  it("refuses anything that isn't a real link", () => {
    for (const bad of ["", "   ", "javascript:alert(1)", "JavaScript:alert(1)", "data:text/html,x", "file:///etc/passwd", "hello world", "notalink", "vbscript:x"]) {
      expect(normalizeUrl(bad), bad).toBeNull();
    }
  });
});

describe("displayUrl / isOpenableUrl", () => {
  it("shows links compactly", () => {
    expect(displayUrl("https://example.com/")).toBe("example.com");
    expect(displayUrl("mailto:me@example.com")).toBe("me@example.com");
    expect(displayUrl("https://example.com/" + "a".repeat(80)).length).toBe(48);
  });
  it("only opens web, mail and phone links", () => {
    expect(isOpenableUrl("https://x.org")).toBe(true);
    expect(isOpenableUrl("javascript:alert(1)")).toBe(false);
  });
});
