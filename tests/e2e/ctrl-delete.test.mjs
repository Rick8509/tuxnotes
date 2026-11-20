// Ctrl+Delete must only delete the note when not typing.
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
const ctrlDelete = async () => { await page.keyboard.down("Control"); await page.keyboard.press("Delete"); await page.keyboard.up("Control"); await sleep(500); };
const has = (id) => page.evaluate((id) => !!document.querySelector(`.note[data-id="${id}"]`), id);
const toasts = () => page.$$eval(".toast span", (els) => els.map((e) => e.textContent.trim()));

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
  const n = (id, lines, dt) => ({ id, folderId: "f0", title: lines[0], snippet: lines[1] ?? "", plain: lines.join("\n"), content: { type: "doc", content: lines.map(p) }, isPinned: false, createdAt: t, updatedAt: t - dt, deletedAt: null, isLocked: false });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [n("a", ["Alpha", "one two three"], 0), n("b", ["Beta", "four five"], 1), n("c", ["Gamma", "six"], 2)],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);

// In the editor: deletes the next word, the note stays
await page.click('.note[data-id="a"]'); await sleep(200);
await page.click(".prose p:last-child"); await page.keyboard.press("Home"); await sleep(100);
await ctrlDelete();
check("Ctrl+Delete in the note keeps the note", await has("a"));
check("…and deletes the next word instead", (await page.$eval(".prose p:last-child", (e) => e.textContent)).trim() === "two three", await page.$eval(".prose p:last-child", (e) => e.textContent));

// In the search field: nothing happens to the note
await page.click(".search input"); await page.keyboard.type("x"); await page.keyboard.press("Home");
await ctrlDelete();
check("Ctrl+Delete in the search field keeps the note", await has("a"));
await page.keyboard.press("Escape"); await sleep(300);

// Note list focused: the note goes to Recently Deleted once (no error)
await page.click('.note[data-id="b"]'); await sleep(300);
await page.focus(".notes"); await ctrlDelete();
const t1 = await toasts();
check("Ctrl+Delete in the note list deletes the selected note", !(await has("b")));
check("…exactly once (no 'already in Recently Deleted' error)", t1.length === 1 && t1[0].includes("moved to Recently Deleted"), JSON.stringify(t1));

// Plain Delete in the list still works
await page.click('.note[data-id="c"]'); await sleep(300);
await page.focus(".notes"); await page.keyboard.press("Delete"); await sleep(500);
check("Delete in the note list still deletes", !(await has("c")));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
