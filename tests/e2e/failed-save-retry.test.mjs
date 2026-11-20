// a failed save is kept and retried, even after switching notes.
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
const toast = () => page.$eval(".toast span", (e) => e.textContent).catch(() => null);

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
  const n = (id, lines, dt) => ({ id, folderId: "f0", title: lines[0], snippet: lines[1] ?? "", plain: lines.join("\n"), content: { type: "doc", content: lines.map(p) }, isPinned: false, createdAt: t, updatedAt: t - dt, deletedAt: null, isLocked: false });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [n("a", ["Alpha", "first"], 0), n("b", ["Beta", "second"], 1)],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click('.note[data-id="a"]'); await sleep(300);

// Saving fails (like a full disk) while typing in A
await page.evaluate(() => localStorage.setItem("tuxnotes-mock-fail-saves", "1"));
await page.click(".prose p:last-child"); await page.keyboard.press("End"); await page.keyboard.type(" kept"); await sleep(1200);
check("the failure is reported, with a promise to retry", (await toast())?.includes("TuxNotes will keep trying"), await toast());

// Switch to B: before the fix, A's changes were thrown away here
await page.click('.note[data-id="b"]'); await sleep(600);
check("switching notes works", (await page.$eval(".prose", (e) => e.textContent)).startsWith("Beta"));
check("A's change isn't saved yet", (await stored("a")) === "Alpha | first", await stored("a"));

// Saving works again: the retry saves A's changes by itself
await page.evaluate(() => localStorage.removeItem("tuxnotes-mock-fail-saves"));
await sleep(6000);
check("the kept changes are saved once saving works again", (await stored("a")) === "Alpha | first kept", await stored("a"));
check("B is untouched", (await stored("b")) === "Beta | second", await stored("b"));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
