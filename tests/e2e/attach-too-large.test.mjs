// a too-large file's error stays visible when other files are added with it.
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
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [{ id: "a", folderId: "f0", title: "Pics", snippet: "", plain: "Pics", content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "Pics" }] }] }, isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false }],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click('.note[data-id="a"]'); await sleep(200);
await page.click(".prose p"); await page.keyboard.press("End");

const paste = (big, small) => page.evaluate(async (big, small) => {
  const dt = new DataTransfer();
  if (big) dt.items.add(new File([new Uint8Array(60 * 1024 * 1024)], "holiday.mp4", { type: "video/mp4" }));
  if (small) {
    const res = await fetch("/icon.png");
    // > 2 MB so the "Adding…" message appears too.
    const png = new Uint8Array(await (await res.blob()).arrayBuffer());
    const padded = new Uint8Array(3 * 1024 * 1024); padded.set(png);
    dt.items.add(new File([padded], "photo.png", { type: "image/png" }));
  }
  document.querySelector(".prose").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
}, big, small);
const toast = () => page.$eval(".toast span", (e) => e.textContent).catch(() => null);

await paste(true, true); await sleep(1500);
check("the picture is added", (await page.$$(".prose img")).length === 1);
check("…and the too-large message stays visible", (await toast())?.includes("“holiday.mp4” is too large"), await toast());

await page.click('.toast button[aria-label="Dismiss"]'); await sleep(200);
await paste(false, true); await sleep(1500);
check("adding only a normal file leaves no message behind", (await toast()) === null, await toast());

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
