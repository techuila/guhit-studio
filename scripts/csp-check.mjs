// Checks the production build under the desktop app's release CSP.
//
// The browser dev setup runs with no CSP at all, so a fetch, worker or
// WebAssembly module the webview refuses only shows up in the installed app.
// This serves `dist/` with the `csp` from src-tauri/tauri.conf.json (plus the
// dev bridge's origin, which stands in for Tauri's IPC), opens the services
// demo in 3D and fails on any CSP violation or console error.
//
//   pnpm build && pnpm bridge      # the bridge on :1430, in another terminal
//   node scripts/csp-check.mjs
import { chromium } from "playwright-core";
import { existsSync, readdirSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { homedir } from "node:os";
import { extname, join, normalize, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");
const DIST = join(ROOT, "dist");
const BRIDGE = process.env.BRIDGE_URL ?? "http://localhost:1430";
const PORT = Number(process.env.PORT ?? 1480);

function findChromium() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  const cache = join(homedir(), "Library/Caches/ms-playwright");
  const dirs = existsSync(cache) ? readdirSync(cache).filter((d) => d.startsWith("chromium_headless_shell-")).sort() : [];
  for (const d of dirs.reverse()) {
    for (const sub of readdirSync(join(cache, d))) {
      for (const name of ["headless_shell", "chrome-headless-shell"]) {
        const bin = join(cache, d, sub, name);
        if (existsSync(bin)) return bin;
      }
    }
  }
  throw new Error("No cached Chromium found. Set CHROMIUM_PATH.");
}

async function ipc(cmd, args) {
  const res = await fetch(`${BRIDGE}/ipc/${cmd}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(args) });
  const body = await res.json();
  if (!res.ok) throw new Error(`${cmd}: ${JSON.stringify(body)}`);
  return body;
}

if (!existsSync(join(DIST, "index.html"))) {
  console.error("No dist/ build. Run pnpm build first.");
  process.exit(2);
}
const conf = JSON.parse(await readFile(join(ROOT, "src-tauri/tauri.conf.json"), "utf8"));
const csp = conf.app.security.csp.replace(/connect-src ([^;]*)/, (_, list) => `connect-src ${list} ${BRIDGE}`);

const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".png": "image/png", ".webp": "image/webp", ".jpg": "image/jpeg", ".glb": "model/gltf-binary", ".svg": "image/svg+xml", ".wasm": "application/wasm" };
const server = createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)).replace(/^(\.\.[/\\])+/, "");
  const file = join(DIST, path === "/" ? "index.html" : path);
  let body;
  try {
    body = await readFile(file);
  } catch {
    body = await readFile(join(DIST, "index.html"));
  }
  res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream", "content-security-policy": csp });
  res.end(body);
}).listen(PORT);

const name = `CSP check ${Date.now()}`;
let projectId = null;
let failures = 0;
const browser = await chromium.launch({
  executablePath: findChromium(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
try {
  const doc = await ipc("hub_create", { name, settings: null, template: "plumbing-demo" });
  projectId = doc.project.id;
  await ipc("hub_close", {});
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.on("console", (m) => {
    if (m.type() !== "error") return;
    failures++;
    console.log(`[console.error] ${m.text()}`);
  });
  page.on("pageerror", (e) => {
    failures++;
    console.log(`[pageerror] ${e.message}`);
  });
  await page.addInitScript(() => {
    window.__csp = [];
    document.addEventListener("securitypolicyviolation", (e) => window.__csp.push(`${e.violatedDirective} ${e.blockedURI}`));
  });
  await page.goto(`http://localhost:${PORT}/`, { waitUntil: "networkidle" });
  await page.click(`[aria-label="Open ${name}"]`);
  await page.waitForSelector('nav[aria-label="Drawing tools"]');
  await page.locator('button:has-text("3D")').first().click();
  // The pack's models, textures and sky load after the first frame.
  await page.waitForTimeout(9000);
  const violations = await page.evaluate(() => window.__csp);
  for (const v of violations) console.log(`[csp] refused: ${v}`);
  failures += violations.length;
} finally {
  await browser.close();
  server.close();
  if (projectId) await ipc("hub_delete", { id: projectId }).catch(() => undefined);
}
console.log(failures ? `CSP CHECK FAILED: ${failures} problem(s)` : "CSP check passed: nothing refused, no console errors");
process.exit(failures ? 1 : 0);
