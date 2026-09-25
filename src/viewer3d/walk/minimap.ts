// Minimap for walk and fly: the walker's level in plan, north up. Walls with
// their door gaps, windows, columns, rooms, stairs (arrow up the flight on the
// level it starts from, down on the level at its top), pipes in their system
// colors, and the walker as a dot with a view cone. Clicking it glides the
// walker there (`toPlan` turns the click into a plan point).
//
// The plan does not change while the walker moves, so it is drawn once into
// an offscreen layer and only blitted afterwards: a moving frame costs one
// image copy and a dot. The engine calls `draw` only when the walker moved or
// the model changed, never on an idle frame.

import type { PipeSystem } from "../../contract/bindings";
import { PIPE_COLOR_HEX, PIPE_COLOR_VAR } from "../../contract/pipes";
import type { CollisionWorld, WallSpan } from "../geom/collision";
import type { Pt } from "../geom/coords";

export interface MinimapPipe {
  system: PipeSystem;
  points: Pt[];
  diameterMm: number;
}

export interface MinimapStair {
  outline: Pt[];
  treads: [Pt, Pt][];
  /** Foot to head, along the middle of the flight. */
  arrow: [Pt, Pt];
  /** True on the level the flight starts from: the arrow points up it. */
  up: boolean;
}

export interface MinimapScene {
  /** Bumped whenever anything below changes. */
  version: number;
  world: CollisionWorld;
  rooms: Pt[][];
  pipes: MinimapPipe[];
  stairs?: MinimapStair[];
}

export interface MinimapWalker {
  x: number;
  y: number;
  /** Plan heading of the view, radians counter-clockwise from east. */
  yaw: number;
  /** Where a glide is taking the walker, drawn as a ring. */
  target?: Pt | null;
}

interface Frame {
  minX: number;
  minY: number;
  scale: number;
  ox: number;
  oy: number;
  h: number;
}

const PAD_MM = 2500;

/** Reads a color token off the element, falling back when the page has none. */
function token(el: Element, name: string, fallback: string): string {
  if (typeof getComputedStyle !== "function") return fallback;
  const v = getComputedStyle(el).getPropertyValue(name).trim();
  return v || fallback;
}

export class Minimap {
  private layer: HTMLCanvasElement | null = null;
  private layerKey = "";
  private frame: Frame | null = null;
  private accent = "#0e8a8f";

  constructor(private canvas: HTMLCanvasElement) {}

  /** Draws the plan (cached) and the walker. Returns false when the canvas has no size yet. */
  draw(scene: MinimapScene, walker: MinimapWalker): boolean {
    const canvas = this.canvas;
    const dpr = Math.min(typeof devicePixelRatio === "number" && devicePixelRatio > 0 ? devicePixelRatio : 1, 2);
    const cw = Math.round(canvas.clientWidth * dpr);
    const ch = Math.round(canvas.clientHeight * dpr);
    if (cw < 2 || ch < 2) return false;
    if (canvas.width !== cw || canvas.height !== ch) {
      canvas.width = cw;
      canvas.height = ch;
    }
    const key = `${scene.version}|${cw}|${ch}|${document.documentElement.dataset.theme ?? ""}`;
    if (key !== this.layerKey || !this.layer) {
      this.layerKey = key;
      this.drawPlan(scene, cw, ch, dpr);
    }
    const ctx = canvas.getContext("2d");
    if (!ctx || !this.layer || !this.frame) return false;
    ctx.clearRect(0, 0, cw, ch);
    ctx.drawImage(this.layer, 0, 0);
    this.drawWalker(ctx, walker, dpr);
    return true;
  }

  private toCanvas(p: Pt): [number, number] {
    const f = this.frame as Frame;
    return [f.ox + (p.x - f.minX) * f.scale, f.h - (f.oy + (p.y - f.minY) * f.scale)];
  }

  /** The plan point under a client (page) point on the minimap, or null before the first draw. */
  toPlan(clientX: number, clientY: number): Pt | null {
    const f = this.frame;
    const rect = this.canvas.getBoundingClientRect();
    if (!f || !(rect.width > 0) || !(f.scale > 0)) return null;
    const k = this.canvas.width / rect.width;
    const cx = (clientX - rect.left) * k;
    const cy = (clientY - rect.top) * k;
    return { x: f.minX + (cx - f.ox) / f.scale, y: f.minY + (f.h - cy - f.oy) / f.scale };
  }

