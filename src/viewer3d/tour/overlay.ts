// What lies over the 3D view while a tour plays (lane L9): the held frame a
// dissolve or a dip starts from, the dip color, the 2D plan of a plan lift,
// the whip pan's smear, and a thin progress bar. The engine draws only the
// camera; the layers come from the frames (`fxLayers`), the same ones the
// video export composites.

import type { FxLayers } from "./frames";
import s from "./overlay.module.css";

export class TourOverlay {
  private root: HTMLDivElement;
  private held: HTMLCanvasElement;
  private black: HTMLDivElement;
  private white: HTMLDivElement;
  private plan: HTMLImageElement;
  private bar: HTMLDivElement;
  private blurred = false;

  constructor(
    host: HTMLElement,
    private canvas: HTMLCanvasElement,
  ) {
    this.root = document.createElement("div");
    this.root.className = s.root;
    this.root.setAttribute("aria-hidden", "true");
    this.held = document.createElement("canvas");
    this.held.className = s.layer;
    this.black = document.createElement("div");
    this.black.className = `${s.layer} ${s.black}`;
    this.white = document.createElement("div");
    this.white.className = `${s.layer} ${s.white}`;
    this.plan = document.createElement("img");
    this.plan.className = `${s.layer} ${s.plan}`;
    this.plan.alt = "";
    this.bar = document.createElement("div");
    this.bar.className = s.progress;
    this.root.append(this.held, this.plan, this.black, this.white, this.bar);
    host.appendChild(this.root);
  }

  /** Keeps a picture to fade from. */
  hold(source: CanvasImageSource, width: number, height: number): void {
    this.held.width = Math.max(1, Math.round(width));
    this.held.height = Math.max(1, Math.round(height));
    this.held.getContext("2d")?.drawImage(source, 0, 0, this.held.width, this.held.height);
  }

  setPlan(url: string | null): void {
    if (url) this.plan.src = url;
    else this.plan.removeAttribute("src");
  }

  get hasPlan(): boolean {
    return !!this.plan.getAttribute("src");
  }

  apply(layers: FxLayers, progress: number): void {
    this.held.style.opacity = String(layers.held);
    this.black.style.opacity = String(layers.color === "black" ? layers.colorAlpha : 0);
    this.white.style.opacity = String(layers.color === "white" ? layers.colorAlpha : 0);
    this.plan.style.opacity = String(this.hasPlan ? layers.plan : 0);
    this.bar.style.transform = `scaleX(${Math.min(Math.max(progress, 0), 1)})`;
    // The smear: a blur on the canvas, strongest mid swing. The export
    // smears sideways only (render/tourJob.ts); a CSS filter cannot.
    const px = layers.blur * 14;
    if (px > 0.3) {
      this.canvas.style.filter = `blur(${px.toFixed(1)}px)`;
      this.blurred = true;
    } else if (this.blurred) {
      this.canvas.style.filter = "";
      this.blurred = false;
    }
  }

  dispose(): void {
    if (this.blurred) this.canvas.style.filter = "";
    this.root.remove();
  }
}
