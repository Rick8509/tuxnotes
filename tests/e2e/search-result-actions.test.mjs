// Delete and Move to on search results act on the note, not the view.
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
const toast = () => page.$eval(".toast span", (e) => e.textContent).catch(() => null);
const search = async (q) => { await page.click(".search input", { count: 3 }); await page.keyboard.type(q); await sleep(600); };
const sidebar = (name) => page.evaluate((name) => [...document.querySelectorAll(".sidebar .row")].find((r) => r.textContent.includes(name)).click(), name);

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
  const n = (id, folderId, title, dt, deletedAt = null) => ({ id, folderId, title, snippet: "", plain: title, content: { type: "doc", content: [p(title)] }, isPinned: false, createdAt: t, updatedAt: t - dt, deletedAt, isLocked: false });
  const f = (id, name, isDefault = false) => ({ id, name, isDefault, parentId: null, sortOrder: isDefault ? 0 : 1, createdAt: t, updatedAt: t });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [f("f0", "Notes", true), f("work", "Work")],
    notes: [n("a", "f0", "Apple pie", 0), n("b", "f0", "Apple crumble", 1), n("c", "f0", "Old apple note", 2, t - 1000)],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);

// In Recently Deleted, search and press Delete on a result
await sidebar("Recently Deleted"); await sleep(400);
await search("pie");
await page.click('.note[data-id="a"]'); await sleep(300);
await page.focus(".notes"); await page.keyboard.press("Delete"); await sleep(500);
check("no 'delete permanently' question for an ordinary note", !(await page.$('[role="alertdialog"]')));
check("…it goes to Recently Deleted instead", (await toast())?.includes("moved to Recently Deleted"), await toast());
await page.keyboard.press("Escape"); await sleep(300);

// In the Notes folder, search and move a result to Work
await page.evaluate(() => [...document.querySelectorAll(".sidebar .tree-row .row")].find((r) => r.querySelector(".name").textContent.trim() === "Notes").click()); await sleep(400);
check("now in the Notes folder", (await page.$eval(".list-pane h1", (e) => e.textContent)) === "Notes");
await search("crumble");
await page.click('.note[data-id="b"]'); await sleep(300);
const r = await (await page.$('.note[data-id="b"]')).boundingBox();
await page.mouse.click(r.x + 40, r.y + 10, { button: "right" }); await sleep(200);
await page.evaluate(() => [...document.querySelectorAll(".menu button")].find((e) => e.textContent.trim() === "Work").click()); await sleep(500);
check("the moved note stays in the search results", !!(await page.$('.note[data-id="b"]')));
check("…and stays open", (await page.$eval(".prose", (e) => e.textContent)).startsWith("Apple crumble"));
check("…now shown in Work", (await page.$eval('.note[data-id="b"] .folder', (e) => e.textContent.trim())) === "Work");
await page.focus(".search input"); await page.keyboard.press("Escape"); await sleep(400);
const heading = await page.$eval(".list-pane h1", (e) => e.textContent);
check("after the search, the Notes folder no longer lists it", heading === "Notes" && !(await page.$('.note[data-id="b"]')), heading);

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
