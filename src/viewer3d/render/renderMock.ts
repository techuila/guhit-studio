// Dev-only mock of the AI render commands, so the studio can be built and
// checked before the provider lands in the backend. Never loaded in a
// production build: RenderPanel imports it behind import.meta.env.DEV and the
// `renderai` query flag.
//
//   ?renderai=mock     a key is configured; generate returns a derived image
//   ?renderai=nokey    no key: settings say so and generate fails with ai_not_configured
//   ?renderai=error    a key is configured but the provider fails
//   &renderdelay=4000  how long the fake provider takes (default 1200 ms)
//
// The fake image is the source capture put through a canvas filter. It is not
// a render and never claims to be one: it only exercises the UI.

import type { RenderAiRequest, RenderAiSettings, RenderRecord } from "../../contract/bindings";

type Mode = "mock" | "nokey" | "error";

const BRIDGE_PATTERN = /\/ipc\/([a-z_]+)$/;

let installed = false;
const extraRecords: RenderRecord[] = [];
const extraImages = new Map<string, string>();

function fail(code: string, message: string): never {
  throw { code, message, element_ids: [] };
}

async function derive(png: string): Promise<string> {
  const img = new Image();
  await new Promise<void>((resolve, reject) => {
    img.onload = () => resolve();
    img.onerror = () => reject(new Error("image did not load"));
    img.src = png;
  });
  const canvas = document.createElement("canvas");
  canvas.width = img.naturalWidth || 1301;
  canvas.height = img.naturalHeight || 800;
  const ctx = canvas.getContext("2d");
  if (!ctx) return png;
  ctx.filter = "saturate(1.5) contrast(1.12) sepia(0.22) brightness(1.05)";
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  ctx.filter = "none";
  const sky = ctx.createLinearGradient(0, 0, 0, canvas.height * 0.55);
  sky.addColorStop(0, "rgba(255, 176, 92, 0.30)");
  sky.addColorStop(1, "rgba(255, 176, 92, 0)");
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, canvas.width, canvas.height * 0.55);
  return canvas.toDataURL("image/png");
}

/** Installs the fake render_ai_* commands. Returns false when no flag is set. */
export function installRenderMock(): boolean {
  if (installed) return true;
  const params = new URLSearchParams(window.location.search);
  const flag = params.get("renderai");
  if (flag !== "mock" && flag !== "nokey" && flag !== "error") return false;
  const mode = flag as Mode;
  const delay = Number(params.get("renderdelay") ?? 1200);

  let settings: RenderAiSettings = {
    provider: "gemini",
    has_api_key: mode !== "nokey",
    model: "gemini-3.1-flash-image",
    cost_hint: "About $0.05 per draft, $0.10 per standard, $0.24 per high image (Google list price)",
  };

  const previous = window.fetch.bind(window);
  const json = (status: number, body: unknown) =>
    new Response(JSON.stringify(body ?? null), { status, headers: { "content-type": "application/json" } });

  window.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const m = BRIDGE_PATTERN.exec(url);
    const cmd = m?.[1];
    const args = init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : {};

    try {
      if (cmd === "render_ai_settings_get") return json(200, settings);
      if (cmd === "render_ai_settings_set") {
        const key = args.api_key as string | null;
        if (key !== null && key !== undefined) settings = { ...settings, has_api_key: key !== "" };
        const model = args.model as string | null;
        if (model) settings = { ...settings, model };
        return json(200, settings);
      }
      if (cmd === "render_ai_generate") {
        if (mode === "nokey") fail("ai_not_configured", "No Google AI Studio key is configured.");
        const request = args.request as RenderAiRequest;
        await new Promise((r) => setTimeout(r, delay));
        if (mode === "error") fail("ai_failed", "The image provider refused the request: quota exhausted.");
        const list = (await (await previous(url.replace("render_ai_generate", "render_list"), { ...init, body: "{}" })).json()) as RenderRecord[];
        const source = list.find((r) => r.id === request.source_render_id);
        if (!source) fail("not_found", "The source capture is gone.");
        const png = (await (
          await previous(url.replace("render_ai_generate", "render_data"), {
            ...init,
            body: JSON.stringify({ id: source.id }),
          })
        ).json()) as string;
        const id = `mock-ai-${Math.random().toString(36).slice(2, 9)}`;
        extraImages.set(id, await derive(png));
        const record: RenderRecord = {
          id,
          created_at: new Date().toISOString(),
          source: "ai_visualization",
          revision: source.revision,
          camera: source.camera,
          style_key: request.style_key,
          prompt: request.prompt,
          image_path: `/mock/renders/${id}.png`,
          source_render_id: source.id,
          provider: "gemini/gemini-3.1-flash-image",
          info: null,
        };
        extraRecords.unshift(record);
        return json(200, { record, seconds: delay / 1000 });
      }
      if (cmd === "render_list") {
        const real = (await (await previous(input, init)).json()) as RenderRecord[];
        const alive = extraRecords.filter((r) => real.some((x) => x.id === r.source_render_id));
        return json(200, [...alive, ...real]);
      }
      if (cmd === "render_data" && extraImages.has(String(args.id))) {
        return json(200, extraImages.get(String(args.id)));
      }
      if (cmd === "render_delete" && extraImages.has(String(args.id))) {
        const id = String(args.id);
        extraImages.delete(id);
        const i = extraRecords.findIndex((r) => r.id === id);
        if (i >= 0) extraRecords.splice(i, 1);
        return json(200, null);
      }
    } catch (e) {
      return json(400, e);
    }
    return previous(input, init);
  };

  installed = true;
  console.info(`[dev] render AI mock installed (${mode})`);
  return true;
}
