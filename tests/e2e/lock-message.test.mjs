// the "Note locked" message matches the auto-lock setting.
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

async function lockMessage(autoLockMinutes) {
  await page.goto(BASE + "/icon.png");
  await page.evaluate((autoLockMinutes) => {
    localStorage.clear();
    localStorage.setItem("tuxnotes-prefs-v1", JSON.stringify({ autoLockMinutes }));
    const t = Date.now();
    localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
      folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
      notes: [{ id: "a", folderId: "f0", title: "Secret", snippet: "", plain: "Secret", content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "Secret" }] }] }, isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false }],
    }));
  }, autoLockMinutes);
  await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
  await page.click('.note[data-id="a"]'); await sleep(200);
  await page.click('button[aria-label="More actions"]'); await sleep(150);
  await page.evaluate(() => [...document.querySelectorAll(".menu button")].find((e) => e.textContent.trim() === "Lock Note…").click()); await sleep(200);
  await page.type('input[aria-label="New password"]', "hunter22");
  await page.type('input[aria-label="Verify password"]', "hunter22");
  await page.click('[aria-labelledby="lock-title"] button[type="submit"]'); await sleep(500);
  return page.$eval(".toast span", (e) => e.textContent).catch(() => null);
}

const ten = await lockMessage(10);
check("with the default setting it mentions the 10 idle minutes", ten?.includes("after 10 minutes without use") && ten.includes("when TuxNotes closes"), ten);
const hour = await lockMessage(60);
check("with 1 hour", hour?.includes("after an hour without use"), hour);
const never = await lockMessage(0);
check("with 'only when TuxNotes closes' it doesn't mention idle time", never === "Note locked. It stays open until you choose Lock Now or when TuxNotes closes.", never);

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
