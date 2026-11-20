// locked notes don't grab the keyboard in the note list; each lock screen starts fresh.
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
const selected = () => page.$eval(".note.selected", (e) => e.dataset.id).catch(() => null);
const focusIn = () => page.evaluate(() => (document.activeElement?.closest(".notes") ? "list" : document.activeElement?.matches('.locked input') ? "password" : document.activeElement?.tagName));

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const n = (id, title, dt, isLocked = false) => ({ id, folderId: "f0", title, snippet: "", plain: "", content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: title }] }] }, isPinned: false, createdAt: t, updatedAt: t - dt, deletedAt: null, isLocked });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [n("a", "Plain one", 0), n("b", "Locked one", 1, true), n("c", "Locked two", 2, true), n("d", "Plain two", 3)],
    lock: { password: "hunter22", hint: "" },
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click('.note[data-id="a"]'); await sleep(200);
await page.focus(".notes");
for (const expected of ["b", "c", "d"]) {
  await page.keyboard.press("ArrowDown"); await sleep(250);
  check(`ArrowDown reaches ${expected} and the keyboard stays in the list`, (await selected()) === expected && (await focusIn()) === "list", `${await selected()} / ${await focusIn()}`);
}

// Enter on a locked note moves into its password field
await page.keyboard.press("ArrowUp"); await sleep(250);
await page.keyboard.press("Enter"); await sleep(250);
check("Enter on a locked note goes to its password field", (await focusIn()) === "password", await focusIn());

// A wrong password on one note doesn't show up on the next
await page.keyboard.type("nope"); await page.keyboard.press("Enter"); await sleep(400);
check("wrong password shows an error", (await page.$eval(".locked .error", (e) => e.textContent.trim())) === "Wrong password.");
await page.click('.note[data-id="b"]'); await sleep(300);
const fresh = await page.evaluate(() => ({ value: document.querySelector(".locked input").value, error: document.querySelector(".locked .error").textContent.trim() }));
check("the next locked note starts fresh", fresh.value === "" && fresh.error === "", JSON.stringify(fresh));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
