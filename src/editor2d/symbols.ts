// Plan symbols for the asset catalog (crates/guhit-model/src/defaults.rs).
// Every symbol draws in local millimeters: origin at the footprint center,
// +x right, +y toward the BACK of the object (bed head, sofa back, WC tank).
// The caller sets the transform, stroke and fill styles. `px` is the size of
// one screen pixel in mm, used for line widths and minimum detail sizes.
//
// Devices and fixtures (docs/CONTRACT.md, "Devices, fixtures and links") are
// a list of primitives in the same frame (`deviceSymbol`), so drawing, pick
// bounds, link anchors and the tests all read one geometry. D is the symbol
// size, 3 mm on paper at the plan scale (`symbolSizeMm`: 300 mm at 1:100,
// 150 at 1:50), never scaled with the object. Wall-mounted objects have their
// back, y = d/2, on the wall face ("W" below); everything they draw stays on
// the room side, -y. Text is upright on screen and filled in the stroke
// color. The sheet export (crates/guhit-export) draws the same shapes, with
// two paper differences written up in docs/INTEROP.md: its text is larger so
// it reads when printed, and its label gaps are measured from the outside of
// the 0.2 mm pen. The shapes:
//
// | Key | Shapes (local mm) |
// |---|---|
// | light-ceiling, light-pendant | circle r 0.5D at 0,0 with an X of its two 45 degree diameters; pendant: "P" 0.28D at (0.58D, -0.44D) |
// | light-downlight | circle r 0.3D at 0,0; filled dot r 0.07D at 0,0 |
// | light-tube | own w x d rectangle; a line along its long axis through the center |
// | light-wall, light-outdoor | half disc r 0.5D, flat side on W, bulging to the room; a line from the middle of W to the top of the arc; outdoor: "WP" under it |
// | light-floor-lamp, light-table-lamp | circle r 0.3D with an X, line weight 0.55 |
// | outlet-duplex, outlet-counter | circle r 0.25D touching W (center W - 0.25D); two lines parallel to W at the center +-0.3r, from x -1.36r to 1.36r |
// | outlet-outdoor | the duplex with "WP" under it |
// | outlet-spo, outlet-aircon | the duplex with its room half filled, "SPO" or "ACO" under it |
// | switch-1..3 | "S" 0.36D tall ("S3" when it shares a light) with 1 to 3 dots under it (r 0.12, pitch 0.42 of the text height), one label 0.08D off W |
// | panelboard | own rectangle; the triangle (-w/2,-d/2) (w/2,-d/2) (w/2,d/2) filled; "PB" under it |
// | smoke-detector | circle r 0.3D, "SD" 0.22D inside |
// | doorbell-button | circle r 0.15D touching W, dot r 0.05D, "PB" under it |
// | doorbell-chime | square 0.5D with its back on W, "CH" 0.2D inside |
// | aircon-indoor-* | own rectangle; arrow from the front edge 0.6D into the room, head 0.12D x 0.16D; "ACU" inside |
// | aircon-outdoor-* | own rectangle; fan circle r min(0.42d, 0.26w) at (-0.16w, 0); "CU" at (0.3w, 0) |
// | aircon-window | own rectangle across the wall, "AC" inside it against its room side edge (-y), 0.04D in |
// | floor-drain | own square, circle r 0.36s, three grate chords; "FD" under it |
// | water-heater | own rectangle, circle r 0.32s; "WH" under it |
// | water-meter | own rectangle, dial circle r 0.36s; "WM" under it |
// | water-tank | circle r s/2, rim r 0.86, lid r 0.3, "WT" inside |
// | septic-tank | dashed: own rectangle, baffle at 2/3 of the length, two access circles r 0.14s at (-w/6, 0.18d) and (w/3, 0.18d), "ST" |
// | lpg-cylinder | circle r s/2, collar r 0.55, valve dot r 0.14; "LPG" under it |
// | electric-meter | own rectangle, dial circle r 0.36s; "kWh" under it |
//
// s is the smaller of w and d. A tag "under" a symbol is 0.24D tall (0.26D
// for "PB" on the panelboard, 0.22D for "FD") and sits past the symbol's
// room-side edge, 0.07D clear of it. On screen tags stay upright, so their
// offset allows for the text's width when the object is turned.

import type { Asset, Element } from "../contract/bindings";

type Ctx = CanvasRenderingContext2D;

function rr(ctx: Ctx, x: number, y: number, w: number, h: number, r: number): void {
  const k = Math.max(0, Math.min(r, Math.abs(w) / 2, Math.abs(h) / 2));
  ctx.beginPath();
  ctx.moveTo(x + k, y);
  ctx.lineTo(x + w - k, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + k);
  ctx.lineTo(x + w, y + h - k);
  ctx.quadraticCurveTo(x + w, y + h, x + w - k, y + h);
  ctx.lineTo(x + k, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - k);
  ctx.lineTo(x, y + k);
  ctx.quadraticCurveTo(x, y, x + k, y);
  ctx.closePath();
}

