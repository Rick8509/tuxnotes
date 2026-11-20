// Browser tests: runs every tests/e2e/*.test.mjs against the built app in the browser
// preview (with the mock backend in src/lib/mock.ts). Run with `npm run test:e2e`.
// Needs Google Chrome or Chromium: set CHROME=/path/to/chrome if it isn't found.
import { spawn } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const port = process.env.E2E_PORT ?? "4179";
const base = `http://localhost:${port}`;
const chrome =
  process.env.CHROME ??
  ["/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find(existsSync);
if (!chrome) {
  console.error("No Chrome or Chromium found. Set CHROME=/path/to/chrome.");
  process.exit(2);
}
const only = process.argv.slice(2); // optional: names to run, e.g. "ctrl-delete"
const files = readdirSync(here)
  .filter((f) => f.endsWith(".test.mjs"))
  .filter((f) => only.length === 0 || only.some((o) => f.startsWith(o)))
  .sort();

const preview = spawn("npx", ["vite", "preview", "--port", port, "--strictPort"], { stdio: "ignore", detached: true });
const stop = () => {
  try {
    process.kill(-preview.pid);
  } catch {
    /* already gone */
  }
};
process.on("exit", stop);
for (let i = 0; ; i++) {
  try {
    if ((await fetch(base)).ok) break;
  } catch {
    /* not up yet */
  }
  if (i > 100) {
    console.error(`The preview at ${base} didn't start. Did you run "npm run build"?`);
    process.exit(2);
  }
  await new Promise((r) => setTimeout(r, 200));
}

let failed = 0;
for (const file of files) {
  const output = await new Promise((resolve) => {
    let text = "";
    const child = spawn(process.execPath, [join(here, file)], { env: { ...process.env, BASE_URL: base, CHROME: chrome } });
    child.stdout.on("data", (d) => (text += d));
    child.stderr.on("data", (d) => (text += d));
    child.on("close", (code) => resolve({ code, text }));
  });
  const ok = output.code === 0;
  if (!ok) failed++;
  console.log(`${ok ? "PASS" : "FAIL"}  ${file.replace(".test.mjs", "")}`);
  if (!ok) console.log(output.text.split("\n").filter((l) => l.startsWith("FAIL") || /Error/.test(l)).map((l) => `      ${l}`).join("\n"));
}
console.log(`\n${files.length - failed} of ${files.length} browser test files passed`);
stop();
process.exit(failed ? 1 : 0);
