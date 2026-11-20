// Enter on Cancel / Remove Link in the link box does what the button says.
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
const links = () => page.$$eval(".prose a", (els) => els.map((a) => a.getAttribute("href")));
const ctrlK = async () => { await page.keyboard.down("Control"); await page.keyboard.press("k"); await page.keyboard.up("Control"); await sleep(300); };
const enterOn = async (label) => {
  await page.evaluate((label) => [...document.querySelectorAll(".link-form button")].find((x) => x.textContent.trim() === label).focus(), label);
  await page.keyboard.press("Enter"); await sleep(300);
};

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (c) => ({ type: "paragraph", content: c });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [{ id: "a", folderId: "f0", title: "Links", snippet: "", plain: "", content: { type: "doc", content: [
      p([{ type: "text", text: "Links" }]),
      p([{ type: "text", text: "plain words here" }]),
      p([{ type: "text", text: "a site", marks: [{ type: "link", attrs: { href: "https://example.com" } }] }]),
    ] }, isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false }],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click('.note[data-id="a"]'); await sleep(300);

// New link: select "words", Ctrl+K, type an address, then Enter on Cancel
await page.evaluate(() => {
  const textNode = [...document.querySelectorAll(".prose p")][1].firstChild;
  const r = document.createRange(); r.setStart(textNode, 6); r.setEnd(textNode, 11);
  const sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
});
await ctrlK();
check("the link box opens", !!(await page.$(".link-form")));
await page.keyboard.type("example.org");
await enterOn("Cancel");
check("Enter on Cancel closes the box", !(await page.$(".link-form")));
check("…without adding the link", !(await links()).includes("https://example.org"), JSON.stringify(await links()));

// Existing link: open it for editing, then Enter on Remove Link
await page.click(".prose a"); await sleep(300);
await page.evaluate(() => [...document.querySelectorAll(".link-bubble button")].find((x) => x.textContent.trim() === "Edit").click()); await sleep(300);
check("the edit box opens", !!(await page.$(".link-form")));
await enterOn("Remove Link");
check("Enter on Remove Link removes the link", (await links()).length === 0, JSON.stringify(await links()));

// Enter in the address field still saves
await page.evaluate(() => {
  const textNode = [...document.querySelectorAll(".prose p")][1].firstChild;
  const r = document.createRange(); r.setStart(textNode, 0); r.setEnd(textNode, 5);
  const sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
});
await ctrlK();
await page.keyboard.type("example.net"); await page.keyboard.press("Enter"); await sleep(300);
check("Enter in the address field still adds the link", (await links()).includes("https://example.net"), JSON.stringify(await links()));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
