// "New notes start with" only affects new notes; existing notes keep their look.
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
/** "title" / "heading" / "body" judged by the first line's font size relative to the body text. */
const look = () => page.evaluate(() => {
  const first = parseFloat(getComputedStyle(document.querySelector(".prose > :first-child")).fontSize);
  const body = parseFloat(getComputedStyle(document.querySelector(".prose")).fontSize);
  const r = first / body;
  return r > 1.6 ? "title" : r > 1.2 ? "heading" : "body";
});
const setFirstLine = async (label) => {
  await page.click('button[aria-label="Settings"]'); await sleep(200);
  await page.evaluate((l) => [...document.querySelectorAll('[aria-label="New notes start with"] button')].find((x) => x.textContent.trim() === l).click(), label); await sleep(150);
  await page.keyboard.press("Escape"); await sleep(200);
};
const newNote = async (text) => { await page.click('button[aria-label="New note"]'); await sleep(250); await page.keyboard.type(text); await sleep(900); };
const openNote = async (title) => { await page.evaluate((t) => [...document.querySelectorAll(".note")].find((n) => n.querySelector(".title").textContent.trim() === t).click(), title); await sleep(300); };
const stored = (title) => page.evaluate((t) => JSON.parse(localStorage.getItem("tuxnotes-mock-v1")).notes.find((n) => n.title === t)?.content.attrs ?? null, title);

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [{ id: "old", folderId: "f0", title: "An older note", snippet: "", plain: "", content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "An older note" }] }, { type: "paragraph", content: [{ type: "text", text: "body" }] }] }, isPinned: false, createdAt: t - 1e6, updatedAt: t - 1e6, deletedAt: null, isLocked: false }],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);

await setFirstLine("Body");
await newNote("Body first");
check("with Body, a new note's first line is body text", (await look()) === "body", await look());
await openNote("An older note");
check("an older note keeps its title look", (await look()) === "title", await look());

await setFirstLine("Heading");
await newNote("Heading first");
check("with Heading, a new note starts with a heading look", (await look()) === "heading", await look());
await openNote("Body first");
check("the Body note keeps its look after changing the setting", (await look()) === "body", await look());

await setFirstLine("Title");
await newNote("Title first");
check("with Title, a new note starts with a title", (await look()) === "title", await look());
check("title notes are stored without the extra mark (compatible)", (await stored("Title first")) === null, JSON.stringify(await stored("Title first")));
check("body notes remember their style", (await stored("Body first"))?.firstLine === "body", JSON.stringify(await stored("Body first")));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
