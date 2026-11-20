// typing during a search must not scroll the note back to the first match.
import puppeteer from "puppeteer-core";
const BASE = process.env.BASE_URL ?? "http://localhost:4173";
const CHROME = process.env.CHROME ?? "/usr/bin/google-chrome";
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ["--no-sandbox"] });
const page = await b.newPage();
await page.setViewport({ width: 1200, height: 600 });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? "  — " + extra : ""}`); if (!ok) failures++; };
const scrollTop = () => page.evaluate(() => document.querySelector(".editor-pane .scroll").scrollTop);

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
  const lines = ["Long note"];
  for (let i = 0; i < 60; i++) lines.push(`filler line ${i}`);
  lines.push("milk near the end");
  for (let i = 60; i < 70; i++) lines.push(`filler line ${i}`);
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [{ id: "a", folderId: "f0", title: "Long note", snippet: "", plain: lines.join("\n"), content: { type: "doc", content: lines.map(p) }, isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false }],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);

await page.click(".search input"); await page.keyboard.type("milk"); await sleep(700);
await page.click('.note[data-id="a"]'); await sleep(500);
const atMatch = await scrollTop();
check("opening a search result scrolls to the match", atMatch > 300, `scrollTop ${atMatch}`);

// Scroll to the very top and type there; saving refreshes the results.
await page.evaluate(() => { document.querySelector(".editor-pane .scroll").scrollTop = 0; });
await page.click(".prose h1, .prose p"); await page.keyboard.press("End"); await page.keyboard.type(" edited"); await sleep(1500);
const after = await scrollTop();
check("typing elsewhere during a search keeps your place", after < 100, `scrollTop ${after}`);

// A new search still scrolls to its first match.
await page.click(".search input", { count: 3 }); await page.keyboard.type("near"); await sleep(700);
await page.click('.note[data-id="a"]'); await sleep(500);
check("a new search scrolls to its match again", (await scrollTop()) > 300, `scrollTop ${await scrollTop()}`);

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
