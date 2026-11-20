// right-click on text keeps the system menu; TuxNotes menus on checkboxes and cell selections.
import puppeteer from "puppeteer-core";
const BASE = process.env.BASE_URL ?? "http://localhost:4173";
const CHROME = process.env.CHROME ?? "/usr/bin/google-chrome";
const b = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ["--no-sandbox"] });
const page = await b.newPage();
await page.setViewport({ width: 1200, height: 760 });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "PASS" : "FAIL"}  ${name}${extra ? "  — " + extra : ""}`); if (!ok) failures++; };
/** Right-click an element; returns whether TuxNotes took over (custom menu shown, system menu blocked). */
const rightClick = (selector) => page.evaluate(async (selector) => {
  const el = document.querySelector(selector);
  const r = el.getBoundingClientRect();
  const ev = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 5, clientY: r.top + r.height / 2, button: 2 });
  el.dispatchEvent(ev);
  await new Promise((res) => setTimeout(res, 150));
  const menu = [...document.querySelectorAll(".menu button")].map((b) => b.textContent.trim());
  return { custom: ev.defaultPrevented, menu };
}, selector);
const closeMenus = async () => { await page.keyboard.press("Escape"); await sleep(150); };

await page.goto(BASE + "/icon.png");
await page.evaluate(() => {
  localStorage.clear();
  const t = Date.now();
  const p = (x) => ({ type: "paragraph", content: [{ type: "text", text: x }] });
  const cell = (kind, x) => ({ type: kind, attrs: { colspan: 1, rowspan: 1, colwidth: null }, content: [p(x)] });
  localStorage.setItem("tuxnotes-mock-v1", JSON.stringify({
    folders: [{ id: "f0", name: "Notes", isDefault: true, parentId: null, sortOrder: 0, createdAt: t, updatedAt: t }],
    notes: [{ id: "a", folderId: "f0", title: "Lists", snippet: "", plain: "", isPinned: false, createdAt: t, updatedAt: t, deletedAt: null, isLocked: false, content: { type: "doc", content: [
      p("Lists"),
      { type: "taskList", content: [{ type: "taskItem", attrs: { checked: false }, content: [p("Buy mlik")] }, { type: "taskItem", attrs: { checked: true }, content: [p("Call mum")] }] },
      { type: "table", content: [
        { type: "tableRow", content: [cell("tableHeader", "Item"), cell("tableHeader", "Qty")] },
        { type: "tableRow", content: [cell("tableCell", "Bread"), cell("tableCell", "2")] },
      ] },
    ] } }],
  }));
});
await page.goto(BASE + "/", { waitUntil: "load" }); await sleep(600);
await page.click('.note[data-id="a"]'); await sleep(300);

const text = await rightClick('ul[data-type="taskList"] li div p');
check("right-click on checklist text keeps the system menu (spelling, cut, copy, paste)", !text.custom && text.menu.length === 0, JSON.stringify(text));
const box = await rightClick('ul[data-type="taskList"] li label');
check("right-click on the checkbox shows the checklist actions", box.custom && box.menu.includes("Check All"), JSON.stringify(box.menu));
await closeMenus();
const cellText = await rightClick("td p");
check("right-click on text in a table cell keeps the system menu", !cellText.custom && cellText.menu.length === 0, JSON.stringify(cellText));

// A multi-cell selection: drag from one cell to another, then right-click it
const c1 = await (await page.$("td:nth-child(1) p")).boundingBox();
const c2 = await (await page.$("td:nth-child(2) p")).boundingBox();
await page.mouse.move(c1.x + 5, c1.y + 5); await page.mouse.down(); await page.mouse.move(c2.x + 5, c2.y + 5, { steps: 5 }); await page.mouse.up(); await sleep(200);
const sel = await rightClick("td.selectedCell");
check("right-click on selected cells shows the table actions", sel.custom && sel.menu.some((m) => m.includes("Merge")), JSON.stringify(sel.menu));
await closeMenus();

// The toolbar's table button still offers the table actions
await page.click("td p"); await sleep(150);
await page.click('button[aria-label="Table options"]'); await sleep(200);
const tb = await page.$$eval(".menu button", (els) => els.map((e) => e.textContent.trim()));
check("the toolbar's table button shows the table actions", tb.some((m) => /Row/.test(m)), JSON.stringify(tb.slice(0, 4)));

console.log(`\n${failures ? failures + " FAILED" : "ALL PASSED"} | page errors: ${errors.length ? JSON.stringify(errors) : "none"}`);
await b.close();
process.exit(failures ? 1 : 0);
