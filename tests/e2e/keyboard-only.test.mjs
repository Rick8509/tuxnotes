// keyboard-only use: note menu from the keyboard, Pin/Move in ⋯, dialogs keep the focus.
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
const menuItems = () => page.$$eval(".menu button", (els) => els.map((e) => e.textContent.replace(/ /g, "").trim()));
const focusIn = () => page.evaluate(() => document.activeElement?.closest(".notes") ? "list" : document.activeElement?.closest(".menu") ? "menu" : document.activeElement?.closest(".app") ? "app" : document.activeElement?.closest('[role="dialog"]') ? "dialog" : document.activeElement?.tagName);

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const f = (id, name, isDefault = false) => ({ id, name, isDefault, parentId: null, sortOrder: isDefault ? 0 : 1, createdAt: t, updatedAt: t });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [f("f0", "Notes", true), f("work", "Work")],
    notes: [{ id: "a", folderId: "f0", title: "Keyboard note", snippet: "", plain: "", content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "Keyboard note" }] }] }, isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false }],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click('.note[data-id="a"]'); await sleep(200);

// The note's menu from the keyboard
await page.focus(".notes");
await page.keyboard.down("Shift"); await page.keyboard.press("F10"); await page.keyboard.up("Shift"); await sleep(250);
check("Shift+F10 in the list opens the note's menu", (await menuItems()).includes("Pin Note") && (await focusIn()) === "menu", JSON.stringify((await menuItems()).slice(0, 3)));
await page.keyboard.press("Escape"); await sleep(150);
check("Escape gives the keyboard back to the list", (await focusIn()) === "list", await focusIn());
await page.keyboard.press("ContextMenu"); await sleep(250);
check("the Menu key opens it too", (await menuItems()).includes("Pin Note"));
await page.keyboard.press("Escape"); await sleep(150);

// ⋯ menu: Pin and Move to Folder…
await page.click('button[aria-label="More actions"]'); await sleep(200);
const more = await menuItems();
check("⋯ has Pin Note and Move to Folder…", more.includes("Pin Note") && more.includes("Move to Folder…"), JSON.stringify(more.slice(0, 6)));
await page.evaluate(() => [...document.querySelectorAll(".menu button")].find((e) => e.textContent.trim() === "Move to Folder…").click()); await sleep(250);
const folders = await menuItems();
check("Move to Folder… lists the folders", folders.includes("Work") && folders.includes("Notes"), JSON.stringify(folders));
await page.evaluate(() => [...document.querySelectorAll(".menu button")].find((e) => e.textContent.trim() === "Work").click()); await sleep(400);
check("…and moves the note", (await page.$eval('.note[data-id="a"] .folder', (e) => e.textContent.trim())) === "Work");
await page.click('button[aria-label="More actions"]'); await sleep(200);
await page.evaluate(() => [...document.querySelectorAll(".menu button")].find((e) => e.textContent.trim() === "Pin Note").click()); await sleep(300);
check("Pin Note from ⋯ pins it", (await page.$$eval(".list-pane .section-title, .list-pane h2, .list-pane .section", (els) => els.map((e) => e.textContent.trim()))).some((t) => t.startsWith("Pinned")));

// A dialog keeps the keyboard: Tab / Shift+Tab never reach the app behind
await page.click('button[aria-label="Settings"]'); await sleep(300);
const reached = new Set();
for (let i = 0; i < 25; i++) { await page.keyboard.down("Shift"); await page.keyboard.press("Tab"); await page.keyboard.up("Shift"); reached.add(await focusIn()); }
for (let i = 0; i < 25; i++) { await page.keyboard.press("Tab"); reached.add(await focusIn()); }
check("Tab and Shift+Tab stay out of the note and list behind Settings", !reached.has("app") && !reached.has("list"), JSON.stringify([...reached]));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
