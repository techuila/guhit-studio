// Renders a tour's frames for its video (lane L9): the live raster look,
// refined with jittered samples, at the video's size, off screen. Never
// calls requestAnimationFrame (AGENTS.md, the 3D frame loop invariant):
// each frame is drawn by `ViewerEngine.captureCanvas` (the canvas at the
// video size for that one call, the live size back right after), and the
// next frame waits on a GPU fence for this one (render/pacing.ts), so the
// app stays responsive and the GPU queue stays short.
//
// Each frame:
// 1. the camera, the light and the open doors of the frame
//    (`Query::TourFrames`, `doorsAt`), the doors in the view only. The
//    camera is the job's own (`ViewerEngine.viewCamera`), never the live
//    view's: a preset, a saved view or an agent's look while the video
//    exports moves the view, never the video (GR3 P1-1);
// 2. the capture's render, which also works out the ambient occlusion, then
//    `samples` jittered renders averaged and tone mapped (light/refine.ts);
// 3. the transition laid over it on a 2D canvas, exactly as playback does
//    (`fxLayers`): the held frame, the dip color, the 2D plan, and a
//    sideways smear for a whip pan.

import * as THREE from "three";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { fromViewLight } from "../light/model";
import { Accumulator, canAccumulate } from "../light/refine";
import { doorsAt, fxLayers, heldFrames, type TourFrames } from "../tour/frames";
import { gpuFence, nextTask } from "./pacing";

/** Jittered samples per frame: clean edges at about four times a live frame. */
export const TOUR_SAMPLES = 4;

const PAPER = "#f7f5f0";

export class TourJob {
  /** The finished picture of the last frame. */
  readonly canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private acc: Accumulator | null;
  private held = new Map<number, HTMLCanvasElement>();
  private keep: Set<number>;
  private smear: HTMLCanvasElement | null = null;
  private plan: HTMLImageElement | null = null;
  private predictedMs = Number.NaN;
  /** The video's camera, set to each frame's pose. */
  readonly camera = new THREE.PerspectiveCamera();

  constructor(
    private engine: ViewerEngine,
    private data: TourFrames,
    readonly width: number,
    readonly height: number,
    private samples = TOUR_SAMPLES,
  ) {
    this.canvas = document.createElement("canvas");
    this.canvas.width = width;
    this.canvas.height = height;
    const ctx = this.canvas.getContext("2d", { alpha: false, willReadFrequently: false });
    if (!ctx) throw new Error("This computer cannot draw the video frames.");
    this.ctx = ctx;
    this.acc = samples > 1 && canAccumulate(engine.renderer) ? new Accumulator() : null;
    this.keep = heldFrames(data);
  }

  /** The 2D plan a plan lift starts from, when there is one. */
  setPlan(img: HTMLImageElement | null): void {
    this.plan = img;
  }

  /** Draws frame i into `canvas`, then waits for the GPU to finish it. */
  async frame(i: number): Promise<HTMLCanvasElement> {
    const f = this.data.frames[i];
    const engine = this.engine;
    const { width: w, height: h } = this;
    const cam = engine.viewCamera(f, this.camera);
    engine.setTourDoors(doorsAt(this.data, f.t));
    const draw = () =>
      engine.captureCanvas(
        w,
        h,
        (gl) => {
          const acc = this.acc;
          if (acc) {
            const r = engine.renderer;
            acc.ensure(w, h);
            acc.reset();
            for (let s = 0; s < this.samples; s++) acc.sample(r, engine.scene, cam);
            r.setRenderTarget(null);
            acc.display(r);
          }
          this.ctx.globalAlpha = 1;
          this.ctx.drawImage(gl, 0, 0, w, h);
        },
        cam,
      );
    const t0 = performance.now();
    if (f.light) engine.lightRig().withLight(fromViewLight(f.light), draw);
    else draw();
    this.compose(i);
    // The next frame waits for this one on the GPU, without blocking the page.
    const gl = engine.renderer.getContext() as WebGL2RenderingContext;
    if (typeof gl.fenceSync === "function") {
      const hidden = typeof document !== "undefined" && document.visibilityState === "hidden";
      await gpuFence(gl, this.predictedMs, hidden, () => gl.isContextLost());
    } else {
      await nextTask();
    }
    this.predictedMs = performance.now() - t0;
    return this.canvas;
  }

  /** The transition over the picture, then keeps it when a later frame holds it. */
  private compose(i: number): void {
    const f = this.data.frames[i];
    const layers = fxLayers(f.fx);
    const ctx = this.ctx;
    const { width: w, height: h } = this;
    if (layers.blur > 0.01) this.smearSideways(layers.blur);
    const from = f.fx?.from_frame;
    if (layers.held > 0 && from !== undefined) {
      const held = this.held.get(from);
      if (held) {
        ctx.globalAlpha = layers.held;
        ctx.drawImage(held, 0, 0, w, h);
      }
    }
    if (layers.plan > 0 && this.plan) {
      ctx.globalAlpha = layers.plan;
      ctx.fillStyle = PAPER;
      ctx.fillRect(0, 0, w, h);
      const img = this.plan;
      const k = Math.min(w / img.naturalWidth, h / img.naturalHeight);
      const dw = img.naturalWidth * k;
      const dh = img.naturalHeight * k;
      ctx.drawImage(img, (w - dw) / 2, (h - dh) / 2, dw, dh);
    }
    if (layers.color && layers.colorAlpha > 0) {
      ctx.globalAlpha = layers.colorAlpha;
      ctx.fillStyle = layers.color === "black" ? "#000" : "#fff";
      ctx.fillRect(0, 0, w, h);
    }
    ctx.globalAlpha = 1;
    if (this.keep.has(i)) {
      const copy = document.createElement("canvas");
      copy.width = w;
      copy.height = h;
      copy.getContext("2d")?.drawImage(this.canvas, 0, 0);
      this.held.set(i, copy);
    }
    // A held frame is used by the transition right after it; drop older ones.
    for (const k of this.held.keys()) if (k < i - this.data.fps * 12) this.held.delete(k);
  }

  /** A whip pan's motion blur: the picture averaged over a sideways sweep. */
  private smearSideways(amount: number): void {
    const { width: w, height: h } = this;
    const src = (this.smear ??= document.createElement("canvas"));
    src.width = w;
    src.height = h;
    const sctx = src.getContext("2d");
    if (!sctx) return;
    sctx.drawImage(this.canvas, 0, 0);
    const reach = amount * w * 0.05;
    const taps = 9;
    const ctx = this.ctx;
    for (let k = 0; k < taps; k++) {
      // A running average: tap k is weighed 1 / (k + 1) over the ones before.
      ctx.globalAlpha = 1 / (k + 1);
      const dx = (k / (taps - 1) - 0.5) * 2 * reach;
      ctx.drawImage(src, dx, 0, w, h);
    }
    ctx.globalAlpha = 1;
  }

  /** Ends the job: the doors it opened rest as drawn again. */
  dispose(): void {
    this.acc?.dispose();
    this.held.clear();
    this.engine.setTourDoors(null);
  }
}
