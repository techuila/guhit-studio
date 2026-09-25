// Live session check on one computer (DECISIONS D29, D30, D31).
//
// Two dev bridges stand in for two computers, each with its own data folder;
// one Vite dev server serves the UI, and two browser tabs point at the two
// bridges with `?bridge=`. Ana hosts the services demo from the Share dialog,
// Ben joins from the hub with the invite. Then it checks, both ways: the
// other's pointer on the plan, cursor chat, the Chat panel, an edit crossing
// over, undo of someone else's step asking first, and the end of the session.
// It also drives the MCP endpoint of Ana's bridge: get_selection, the
// "Only the selection" limit, get_session, send_chat_message, and a plan
// picture drawn by Ana's tab.
//
//   cargo build -p guhit-devbridge
//   node scripts/live-check.mjs            # screenshots go to ./live-check/
//
// Environment: CHROMIUM_PATH (a Chromium or headless shell), OUT (screenshot
// folder), KEEP=1 (leave the bridges' data folders for a look).

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");
const OUT = resolve(process.env.OUT ?? join(ROOT, "live-check"));
const BRIDGE_BIN = join(ROOT, "target", "debug", process.platform === "win32" ? "guhit-devbridge.exe" : "guhit-devbridge");
const A = { name: "Ana", port: 1440 };
const B = { name: "Ben", port: 1441 };
const UI_PORT = 1427;

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

// ------------------------------------------------------------------ helpers

const children = [];
function start(cmd, args, env = {}) {
  const child = spawn(cmd, args, { cwd: ROOT, env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] });
  child.stdout.on("data", () => {});
  child.stderr.on("data", (d) => {
    const line = String(d).trim();
    if (/error|panic/i.test(line)) console.log(`[${args[1] ?? cmd}] ${line}`);
  });
  children.push(child);
  return child;
}

async function until(what, fn, ms = 20000) {
  const startAt = Date.now();
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // not yet
    }
    if (Date.now() - startAt > ms) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 250));
  }
}

function bridgeUrl(who) {
  return `http://localhost:${who.port}`;
}

async function ipc(who, cmd, args = {}) {
  const res = await fetch(`${bridgeUrl(who)}/ipc/${cmd}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(args) });
  const body = await res.json();
  if (!res.ok) throw Object.assign(new Error(`${cmd}: ${body.message}`), { body });
  return body;
}

let rpcId = 1;
async function tool(who, name, args = {}) {
  const res = await fetch(`${bridgeUrl(who)}/mcp`, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json, text/event-stream" },
    body: JSON.stringify({ jsonrpc: "2.0", id: rpcId++, method: "tools/call", params: { name, arguments: args } }),
  });
  const body = await res.json();
  if (body.error) throw new Error(`mcp ${name}: ${JSON.stringify(body.error)}`);
  const r = body.result;
  return {
    isError: !!r.isError,
    text: r.content?.find((c) => c.type === "text")?.text ?? "",
    json: r.structuredContent ?? null,
    images: (r.content ?? []).filter((c) => c.type === "image"),
  };
}

const results = [];
function check(name, ok, detail = "") {
  results.push({ name, ok: !!ok });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? `: ${String(detail).slice(0, 220)}` : ""}`);
}

async function openTab(browser, who) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.errors = 0;
  page.on("console", (m) => {
    if (m.type() !== "error") return;
    page.errors++;
    console.log(`[${who.name} console.error] ${m.text()}`);
  });
  page.on("pageerror", (e) => {
    page.errors++;
    console.log(`[${who.name} pageerror] ${e.message}`);
  });
  await page.goto(`http://localhost:${UI_PORT}/?bridge=${encodeURIComponent(bridgeUrl(who))}`, { waitUntil: "networkidle" });
  return page;
}

async function shot(page, name) {
  await page.screenshot({ path: join(OUT, `${name}.png`) });
}

/** A point of the plan, in page pixels, from the plan controller's view. */
async function planPoint(page, x, y) {
  return page.evaluate(
    ([px, py]) => {
      const c = window.__planController;
      const r = c.canvas.getBoundingClientRect();
      return { x: r.left + c.view.ox + px * c.view.scale, y: r.top + c.view.oy - py * c.view.scale };
    },
    [x, y],
  );
}

// -------------------------------------------------------------------- run

if (!existsSync(BRIDGE_BIN)) {
  console.error(`No dev bridge at ${BRIDGE_BIN}. Build it first: cargo build -p guhit-devbridge`);
  process.exit(2);
}
mkdirSync(OUT, { recursive: true });
const data = mkdtempSync(join(tmpdir(), "guhit-live-check-"));
start(BRIDGE_BIN, ["--port", String(A.port), "--data", join(data, "ana")]);
start(BRIDGE_BIN, ["--port", String(B.port), "--data", join(data, "ben")]);
start(process.execPath, [join(ROOT, "node_modules", "vite", "bin", "vite.js"), "--port", String(UI_PORT), "--strictPort"]);

