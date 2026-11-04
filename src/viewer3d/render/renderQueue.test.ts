// MCP renders (renderForRequest): a saved view renders the model as it is,
// not the window's toggles, and a render already running is waited for.
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Camera, DocState } from "../../contract/bindings";

const jobs: { view: unknown; release: () => void }[] = [];

vi.mock("./renderJob", async (orig) => {
  const real = await orig<typeof import("./renderJob")>();
  class FakeJob {
    preview = null;
    private done: Promise<void>;
    constructor(input: { view: unknown }) {
      let release = () => {};
      this.done = new Promise<void>((r) => (release = r));
      jobs.push({ view: input.view, release });
    }
    async run() {
      await this.done;
      return { kind: "path_traced", png: "data:image/png;base64,", width: 8, height: 8, samples: 10, seconds: 1, gpu: "" };
    }
    stop() {}
  }
  return { ...real, RenderJob: FakeJob };
});

vi.mock("../../contract/ipc", async (orig) => {
  const real = await orig<typeof import("../../contract/ipc")>();
  let n = 0;
  return { ...real, ipc: { ...real.ipc, renderCapture: async () => ({ id: `r${++n}` }) } };
});

// Node has no window: the queue only adds and removes its Esc listener on it.
(globalThis as { window?: unknown }).window ??= { addEventListener() {}, removeEventListener() {}, location: { search: "" } };

const { renderForRequest, useRenderQueue } = await import("./renderQueue");
const { useApp } = await import("../../state/store");
const { useViewer } = await import("../viewerStore");

const camera: Camera = {
  id: "cam-front",
  name: "Front",
  position: { x: 0, y: -9000, z: 1600 },
  target: { x: 0, y: 0, z: 1600 },
  fov_deg: 60,
  preset: "custom",
  light: null,
} as Camera;

function doc(): DocState {
  return { revision: 3, project: { elements: [{ kind: "camera", ...camera }] } } as unknown as DocState;
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("renders for an MCP client", () => {
  beforeEach(() => {
    jobs.length = 0;
    useApp.setState({ doc: doc() });
    // The window has the roof off and the cutaway on: the client cannot see that.
    useViewer.setState({ roofVisible: false, cutaway: true });
  });

  it("renders a saved view with the roof on and no cutaway", async () => {
    const done = renderForRequest(["cam-front"], "quick", "hd");
    await vi.waitFor(() => expect(jobs).toHaveLength(1));
    expect(jobs[0].view).toEqual({ cutaway: false, roofVisible: true, activeLevelId: null });
    jobs[0].release();
    const results = await done;
    expect(results.map((r) => r.name)).toEqual(["Front"]);
    // The window is free the moment the render is saved.
    expect(useRenderQueue.getState().running).toBe(false);
  });

  it("shows exactly the roof and cutaway the request asks for", async () => {
    useApp.setState({ activeLevelId: "L2" });
    const done = renderForRequest(["cam-front"], "quick", "hd", { roof: false, cutaway: true });
    await vi.waitFor(() => expect(jobs).toHaveLength(1));
    expect(jobs[0].view).toEqual({ cutaway: true, roofVisible: false, activeLevelId: "L2" });
    jobs[0].release();
    await done;
  });

  it("waits for a render already running instead of refusing", async () => {
    const first = renderForRequest(["cam-front"], "quick", "hd");
    await vi.waitFor(() => expect(jobs).toHaveLength(1));
    const second = renderForRequest(["cam-front"], "quick", "hd");
    await tick();
    expect(jobs).toHaveLength(1);
    jobs[0].release();
    await first;
    await vi.waitFor(() => expect(jobs).toHaveLength(2));
    jobs[1].release();
    expect((await second).length).toBe(1);
  });
});
