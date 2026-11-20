// a collapsed folder counts the notes in its subfolders too.
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
const count = (name) => page.evaluate((name) => [...document.querySelectorAll(".sidebar .tree-row .row")].find((r) => r.querySelector(".name").textContent.trim() === name)?.querySelector(".count").textContent.trim(), name);

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const f = (id, name, parentId = null, isDefault = false) => ({ id, name, isDefault, parentId, sortOrder: 1, createdAt: t, updatedAt: t });
  const n = (id, folderId) => ({ id, folderId, title: id, snippet: "", plain: id, content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: id }] }] }, isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [f("f0", "Notes", null, true), f("work", "Work"), f("proj", "Projects", "work"), f("old", "Old", "proj")],
    notes: [n("w1", "work"), n("p1", "proj"), n("p2", "proj"), n("o1", "old")],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
check("expanded: Work shows its own notes", (await count("Work")) === "1", await count("Work"));
await page.click('button[aria-label="Collapse Work"]'); await sleep(200);
check("collapsed: Work counts its subfolders' notes too", (await count("Work")) === "4", await count("Work"));
await page.click('button[aria-label="Expand Work"]'); await sleep(200);
await page.click('button[aria-label="Collapse Projects"]'); await sleep(200);
check("a collapsed subfolder counts its own subfolders", (await count("Projects")) === "3" && (await count("Work")) === "1", `${await count("Projects")} / ${await count("Work")}`);

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
