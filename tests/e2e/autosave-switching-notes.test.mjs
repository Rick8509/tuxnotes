// an edit that lands while switching notes must be saved into the note it was made in.
import puppeteer from "puppeteer-core";
const BASE = process.env.BASE_URL ?? "http://localhost:4173";
const CHROME = process.env.CHROME ?? "/usr/bin/google-chrome";
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ["--no-sandbox"] });
const page = await b.newPage();
await page.setViewport({ width: 1200, height: 700 });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? "  — " + extra : ""}`); if (!ok) failures++; };
const stored = (id) => page.evaluate((id) => {
  const n = JSON.parse(localStorage.getItem("tuxnotes-mock-v1")).notes.find((x) => x.id === id);
  return n.content.content.map((p) => (p.content ?? []).map((t) => t.text).join("")).join(" | ");
}, id);

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
  const n = (id, lines, dt) => ({ id, folderId: "f0", title: lines[0], snippet: lines[1] ?? "", plain: lines.join("\n"), content: { type: "doc", content: lines.map(p) }, isPinned: false, createdAt: t, updatedAt: t - dt, deletedAt: null, isLocked: false });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [n("a", ["Alpha", "one two three"], 0), n("b", ["Beta", "four five"], 1)],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click('.note[data-id="a"]'); await sleep(300);
await page.click(".prose p:last-child"); await page.keyboard.press("End"); await sleep(800);

// Click note B and, in the same moment, an edit lands in note A (like a paste finishing).
await page.evaluate(() => {
  document.querySelector('.note[data-id="b"]').click();
  document.execCommand("insertText", false, " LATE");
});
await sleep(1500);
const a = await stored("a"), bb = await stored("b");
check("note A keeps its own text plus the late edit", a === "Alpha | one two three LATE", a);
check("note B is unchanged", bb === "Beta | four five", bb);
check("the editor now shows note B", (await page.$eval(".prose", (e) => e.textContent)).startsWith("Beta"));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
