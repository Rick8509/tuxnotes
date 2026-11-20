// Japanese words inside a sentence are found and highlighted.
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
await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [{ id: "ja", folderId: "f0", title: "買い物", snippet: "今日は牛乳を買う", plain: "買い物\n今日は牛乳を買う", content: { type: "doc", content: [p("買い物"), p("今日は牛乳を買う")] }, isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false }],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click(".search input"); await page.keyboard.type("牛乳"); await sleep(700);
check("the note is found", !!(await page.$('.note[data-id="ja"]')));
await page.click('.note[data-id="ja"]'); await sleep(400);
const marks = await page.$$eval(".prose .search-match, .prose [class*='search']", (els) => els.map((e) => e.textContent));
check("the word is highlighted in the note", marks.includes("牛乳"), JSON.stringify(marks));
console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
