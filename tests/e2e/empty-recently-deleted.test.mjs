// Empty Recently Deleted keeps the open note (unless it was deleted).
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
const trashRow = () => page.evaluateHandle(() => [...document.querySelectorAll(".sidebar .row")].find((r) => r.textContent.includes("Recently Deleted")));
const emptyFromSidebar = async () => {
  const r = await (await trashRow()).boundingBox();
  await page.mouse.click(r.x + 40, r.y + 10, { button: "right" }); await sleep(200);
  await page.evaluate(() => [...document.querySelectorAll(".menu button")].find((e) => e.textContent.includes("Empty")).click()); await sleep(200);
  await page.evaluate(() => [...document.querySelectorAll('[role="alertdialog"] button')].find((e) => e.textContent.includes("Delete All")).click()); await sleep(500);
};
const openTitle = () => page.$eval(".prose", (e) => e.textContent.split("\n")[0]).catch(() => null);

async function seed() {
  await page.goto(BASE + "/icon.png");
  await page.evaluate(() => {
    localStorage.clear();
    const t = Date.now();
    const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
    const n = (id, title, dt, deletedAt = null) => ({ id, folderId: "f0", title, snippet: "", plain: title, content: { type: "doc", content: [p(title)] }, isPinned: false, createdAt: t, updatedAt: t - dt, deletedAt, isLocked: false });
    localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
      folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
      notes: [n("a", "First", 0), n("b", "Second", 1), n("c", "Deleted one", 2, t - 1000), n("d", "Deleted two", 3, t - 2000)],
    }));
  });
  await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
}

await seed();
await page.click('.note[data-id="b"]'); await sleep(300);
await emptyFromSidebar();
check("emptying from another view keeps the open note", (await openTitle())?.startsWith("Second"), await openTitle());
check("…and the deleted notes are gone", !(await page.evaluate(() => [...document.querySelectorAll(".sidebar .row")].some((r) => r.textContent.includes("Recently Deleted")))));

await seed();
await page.evaluate(() => [...document.querySelectorAll(".sidebar .row")].find((r) => r.textContent.includes("Recently Deleted")).click()); await sleep(400);
await page.click('.note[data-id="d"]'); await sleep(300);
await emptyFromSidebar();
check("emptying while a deleted note is open closes it", !(await page.$(".prose")) || (await page.$eval(".editor-pane .scroll", (e) => e.classList.contains("hidden"))));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