function box(ctx: Ctx, x: number, y: number, w: number, h: number, r = 0, fill = true): void {
  rr(ctx, x, y, w, h, r);
  if (fill) ctx.fill();
  ctx.stroke();
}

function line(ctx: Ctx, x1: number, y1: number, x2: number, y2: number): void {
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x2, y2);
  ctx.stroke();
}

function ellipse(ctx: Ctx, cx: number, cy: number, rx: number, ry: number, fill = false): void {
  ctx.beginPath();
  ctx.ellipse(cx, cy, Math.abs(rx), Math.abs(ry), 0, 0, Math.PI * 2);
  if (fill) ctx.fill();
  ctx.stroke();
}

function bed(ctx: Ctx, w: number, d: number, pillows: number): void {
  const x = -w / 2;
  const y = -d / 2;
  box(ctx, x, y, w, d, 40);
  // headboard strip at the back
  line(ctx, x, d / 2 - 60, x + w, d / 2 - 60);
  const gap = 70;
  const pw = (w - gap * (pillows + 1)) / pillows;
  const ph = Math.min(380, d * 0.2);
  for (let i = 0; i < pillows; i++) {
    box(ctx, x + gap + i * (pw + gap), d / 2 - 110 - ph, pw, ph, 70);
  }
  // blanket edge and folded corner
  const by = d / 2 - 110 - ph - 140;
  line(ctx, x, by, x + w, by);
  ctx.beginPath();
  ctx.moveTo(x + w, by - 380);
  ctx.lineTo(x + w - 380, by);
  ctx.stroke();
}

function sofa(ctx: Ctx, w: number, d: number, seats: number): void {
  const x = -w / 2;
  const y = -d / 2;
  const arm = Math.min(170, w * 0.14);
  const back = Math.min(220, d * 0.28);
  box(ctx, x, y, w, d, 60);
  // back rest
  box(ctx, x, d / 2 - back, w, back, 60, false);
  // arms
  box(ctx, x, y + 40, arm, d - back - 40, 50, false);
  box(ctx, w / 2 - arm, y + 40, arm, d - back - 40, 50, false);
  const sw = (w - arm * 2) / seats;
  for (let i = 1; i < seats; i++) line(ctx, x + arm + sw * i, y + 30, x + arm + sw * i, d / 2 - back);
}

function chair(ctx: Ctx, cx: number, cy: number, angle: number, size: number): void {
  ctx.save();
  ctx.translate(cx, cy);
  ctx.rotate(angle);
  box(ctx, -size / 2, -size / 2, size, size, size * 0.18);
  // back rest on local +y
  line(ctx, -size / 2 + size * 0.1, size / 2 - size * 0.18, size / 2 - size * 0.1, size / 2 - size * 0.18);
  ctx.restore();
}

function dining(ctx: Ctx, w: number, d: number, seats: number): void {
  const cs = 420;
  const out = 230; // how far a tucked chair sticks out of the table edge
  const perSide = 2; // six seats adds one chair at each end
  const step = w / perSide;
  for (let i = 0; i < perSide; i++) {
    const cx = -w / 2 + step * (i + 0.5);
    chair(ctx, cx, d / 2 + out - cs / 2, 0, cs);
    chair(ctx, cx, -d / 2 - out + cs / 2, Math.PI, cs);
  }
  if (seats === 6) {
    chair(ctx, w / 2 + out - cs / 2, 0, -Math.PI / 2, cs);
    chair(ctx, -w / 2 - out + cs / 2, 0, Math.PI / 2, cs);
  }
  box(ctx, -w / 2, -d / 2, w, d, 30);
}