let browser;
try {
  await until("the bridges", async () => (await fetch(`${bridgeUrl(A)}/health`)).ok && (await fetch(`${bridgeUrl(B)}/health`)).ok);
  await until("the UI", async () => (await fetch(`http://localhost:${UI_PORT}/`)).ok, 60000);

  const project = `Live check ${Date.now()}`;
  const doc = await ipc(A, "hub_create", { name: project, settings: null, template: "plumbing-demo" });
  await ipc(A, "hub_close", {});

  browser = await chromium.launch({
    executablePath: findChromium(),
    args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
  });
  const ana = await openTab(browser, A);
  const ben = await openTab(browser, B);

  // ---- Ana hosts from the Share dialog.
  await ana.click(`[aria-label="Open ${project}"]`);
  await ana.waitForSelector('nav[aria-label="Drawing tools"]');
  await ana.getByRole("button", { name: "Share" }).click();
  const anaName = ana.getByPlaceholder("How others see you");
  if (await anaName.count()) await anaName.fill(A.name);
  await ana.getByRole("button", { name: "Start live session" }).click();
  const invite = await until("the invite", async () => ana.locator('[aria-label="Invite"]').inputValue(), 15000);
  check("Ana hosts and gets an invite", invite.startsWith("guhit-live:"), `${invite.length} characters`);
  await shot(ana, "01-ana-share-hosting");
  await ana.keyboard.press("Escape");

  // ---- Ben joins from the hub.
  await ben.getByRole("button", { name: /Join a live session/ }).first().click();
  await ben.getByPlaceholder("Paste the invite the host sent you").fill(invite);
  const benName = ben.getByPlaceholder("How others see you");
  if (await benName.count()) await benName.fill(B.name);
  await shot(ben, "02-ben-join-dialog");
  await ben.getByRole("button", { name: "Join", exact: true }).click();
  await ben.waitForSelector('nav[aria-label="Drawing tools"]', { timeout: 20000 });
  const benDoc = await ben.evaluate(() => window.__app.getState().doc?.project.name);
  check("Ben sees the shared project", benDoc === project, benDoc);
  const people = await until("two participants", async () => {
    const st = await ipc(A, "live_status");
    return st.participants.length === 2 ? st : null;
  });
  check("Ana sees Ben in the session", people.participants.map((p) => p.name).join(",") === `${A.name},${B.name}`, people.participants.map((p) => `${p.name}:${p.color}`).join(" "));

  // ---- Pointers cross over.
  const kitchen = doc.project.elements.find((e) => e.kind === "room");
  const room = doc.derived.rooms.find((r) => r.room_id === kitchen.id);
  const at = await planPoint(ana, room.label_point.x, room.label_point.y - 600);
  await ana.mouse.move(at.x - 40, at.y - 40);
  await ana.mouse.move(at.x, at.y, { steps: 8 });
  const anaCursor = await until("Ana's pointer in Ben's tab", async () => {
    const n = await ben.locator('[class*="_cursor_"]').filter({ hasText: A.name }).count();
    return n > 0 ? n : null;
  }, 10000).catch(() => 0);
  check("Ben sees Ana's pointer with her name", anaCursor > 0);

  // ---- Cursor chat: "/" at Ana's pointer.
  await ana.keyboard.press("/");
  await ana.keyboard.type("Dito ang kusina");
  const typing = await until("typing shown to Ben", async () => {
    const t = await ben.locator('[class*="_cursor_"]').filter({ hasText: "Dito ang kusina" }).count();
    return t > 0 ? t : null;
  }, 10000).catch(() => 0);
  check("Ben sees Ana typing at her pointer", typing > 0);
  await shot(ben, "03-ben-sees-ana-typing");
  await ana.keyboard.press("Enter");
  const chat = await until("the message in the chat", async () => {
    const list = await ipc(B, "chat_list");
    return list.find((m) => m.text === "Dito ang kusina") ?? null;
  }, 10000).catch(() => null);
  check("the cursor chat message is in Ben's chat history", !!chat && chat.author_name === A.name && !!chat.at, chat ? `${chat.author_name} at ${JSON.stringify(chat.at)}` : "missing");

  // ---- Ben answers from the Chat panel.
  await ben.getByRole("tab", { name: /Chat/ }).click();
  await ben.getByLabel("Chat message").fill("Sige, ililipat ko ang pinto");
  await ben.getByLabel("Chat message").press("Enter");
  const reply = await until("Ben's reply at Ana", async () => (await ipc(A, "chat_list")).find((m) => m.text.startsWith("Sige")) ?? null, 10000).catch(() => null);
  check("Ben's panel message reaches Ana", !!reply && reply.author_name === B.name);
  await shot(ben, "04-ben-chat-panel");

  // ---- An edit crosses over: Ben renames a room, Ana sees it.
  const renamed = await ben.evaluate(async (id) => {
    const st = window.__app.getState();
    const el = st.doc.project.elements.find((e) => e.id === id);
    const r = await st.dispatch({ type: "update_element", element: { ...el, name: "Kusina ni Ben" } });
    return r?.state.revision ?? null;
  }, kitchen.id);
  check("Ben's edit is applied by the host", renamed !== null, String(renamed));
  const anaSees = await until("Ana's window to show Ben's edit", async () =>
    ana.evaluate((id) => window.__app.getState().doc?.project.elements.find((e) => e.id === id)?.name === "Kusina ni Ben", kitchen.id),
  ).catch(() => false);
  check("Ana's window shows Ben's edit", anaSees);
  const anaState = await ipc(A, "doc_state");
  check("the step is recorded as Ben's", anaState.undo_by === people.participants[1].id, anaState.undo_by);

  // ---- Ana undoes Ben's step: she is asked first.
  await ana.evaluate(() => window.__app.getState().undo());
  const asked = await until("the question", async () => ana.evaluate(() => window.__app.getState().undoConfirm?.message ?? null), 5000).catch(() => null);
  check("undoing Ben's step asks Ana first", !!asked && asked.includes(B.name), asked);
  await shot(ana, "05-ana-undo-asks");
  await ana.getByRole("button", { name: "Undo anyway" }).click();
  const undone = await until("the undo at Ben", async () =>
    ben.evaluate((id) => window.__app.getState().doc?.project.elements.find((e) => e.id === id)?.name !== "Kusina ni Ben", kitchen.id),
  ).catch(() => false);
  check("the forced undo reaches Ben", undone);

  // ---- MCP on Ana's computer.
  await ana.evaluate((id) => window.__app.getState().select([id]), kitchen.id);
  await ana.waitForTimeout(500);
  const selection = await tool(A, "get_selection");
  check("MCP get_selection sees Ana's selection", selection.json?.count === 1 && selection.json.selected[0].id === kitchen.id);
  await ana.evaluate(() => window.__app.getState().setAiScope(true));
  await ana.waitForTimeout(500);
  const bedroom = doc.project.elements.filter((e) => e.kind === "room")[1];
  const refused = await tool(A, "rename_room", { room_id: bedroom.id, name: "Outside" });
  check("MCP edits are held to Ana's selection", refused.isError && refused.text.startsWith("out_of_scope"), refused.text);
  await ana.evaluate(() => window.__app.getState().setAiScope(false));
  const session = await tool(A, "get_session");
  check("MCP get_session lists both people", session.json?.participants?.length === 2, session.text);
  const said = await tool(A, "send_chat_message", { text: "Tapos na ang kusina" });
  check("MCP send_chat_message reaches Ben", !said.isError && !!(await until("the AI message at Ben", async () => (await ipc(B, "chat_list")).find((m) => m.via_ai) ?? null, 8000).catch(() => null)));
  const plan = await tool(A, "get_plan_image");
  check("MCP plan picture drawn by Ana's tab", !plan.isError && plan.json?.source === "window" && plan.images.length === 1, plan.text);
  await shot(ana, "06-ana-after-mcp");
  await shot(ben, "07-ben-after-mcp");

  // ---- Ana ends the session; Ben goes back to the hub with a notice.
  await ipc(A, "live_leave");
  const benOff = await until("Ben's session to end", async () => (await ipc(B, "live_status")).mode === "off", 10000).catch(() => false);
  check("ending the session reaches Ben", benOff);
  const benHub = await until("Ben's hub", async () => ben.evaluate(() => window.__app.getState().screen === "hub"), 10000).catch(() => false);
  check("Ben's window goes back to the hub", benHub);
  await ben.waitForTimeout(600);
  await shot(ben, "08-ben-session-ended");

  check("no console errors in Ana's tab", ana.errors === 0, String(ana.errors));
  check("no console errors in Ben's tab", ben.errors === 0, String(ben.errors));
} catch (e) {
  check("the run finished", false, e instanceof Error ? e.message : String(e));
} finally {
  await browser?.close();
  for (const c of children) c.kill();
  if (!process.env.KEEP) rmSync(data, { recursive: true, force: true });
}

const failed = results.filter((r) => !r.ok).length;
console.log(failed ? `LIVE CHECK FAILED: ${failed} of ${results.length}` : `Live check passed: ${results.length} checks. Screenshots in ${OUT}`);
process.exit(failed ? 1 : 0);