  private drawPlan(scene: MinimapScene, cw: number, ch: number, dpr: number): void {
    const layer = (this.layer ??= document.createElement("canvas"));
    layer.width = cw;
    layer.height = ch;
    const ctx = layer.getContext("2d");
    if (!ctx) return;
    const el = this.canvas;
    const ink = token(el, "--ink", "#1f2d44");
    const ink3 = token(el, "--ink-3", "#7b8796");
    const floor = token(el, "--surface-2", "#ece8df");
    this.accent = token(el, "--accent", "#0e8a8f");

    // Framed on the building, not the site: a service line from the street or
    // a septic tank outlet would otherwise shrink the house to a stamp. What
    // runs past the frame is clipped at its edge.
    let pts: Pt[] = [...scene.world.wallPieces.flat(), ...scene.rooms.flat(), ...(scene.stairs ?? []).flatMap((st) => st.outline)];
    for (const c of scene.world.colliders) if (c.source === "column") pts.push({ x: c.minX, y: c.minY }, { x: c.maxX, y: c.maxY });
    if (pts.length === 0) pts = scene.pipes.flatMap((p) => p.points);
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const p of pts) {
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x);
      maxY = Math.max(maxY, p.y);
    }
    if (!Number.isFinite(minX)) {
      minX = -5000;
      minY = -5000;
      maxX = 5000;
      maxY = 5000;
    }
    minX -= PAD_MM;
    minY -= PAD_MM;
    maxX += PAD_MM;
    maxY += PAD_MM;
    const scale = Math.min(cw / (maxX - minX), ch / (maxY - minY));
    this.frame = { minX, minY, scale, ox: (cw - (maxX - minX) * scale) / 2, oy: (ch - (maxY - minY) * scale) / 2, h: ch };

    ctx.clearRect(0, 0, cw, ch);
    const poly = (p: Pt[]) => {
      ctx.beginPath();
      p.forEach((q, i) => {
        const [x, y] = this.toCanvas(q);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.closePath();
    };

    ctx.fillStyle = floor;
    for (const r of scene.rooms) {
      poly(r);
      ctx.fill();
    }

    ctx.fillStyle = ink;
    for (const piece of scene.world.wallPieces) {
      poly(piece);
      ctx.fill();
    }
    for (const c of scene.world.colliders) {
      if (c.source !== "column") continue;
      if (c.kind === "circle") {
        const [x, y] = this.toCanvas(c.c);
        ctx.beginPath();
        ctx.arc(x, y, Math.max(c.r * scale, dpr), 0, Math.PI * 2);
        ctx.fill();
      } else {
        poly(c.pts);
        ctx.fill();
      }
    }
    ctx.fillStyle = ink3;
    ctx.globalAlpha = 0.35;
    for (const c of scene.world.colliders) {
      if (c.source !== "object" || c.kind !== "poly") continue;
      poly(c.pts);
      ctx.fill();
    }
    ctx.globalAlpha = 1;

    // Stairs: the flight with its treads and an arrow up it (down on the level at its top).
    const line = (a: Pt, b: Pt) => {
      const [ax, ay] = this.toCanvas(a);
      const [bx, by] = this.toCanvas(b);
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
      ctx.stroke();
    };
    for (const st of scene.stairs ?? []) {
      ctx.strokeStyle = ink3;
      ctx.lineWidth = 1 * dpr;
      poly(st.outline);
      ctx.stroke();
      ctx.globalAlpha = 0.55;
      for (const [a, b] of st.treads) line(a, b);
      ctx.globalAlpha = 1;
      const [from, to] = st.up ? st.arrow : [st.arrow[1], st.arrow[0]];
      ctx.strokeStyle = ink;
      ctx.lineWidth = 1.3 * dpr;
      line(from, to);
      const [fx, fy] = this.toCanvas(from);
      const [tx, ty] = this.toCanvas(to);
      const len = Math.hypot(tx - fx, ty - fy);
      if (len > 1) {
        const ux = (tx - fx) / len;
        const uy = (ty - fy) / len;
        const head = Math.min(5 * dpr, len * 0.4);
        ctx.beginPath();
        ctx.moveTo(tx - ux * head - uy * head * 0.6, ty - uy * head + ux * head * 0.6);
        ctx.lineTo(tx, ty);
        ctx.lineTo(tx - ux * head + uy * head * 0.6, ty - uy * head - ux * head * 0.6);
        ctx.stroke();
      }
    }

    const span = (s: WallSpan, color: string, width: number) => {
      const [ax, ay] = this.toCanvas(s.a);
      const [bx, by] = this.toCanvas(s.b);
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
      ctx.stroke();
    };
    for (const s of scene.world.windows) span(s, "#9cc6dc", 1.6 * dpr);
    // A door is a gap in the wall; a thin line marks where the leaf would be.
    ctx.setLineDash([2 * dpr, 2 * dpr]);
    for (const s of scene.world.doorGaps) span(s, ink3, 1 * dpr);
    ctx.setLineDash([]);

    // Pipes last: a run in a wall chase still reads.
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    for (const pipe of scene.pipes) {
      if (pipe.points.length < 2) continue;
      ctx.strokeStyle = token(el, PIPE_COLOR_VAR[pipe.system], PIPE_COLOR_HEX[pipe.system]);
      ctx.lineWidth = Math.max(1.2 * dpr, (pipe.diameterMm >= 75 ? 2.4 : 1.6) * dpr);
      ctx.beginPath();
      pipe.points.forEach((q, i) => {
        const [x, y] = this.toCanvas(q);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.stroke();
    }
  }

  private drawWalker(ctx: CanvasRenderingContext2D, w: MinimapWalker, dpr: number): void {
    if (w.target) {
      const [gx, gy] = this.toCanvas(w.target);
      ctx.save();
      ctx.strokeStyle = this.accent;
      ctx.lineWidth = 1.6 * dpr;
      ctx.beginPath();
      ctx.arc(gx, gy, 5 * dpr, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }
    // Outside the frame the dot waits at its edge, still pointing the right way.
    const edge = 6 * dpr;
    const [rx, ry] = this.toCanvas(w);
    const x = Math.min(Math.max(rx, edge), ctx.canvas.width - edge);
    const y = Math.min(Math.max(ry, edge), ctx.canvas.height - edge);
    // Canvas y runs down, plan y runs up: the heading flips.
    const fx = Math.cos(w.yaw);
    const fy = -Math.sin(w.yaw);
    const len = 26 * dpr;
    const half = 0.55;
    ctx.save();
    ctx.fillStyle = this.accent;
    ctx.globalAlpha = 0.22;
    ctx.beginPath();
    ctx.moveTo(x, y);
    for (const a of [-half, half]) {
      const c = Math.cos(a);
      const s = Math.sin(a);
      ctx.lineTo(x + (fx * c - fy * s) * len, y + (fx * s + fy * c) * len);
    }
    ctx.closePath();
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.beginPath();
    ctx.arc(x, y, 4 * dpr, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = 1.5 * dpr;
    ctx.stroke();
    ctx.restore();
  }
}
