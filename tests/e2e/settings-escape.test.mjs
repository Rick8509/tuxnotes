// Escape on a question inside Settings closes only the question.
import puppeteer from "puppeteer-core";
const BASE = process.env.BASE_URL ?? "http://localhost:4173";
const CHROME = process.env.CHROME ?? "/usr/bin/google-chrome";
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ["--no-sandbox"] });
const page = await b.newPage();
await page.setViewport({ width: 1100, height: 720 });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? "  — " + extra : ""}`); if (!ok) failures++; };
const click = (sel, t) => page.evaluate((sel, t) => [...document.querySelectorAll(sel)].find((e) => e.textContent.includes(t)).click(), sel, t);
const settingsOpen = () => page.$('[role="dialog"][aria-label="Settings"]').then(Boolean);
const confirmOpen = () => page.$('[role="alertdialog"]').then(Boolean);

await page.goto(BASE + "", { waitUntil: "load" });
await page.evaluate(() => localStorage.clear());
await page.reload({ waitUntil: "load" }); await sleep(300);
await page.click('button[aria-label="New note"]'); await sleep(150);
await page.keyboard.type("A note"); await sleep(500);
await page.click('button[aria-label="Settings"]'); await sleep(200);
await click(".nav-item", "Backup"); await sleep(300);
await click(".panel .btn", "Back Up to Google Drive"); await sleep(200);
await click('[role="alertdialog"] .btn', "Open Browser"); await sleep(2300);

// "Disconnect?" → Escape
await click(".panel .link", "Disconnect"); await sleep(200);
check("the question is shown", await confirmOpen());
await page.keyboard.press("Escape"); await sleep(200);
check("Escape closes the question", !(await confirmOpen()));
check("…but Settings stays open", await settingsOpen());

// "Replace your notes with this backup?" → Escape stays on the backup list
await click(".panel .btn", "Restore…"); await sleep(600);
await page.click(".panel .backup input"); await sleep(100);
await click(".panel .actions .btn", "Restore"); await sleep(200);
await page.keyboard.press("Escape"); await sleep(200);
check("Escape on 'Replace your notes?' stays on the backup list", (await page.$eval("#backup-title", (e) => e.textContent)) === "Restore from Google Drive" && (await settingsOpen()));

// Escape without a question still does its normal job
await page.keyboard.press("Escape"); await sleep(200);
check("Escape on the backup list goes back", (await page.$eval("#backup-title", (e) => e.textContent)) === "Google Drive Backup");
await page.keyboard.press("Escape"); await sleep(200);
check("Escape then closes Settings", !(await settingsOpen()));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
