// Headless UI check for development. Drives the app in Chromium, saves
// screenshots, prints console errors. Parallel-safe: each run is its own browser.
//
//   node scripts/ui-check.mjs <url> <out.png> [steps.mjs]
//
// steps.mjs (optional) default-exports `async (page, shot) => {}` where
// `shot(name)` saves another screenshot next to <out.png>.
import { chromium } from "playwright-core";
import { readdirSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

function findChromium() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  const cache = join(homedir(), "Library/Caches/ms-playwright");
  const dirs = existsSync(cache) ? readdirSync(cache).filter((d) => d.startsWith("chromium_headless_shell-")).sort() : [];
  for (const d of dirs.reverse()) {
    for (const sub of readdirSync(join(cache, d))) {
      const bin = join(cache, d, sub, "headless_shell");
      if (existsSync(bin)) return bin;
      const bin2 = join(cache, d, sub, "chrome-headless-shell");
      if (existsSync(bin2)) return bin2;
    }
  }
  throw new Error("No cached Chromium found. Set CHROMIUM_PATH.");
}

const [url, out = "shot.png", stepsFile] = process.argv.slice(2);
if (!url) {
  console.error("usage: node scripts/ui-check.mjs <url> <out.png> [steps.mjs]");
  process.exit(2);
}
const browser = await chromium.launch({
  executablePath: findChromium(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
let errors = 0;
page.on("console", (m) => {
  if (m.type() === "error" || m.type() === "warning") {
    if (m.type() === "error") errors++;
    console.log(`[console.${m.type()}] ${m.text()}`);
  }
});
page.on("pageerror", (e) => {
  errors++;
  console.log(`[pageerror] ${e.message}`);
});
const shot = async (name) => {
  const p = name ? join(dirname(out), name.endsWith(".png") ? name : `${name}.png`) : out;
  await page.screenshot({ path: p });
  console.log(`screenshot: ${p}`);
};
try {
  await page.goto(url, { waitUntil: "networkidle" });
  if (stepsFile) {
    const steps = (await import(pathToFileURL(resolve(stepsFile)).href)).default;
    await steps(page, shot);
  }
  await shot();
} finally {
  await browser.close();
}
console.log(errors ? `done with ${errors} error(s)` : "done, no console errors");