function tree(ctx: Ctx, r: number, px: number): void {
  const lobes = 11;
  ctx.beginPath();
  for (let i = 0; i <= lobes * 8; i++) {
    const a = (i / (lobes * 8)) * Math.PI * 2;
    const k = r * (0.9 + 0.1 * Math.cos(a * lobes));
    const x = Math.cos(a) * k;
    const y = Math.sin(a) * k;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  for (let i = 0; i < 7; i++) {
    const a = (i / 7) * Math.PI * 2 + 0.3;
    line(ctx, Math.cos(a) * r * 0.12, Math.sin(a) * r * 0.12, Math.cos(a) * r * 0.62, Math.sin(a) * r * 0.62);
  }
  ellipse(ctx, 0, 0, Math.max(r * 0.07, px * 1.5), Math.max(r * 0.07, px * 1.5), true);
}

function car(ctx: Ctx, w: number, d: number): void {
  const x = -w / 2;
  const y = -d / 2;
  box(ctx, x, y, w, d, w * 0.32);
  // front is local -y
  const hood = d * 0.24;
  const trunk = d * 0.17;
  const glass = d * 0.12;
  const inset = w * 0.12;
  // cabin
  box(ctx, x + inset, y + hood, w - inset * 2, d - hood - trunk, w * 0.12, false);
  // windshield and rear glass
  ctx.beginPath();
  ctx.moveTo(x + inset, y + hood);
  ctx.lineTo(x + inset * 1.7, y + hood + glass);
  ctx.lineTo(w / 2 - inset * 1.7, y + hood + glass);
  ctx.lineTo(w / 2 - inset, y + hood);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(x + inset, d / 2 - trunk);
  ctx.lineTo(x + inset * 1.7, d / 2 - trunk - glass * 0.8);
  ctx.lineTo(w / 2 - inset * 1.7, d / 2 - trunk - glass * 0.8);
  ctx.lineTo(w / 2 - inset, d / 2 - trunk);
  ctx.stroke();
  // mirrors
  box(ctx, x - 90, y + hood + 60, 90, 160, 30);
  box(ctx, w / 2, y + hood + 60, 90, 160, 30);
}

// ------------------------------------------------------------ devices and fixtures

/** Paper size of a device symbol, mm. D is this times the scale denominator. */
export const SYMBOL_PAPER_MM = 3;

/** The UI font stack (tokens.css `--font-ui`), for tags when the caller gives none. */
const SYMBOL_FONT = '"Inter", system-ui, -apple-system, "Segoe UI", Roboto, sans-serif';

/** D, the device symbol size in model mm at plan scale 1:n. 300 at 1:100, 150 at 1:50. */
export function symbolSizeMm(scaleDenominator: number): number {
  const n = Number.isFinite(scaleDenominator) && scaleDenominator > 0 ? scaleDenominator : 100;
  return SYMBOL_PAPER_MM * n;
}

export interface SymbolOptions {
  /** D in model mm, from `symbolSizeMm(project.settings.scale_denominator)`. Default 300, the 1:100 size. */
  symbolMm?: number;
  /**
   * The switch shares a light with another switch: it reads "S3". Used as
   * given; `threeWaySwitches` works it out for a caller that has not.
   */
  threeWay?: boolean;
  /** Font family for the tags. Default: the UI font stack. */
  font?: string;
}

type Fill = "none" | "body" | "ink";

interface Pt {
  x: number;
  y: number;
}

/**
 * One shape of a device symbol. `body` fills with the caller's fill style
 * (white, or clear for a hollow ghost), `ink` with the stroke color, so the
 * selection tint and the AI preview color reach every part.
 */
export type SymbolPrim =
  | { kind: "circle"; x: number; y: number; r: number; fill: Fill; minPx?: number }
  /** A disc sector from `from` to `to` (radians, local frame), closed by its chord. */
  | { kind: "arc"; x: number; y: number; r: number; from: number; to: number; fill: Fill }
  | { kind: "line"; a: Pt; b: Pt }
  /** A closed polygon. */
  | { kind: "poly"; points: Pt[]; fill: Fill }
  /**
   * Text `h` mm tall, upright on screen, with `dots` filled dots in a row
   * under it (a switch's gangs). Without `away` it is centered on (x, y).
   * With `away`, a local unit direction, (x, y) is an edge point and the
   * whole label sits past it in that direction, `gap` mm clear of it at any
   * rotation: a tag never runs into its symbol.
   */
  | { kind: "text"; x: number; y: number; h: number; text: string; away?: Pt; gap?: number; dots?: number };

export interface DeviceSymbol {
  prims: SymbolPrim[];
  /** Line weight against a normal symbol line: 0.55 for plug-in lamps. */
  weight: number;
  /** Below grade (the septic tank): drawn dashed. */
  dashed: boolean;
  /** Where link arcs attach, local mm, and the radius the symbol keeps clear around it. */
  anchor: { x: number; y: number; r: number };
}

const SWITCH_GANGS: Record<string, number> = { "switch-1": 1, "switch-2": 2, "switch-3": 3 };

const circle = (x: number, y: number, r: number, fill: Fill, minPx?: number): SymbolPrim => ({ kind: "circle", x, y, r, fill, minPx });
const halfDisc = (x: number, y: number, r: number, fill: Fill): SymbolPrim => ({ kind: "arc", x, y, r, from: Math.PI, to: 2 * Math.PI, fill });
const seg = (x1: number, y1: number, x2: number, y2: number): SymbolPrim => ({ kind: "line", a: { x: x1, y: y1 }, b: { x: x2, y: y2 } });
const text = (x: number, y: number, h: number, t: string): SymbolPrim => ({ kind: "text", x, y, h, text: t });
/** A tag past the edge point (x, y), toward the room (local -y), `gap` clear of it. */
const tag = (x: number, y: number, h: number, t: string, gap: number, dots = 0): SymbolPrim => ({ kind: "text", x, y, h, text: t, away: { x: 0, y: -1 }, gap, dots });
const rect = (x0: number, y0: number, x1: number, y1: number, fill: Fill): SymbolPrim => ({
  kind: "poly",
  points: [
    { x: x0, y: y0 },
    { x: x1, y: y0 },
    { x: x1, y: y1 },
    { x: x0, y: y1 },
  ],
  fill,
});
const cross = (r: number): SymbolPrim[] => {
  const k = r * Math.SQRT1_2;
  return [seg(-k, -k, k, k), seg(-k, k, k, -k)];
};

const dim = (v: number): number => (Number.isFinite(v) && Math.abs(v) >= 1 ? Math.abs(v) : 1);

/**
 * The plan symbol of a device or fixture key, in local mm, or null for keys
 * drawn as furniture (and unknown keys). `w` and `d` are the object's own
 * width and depth; symbols sized by D ignore them except for the wall face.
 */
export function deviceSymbol(key: string, w: number, d: number, opts: SymbolOptions = {}): DeviceSymbol | null {
  const D = opts.symbolMm !== undefined && opts.symbolMm > 0 ? opts.symbolMm : symbolSizeMm(100);
  const W = dim(w);
  const De = dim(d);
  const s = Math.min(W, De);
  const yw = De / 2; // the wall face of a wall-mounted object
  const tagH = 0.24 * D;
  const gap = 0.07 * D;
  const own = (fill: Fill): SymbolPrim => rect(-W / 2, -De / 2, W / 2, De / 2, fill);
  const P: SymbolPrim[] = [];
  let anchor = { x: 0, y: 0, r: s / 2 };
  let weight = 1;
  let dashed = false;

  if (key.startsWith("aircon-indoor-")) {
    const tip = -De / 2 - 0.6 * D;
    P.push(own("body"), seg(0, -De / 2, 0, tip), seg(0, tip, -0.12 * D, tip + 0.16 * D), seg(0, tip, 0.12 * D, tip + 0.16 * D));
    P.push(text(0, 0, Math.min(0.3 * D, 0.55 * De), "ACU"));
    return { prims: P, weight, dashed, anchor };
  }
  if (key.startsWith("aircon-outdoor-")) {
    P.push(own("body"), circle(-0.16 * W, 0, Math.min(0.42 * De, 0.26 * W), "none"));
    P.push(text(0.3 * W, 0, Math.min(0.3 * D, 0.5 * De), "CU"));
    return { prims: P, weight, dashed, anchor };
  }
  const gangs = SWITCH_GANGS[key];
  if (gangs) {
    // "S" and its dots are one upright label beside the wall face.
    const h = 0.36 * D;
    P.push(tag(0, yw, h, opts.threeWay ? "S3" : "S", 0.08 * D, gangs));
    return { prims: P, weight, dashed, anchor: { x: 0, y: yw - 0.08 * D - labelHalfHeight(h, gangs), r: 0.25 * D } };
  }

  switch (key) {
    // ---- light fixtures
    case "light-ceiling":
    case "light-pendant": {
      const r = 0.5 * D;
      P.push(circle(0, 0, r, "body"), ...cross(r));
      if (key === "light-pendant") P.push(text(0.58 * D, -0.44 * D, 0.28 * D, "P"));
      anchor = { x: 0, y: 0, r };
      break;
    }
    case "light-downlight": {
      const r = 0.3 * D;
      P.push(circle(0, 0, r, "body"), circle(0, 0, 0.07 * D, "ink", 1.2));
      anchor = { x: 0, y: 0, r };
      break;
    }
    case "light-tube":
      P.push(own("body"), W >= De ? seg(-W / 2, 0, W / 2, 0) : seg(0, -De / 2, 0, De / 2));
      break;
    case "light-wall":
    case "light-outdoor": {
      const r = 0.5 * D;
      P.push(halfDisc(0, yw, r, "body"), seg(0, yw, 0, yw - r));
      if (key === "light-outdoor") P.push(tag(0, yw - r, tagH, "WP", gap));
      anchor = { x: 0, y: yw - r / 2, r: 0.6 * r };
      break;
    }
    case "light-floor-lamp":
    case "light-table-lamp": {
      const r = 0.3 * D;
      P.push(circle(0, 0, r, "body"), ...cross(r));
      weight = 0.55;
      anchor = { x: 0, y: 0, r };
      break;
    }

    // ---- outlets and wall devices
    case "outlet-duplex":
    case "outlet-counter":
    case "outlet-outdoor":
    case "outlet-spo":
    case "outlet-aircon": {
      const r = 0.25 * D;
      const cy = yw - r;
      const special = key === "outlet-spo" || key === "outlet-aircon";
      P.push(circle(0, cy, r, "body"));
      if (special) P.push(halfDisc(0, cy, r, "ink"));
      for (const side of [-1, 1]) P.push(seg(-1.36 * r, cy + side * 0.3 * r, 1.36 * r, cy + side * 0.3 * r));
      const label = key === "outlet-outdoor" ? "WP" : key === "outlet-spo" ? "SPO" : key === "outlet-aircon" ? "ACO" : "";
      if (label) P.push(tag(0, cy - r, tagH, label, gap));
      anchor = { x: 0, y: cy, r };
      break;
    }
    case "panelboard":
      P.push(
        own("body"),
        {
          kind: "poly",
          points: [
            { x: -W / 2, y: -De / 2 },
            { x: W / 2, y: -De / 2 },
            { x: W / 2, y: De / 2 },
          ],
          fill: "ink",
        },
        tag(0, -De / 2, 0.26 * D, "PB", gap),
      );
      anchor = { x: 0, y: 0, r: Math.max(W, De) / 2 };
      break;
    case "smoke-detector": {
      const r = 0.3 * D;
      P.push(circle(0, 0, r, "body"), text(0, 0, 0.22 * D, "SD"));
      anchor = { x: 0, y: 0, r };
      break;
    }
    case "doorbell-button": {
      const r = 0.15 * D;
      const cy = yw - r;
      P.push(circle(0, cy, r, "body"), circle(0, cy, 0.05 * D, "ink", 1.2), tag(0, cy - r, tagH, "PB", gap));
      anchor = { x: 0, y: cy, r };
      break;
    }
    case "doorbell-chime": {
      const q = 0.5 * D;
      P.push(rect(-q / 2, yw - q, q / 2, yw, "body"), text(0, yw - q / 2, 0.2 * D, "CH"));
      anchor = { x: 0, y: yw - q / 2, r: q / 2 };
      break;
    }
    case "aircon-window":
      // The unit crosses the wall, which draws over its middle: the tag sits
      // inside the unit against its room side edge.
      P.push(own("body"), { kind: "text", x: 0, y: -De / 2, h: Math.min(0.3 * D, 0.45 * s), text: "AC", away: { x: 0, y: 1 }, gap: 0.04 * D });
      break;

    // ---- plumbing and utility fixtures, drawn at their own size
    case "floor-drain": {
      const r = 0.36 * s;
      P.push(own("body"), circle(0, 0, r, "none"));
      for (const o of [-0.45, 0, 0.45]) {
        const hx = Math.sqrt(1 - o * o) * r;
        P.push(seg(-hx, o * r, hx, o * r));
      }
      P.push(tag(0, -De / 2, 0.22 * D, "FD", gap));
      break;
    }
    case "water-heater":
      P.push(own("body"), circle(0, 0, 0.32 * s, "none"), tag(0, -De / 2, tagH, "WH", gap));
      break;
    case "water-meter":
      P.push(own("body"), circle(0, 0, 0.36 * s, "none"), tag(0, -De / 2, tagH, "WM", gap));
      break;
    case "water-tank": {
      const r = s / 2;
      P.push(circle(0, 0, r, "body"), circle(0, 0, 0.86 * r, "none"), circle(0, 0, 0.3 * r, "none"));
      P.push(text(0, 0, Math.min(0.3 * D, 0.36 * r), "WT"));
      break;
    }
    case "septic-tank": {
      dashed = true;
      const baffle = -W / 2 + (2 * W) / 3;
      P.push(own("none"), seg(baffle, -De / 2, baffle, De / 2));
      P.push(circle(-W / 6, 0.18 * De, 0.14 * s, "none"), circle(W / 3, 0.18 * De, 0.14 * s, "none"));
      P.push(text(-W / 6, -0.2 * De, Math.min(0.3 * D, 0.3 * De), "ST"));
      break;
    }
    case "lpg-cylinder": {
      const r = s / 2;
      P.push(circle(0, 0, r, "body"), circle(0, 0, 0.55 * r, "none"), circle(0, 0, 0.14 * r, "ink", 1));
      P.push(tag(0, -r, tagH, "LPG", gap));
      break;
    }
    case "electric-meter":
      P.push(own("body"), circle(0, 0, 0.36 * s, "none"), tag(0, -De / 2, tagH, "kWh", gap));
      break;
    default:
      return null;
  }
  return { prims: P, weight, dashed, anchor };
}

/** Local bounds of everything a device symbol draws, text included (estimated). */
export function symbolBounds(sym: DeviceSymbol): { minX: number; minY: number; maxX: number; maxY: number } {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  const add = (x: number, y: number): void => {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  };
  for (const p of sym.prims) {
    switch (p.kind) {
      case "circle":
        add(p.x - p.r, p.y - p.r);
        add(p.x + p.r, p.y + p.r);
        break;
      case "arc":
        add(p.x + Math.cos(p.from) * p.r, p.y + Math.sin(p.from) * p.r);
        add(p.x + Math.cos(p.to) * p.r, p.y + Math.sin(p.to) * p.r);
        for (let i = 1; i < 16; i++) {
          const t = p.from + ((p.to - p.from) * i) / 16;
          add(p.x + Math.cos(t) * p.r, p.y + Math.sin(t) * p.r);
        }
        add(p.x + Math.cos((p.from + p.to) / 2) * p.r, p.y + Math.sin((p.from + p.to) / 2) * p.r);
        break;
      case "line":
        add(p.a.x, p.a.y);
        add(p.b.x, p.b.y);
        break;
      case "poly":
        for (const q of p.points) add(q.x, q.y);
        break;
      case "text": {
        // Upright on screen: a square that holds the label at any rotation.
        const dots = p.dots ?? 0;
        const half = Math.max(labelHalfWidth(p.h, p.text, dots), labelHalfHeight(p.h, dots));
        const along = p.away ? (p.gap ?? 0) + half : 0;
        const cx = p.x + (p.away?.x ?? 0) * along;
        const cy = p.y + (p.away?.y ?? 0) * along;
        add(cx - half, cy - half);
        add(cx + half, cy + half);
        break;
      }
    }
  }
  if (!Number.isFinite(minX)) return { minX: 0, minY: 0, maxX: 0, maxY: 0 };
  return { minX, minY, maxX, maxY };
}

/** Label layout in text heights: a switch's dots under its "S". */
const DOT_R = 0.12;
const DOT_PITCH = 0.42;
const DOT_GAP = 0.24;

/** Half the height of a label `h` tall with `dots` under it. */
function labelHalfHeight(h: number, dots: number): number {
  return (h + (dots > 0 ? (DOT_GAP + 2 * DOT_R) * h : 0)) / 2;
}

/** Half the width of a label, text width estimated. */
function labelHalfWidth(h: number, t: string, dots: number): number {
  const text = t.length > 0 ? (0.62 * t.length + 0.1) * h : 0;
  const row = dots > 0 ? ((dots - 1) * DOT_PITCH + 2 * DOT_R) * h : 0;
  return Math.max(text, row) / 2;
}

/**
 * A text prim, upright on screen whatever the object's rotation and the
 * plan's flipped y. A tag with `away` is pushed past its edge by the extent
 * of its box in the direction it goes on screen, so turning the object never
 * slides a wide tag into its symbol. Skipped below 3 pixels.
 */
function drawLabel(ctx: Ctx, p: Extract<SymbolPrim, { kind: "text" }>, px: number, font: string, ink: Ctx["strokeStyle"]): void {
  const size = p.h / px;
  const dots = Math.max(0, Math.round(p.dots ?? 0));
  if (!(size >= 3) || (p.text === "" && dots === 0)) return;
  ctx.save();
  ctx.translate(p.x, p.y);
  ctx.font = `600 ${size}px ${font}`;
  const textW = p.text ? ctx.measureText(p.text).width : 0;
  const hw = Math.max(textW, dots > 0 ? ((dots - 1) * DOT_PITCH + 2 * DOT_R) * size : 0) / 2;
  const hh = labelHalfHeight(size, dots);
  const gapPx = (p.gap ?? 0) / px;
  // Center of the label's box from (x, y), CSS pixels, screen axes (y down).
  let ox = 0;
  let oy = 0;
  if (typeof ctx.getTransform === "function") {
    const m = ctx.getTransform();
    // Device pixels per CSS pixel: the device pixel ratio times any scale
    // the caller applied on top (the settle flash).
    const k = Math.sqrt(Math.abs(m.a * m.d - m.b * m.c)) * px;
    if (!(k > 0)) {
      ctx.restore();
      return;
    }
    if (p.away) {
      const dx = m.a * p.away.x + m.c * p.away.y;
      const dy = m.b * p.away.x + m.d * p.away.y;
      const len = Math.hypot(dx, dy);
      if (len > 0) {
        const dist = gapPx + (Math.abs(dx) / len) * hw + (Math.abs(dy) / len) * hh;
        ox = (dx / len) * dist;
        oy = (dy / len) * dist;
      }
    }
    ctx.setTransform(k, 0, 0, k, m.e, m.f);
  } else {
    ctx.scale(px, -px);
    if (p.away) {
      ox = p.away.x * (gapPx + hh);
      oy = -p.away.y * (gapPx + hh);
    }
  }
  // The text sits at the top of the box, the dots under it.
  const textY = oy - hh + size / 2;
  ctx.fillStyle = ink;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  if (p.text) ctx.fillText(p.text, ox, textY);
  if (dots > 0) {
    const r = Math.max(DOT_R * size, 1.2);
    const y = textY + size / 2 + (DOT_GAP + DOT_R) * size;
    for (let i = 0; i < dots; i++) {
      ctx.beginPath();
      ctx.arc(ox + (i - (dots - 1) / 2) * DOT_PITCH * size, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  ctx.restore();
}

function drawDevice(ctx: Ctx, sym: DeviceSymbol, px: number, font: string): void {
  ctx.save();
  if (sym.weight !== 1) ctx.lineWidth *= sym.weight;
  if (sym.dashed) ctx.setLineDash([6 * px, 4 * px]);
  const ink = ctx.strokeStyle;
  const body = ctx.fillStyle;
  const paint = (fill: Fill): void => {
    if (fill === "none") return;
    ctx.fillStyle = fill === "ink" ? ink : body;
    ctx.fill();
  };
  for (const p of sym.prims) {
    switch (p.kind) {
      case "circle":
        ctx.beginPath();
        ctx.arc(p.x, p.y, Math.max(p.r, (p.minPx ?? 0) * px), 0, Math.PI * 2);
        paint(p.fill);
        ctx.stroke();
        break;
      case "arc":
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, p.from, p.to);
        ctx.closePath();
        paint(p.fill);
        ctx.stroke();
        break;
      case "line":
        ctx.beginPath();
        ctx.moveTo(p.a.x, p.a.y);
        ctx.lineTo(p.b.x, p.b.y);
        ctx.stroke();
        break;
      case "poly":
        ctx.beginPath();
        p.points.forEach((q, i) => (i === 0 ? ctx.moveTo(q.x, q.y) : ctx.lineTo(q.x, q.y)));
        ctx.closePath();
        paint(p.fill);
        ctx.stroke();
        break;
      case "text":
        drawLabel(ctx, p, px, font, ink);
        break;
    }
  }
  ctx.restore();
}

type Placed = Pick<Asset, "catalog_key" | "position" | "rotation_deg" | "width_mm" | "depth_mm">;

/** Local mm to plan mm for a placed asset (the transform `drawAsset` uses). */
function toPlan(a: Placed, p: Pt): Pt {
  const r = (a.rotation_deg * Math.PI) / 180;
  const c = Math.cos(r);
  const s = Math.sin(r);
  return { x: a.position.x + p.x * c - p.y * s, y: a.position.y + p.x * s + p.y * c };
}

/**
 * Where a link arc meets this asset's symbol, in plan mm, with the radius the
 * symbol keeps clear (end the arc there). For a switch it is the "S", for a
 * light the middle of its symbol. Objects without a device symbol answer
 * their center.
 */
export function assetSymbolAnchor(a: Placed, opts: SymbolOptions = {}): { x: number; y: number; r: number } {
  const sym = deviceSymbol(a.catalog_key, a.width_mm, a.depth_mm, opts);
  const local = sym ? sym.anchor : { x: 0, y: 0, r: Math.min(dim(a.width_mm), dim(a.depth_mm)) / 2 };
  const p = toPlan(a, local);
  return { x: p.x, y: p.y, r: local.r };
}

/**
 * Plan outline (four corners, counter-clockwise) of everything a device
 * symbol draws plus the object's own footprint, for picking and highlights:
 * a 70 mm switch is drawn much larger than its footprint. Null for objects
 * drawn at their footprint (furniture and unknown keys).
 */
export function assetSymbolOutline(a: Placed, opts: SymbolOptions = {}): Pt[] | null {
  const sym = deviceSymbol(a.catalog_key, a.width_mm, a.depth_mm, opts);
  if (!sym) return null;
  const b = symbolBounds(sym);
  const hw = dim(a.width_mm) / 2;
  const hd = dim(a.depth_mm) / 2;
  const x0 = Math.min(b.minX, -hw);
  const y0 = Math.min(b.minY, -hd);
  const x1 = Math.max(b.maxX, hw);
  const y1 = Math.max(b.maxY, hd);
  return [
    { x: x0, y: y0 },
    { x: x1, y: y0 },
    { x: x1, y: y1 },
    { x: x0, y: y1 },
  ].map((p) => toPlan(a, p));
}

/**
 * Switches that share a light with another switch (a 3-way pair, or more):
 * their symbol reads "S3". The plan works this out itself (editor2d/links.ts)
 * and passes it in `SymbolOptions.threeWay`, which the drawing always uses;
 * this is the fallback for a caller that has not. Same rule: only links to
 * lights that still exist count, so a link left pointing at a deleted light
 * never makes a 3-way.
 */
export function threeWaySwitches(elements: readonly Element[]): Set<string> {
  const lights = new Set<string>();
  const switches: Asset[] = [];
  for (const e of elements) {
    if (e.kind !== "asset") continue;
    if (e.catalog_key.startsWith("switch-")) switches.push(e);
    else if (e.category === "lighting" || e.catalog_key.startsWith("light-")) lights.add(e.id);
  }
  const count = new Map<string, number>();
  for (const sw of switches) {
    for (const id of new Set(sw.links ?? [])) if (lights.has(id)) count.set(id, (count.get(id) ?? 0) + 1);
  }
  const out = new Set<string>();
  for (const sw of switches) if ((sw.links ?? []).some((id) => (count.get(id) ?? 0) >= 2)) out.add(sw.id);
  return out;
}

/**
 * Draws the symbol for `key`. Returns a short label to print on top (may be
 * empty), or null when the key is unknown and the caller should draw the
 * labelled box fallback. Devices and fixtures draw their own tags and need
 * `opts.symbolMm` for the plan scale (300 at 1:100 when left out).
 */
export function drawAssetSymbol(ctx: Ctx, key: string, w: number, d: number, px: number, opts: SymbolOptions = {}): string | null {
  const device = deviceSymbol(key, w, d, opts);
  if (device) {
    drawDevice(ctx, device, px, opts.font ?? SYMBOL_FONT);
    return "";
  }
  const x = -w / 2;
  const y = -d / 2;
  switch (key) {
    case "bed-single":
      bed(ctx, w, d, 1);
      return "";
    case "bed-double":
    case "bed-queen":
      bed(ctx, w, d, 2);
      return "";
    case "wardrobe": {
      box(ctx, x, y, w, d);
      line(ctx, x + 40, 0, x + w - 40, 0);
      const n = Math.max(3, Math.round(w / 170));
      for (let i = 1; i < n; i++) {
        const hx = x + (w / n) * i;
        line(ctx, hx - 40, -d * 0.32, hx + 40, d * 0.32);
      }
      return "";
    }
    case "sofa-3":
      sofa(ctx, w, d, 3);
      return "";
    case "sofa-2":
      sofa(ctx, w, d, 2);
      return "";
    case "armchair":
      sofa(ctx, w, d, 1);
      return "";
    case "coffee-table":
      box(ctx, x, y, w, d, 50);
      box(ctx, x + 70, y + 70, w - 140, d - 140, 30, false);
      return "";
    case "tv-console":
      box(ctx, x, y, w, d);
      // screen, against the back
      box(ctx, -w * 0.36, d / 2 - 140, w * 0.72, 60, 0, false);
      line(ctx, -w * 0.12, d / 2 - 140, -w * 0.12, d / 2 - 250);
      line(ctx, w * 0.12, d / 2 - 140, w * 0.12, d / 2 - 250);
      line(ctx, -w * 0.12, d / 2 - 250, w * 0.12, d / 2 - 250);
      return "";
    case "dining-4":
      dining(ctx, w, d, 4);
      return "";
    case "dining-6":
      dining(ctx, w, d, 6);
      return "";
    case "desk":
      chair(ctx, 0, -d / 2 - 120, Math.PI, 440);
      box(ctx, x, y, w, d);
      return "";
    case "wc": {
      const tank = d * 0.27;
      box(ctx, x, d / 2 - tank, w, tank, 30);
      const bowlH = d - tank - 10;
      ctx.beginPath();
      ctx.ellipse(0, d / 2 - tank - bowlH / 2, w * 0.46, bowlH / 2, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ellipse(ctx, 0, d / 2 - tank - bowlH * 0.52, w * 0.27, bowlH * 0.3);
      return "";
    }
    case "lavatory":
      box(ctx, x, y, w, d, 60);
      ellipse(ctx, 0, -d * 0.06, w * 0.36, d * 0.3);
      ellipse(ctx, 0, d * 0.36, 18, 18, false);
      return "";
    case "shower":
      box(ctx, x, y, w, d);
      line(ctx, x, y, x + w, y + d);
      line(ctx, x, y + d, x + w, y);
      ellipse(ctx, 0, 0, 55, 55, true);
      ellipse(ctx, x + w - 130, y + d - 130, 70, 70);
      return "";
    case "bathtub":
      box(ctx, x, y, w, d, 40);
      box(ctx, x + 70, y + 70, w - 140, d - 140, (d - 140) / 2, false);
      ellipse(ctx, x + w - 230, 0, 28, 28);
      return "";
    case "kitchen-counter":
      box(ctx, x, y, w, d);
      line(ctx, x, y + 40, x + w, y + 40);
      return "";
    case "kitchen-sink": {
      box(ctx, x, y, w, d);
      line(ctx, x, y + 40, x + w, y + 40);
      const bw = Math.min(w * 0.34, 430);
      box(ctx, -bw - 25, -d * 0.24, bw, d * 0.56, 50, false);
      box(ctx, 25, -d * 0.24, bw, d * 0.56, 50, false);
      ellipse(ctx, 0, d * 0.4, 22, 22);
      line(ctx, 0, d * 0.4, 0, d * 0.16);
      return "";
    }
    case "range": {
      box(ctx, x, y, w, d);
      const r = Math.min(w, d) * 0.15;
      for (const sx of [-1, 1]) {
        for (const sy of [-1, 1]) {
          ellipse(ctx, sx * w * 0.23, sy * d * 0.2 + d * 0.05, r, r);
          ellipse(ctx, sx * w * 0.23, sy * d * 0.2 + d * 0.05, r * 0.35, r * 0.35);
        }
      }
      line(ctx, x, y + d * 0.1, x + w, y + d * 0.1);
      return "";
    }
    case "refrigerator":
      box(ctx, x, y, w, d, 20);
      line(ctx, x, y + 60, x + w, y + 60);
      line(ctx, x + w - 90, y + 60, x + w - 90, y + 10);
      return "REF";
    case "washing-machine": {
      box(ctx, x, y, w, d, 30);
      line(ctx, x, d / 2 - d * 0.18, x + w, d / 2 - d * 0.18);
      const r = Math.min(w, d * 0.8) * 0.34;
      ellipse(ctx, 0, -d * 0.08, r, r);
      ellipse(ctx, 0, -d * 0.08, r * 0.7, r * 0.7);
      return "";
    }
    case "plant-pot": {
      const r = Math.min(w, d) / 2;
      ellipse(ctx, 0, 0, r * 0.45, r * 0.45, true);
      for (let i = 0; i < 8; i++) {
        const a = (i / 8) * Math.PI * 2;
        ctx.beginPath();
        ctx.ellipse(Math.cos(a) * r * 0.58, Math.sin(a) * r * 0.58, r * 0.42, r * 0.16, a, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
      return "";
    }
    case "tree":
      tree(ctx, Math.min(w, d) / 2, px);
      return "";
    case "car-sedan":
      car(ctx, w, d);
      return "";
    default:
      return null;
  }
}
