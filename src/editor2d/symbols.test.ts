import { describe, expect, it } from "vitest";
import type { Asset, BuiltInParams, Element } from "../contract/bindings";
import catalogSource from "../../crates/guhit-model/src/catalog.rs?raw";
import {
  assetSymbolAnchor,
  assetSymbolOutline,
  deviceSymbol,
  drawAssetSymbol,
  symbolBounds,
  symbolSizeMm,
  threeWaySwitches,
  type DeviceSymbol,
  type SymbolPrim,
} from "./symbols";

/** Every catalog item, read from the Rust source so a new key needs a symbol here too. */
const CATALOG = [...catalogSource.matchAll(/item\("([a-z0-9-]+)",\s*"[^"]*",\s*A::\w+,\s*([\d.]+),\s*([\d.]+),/g)].map((m) => ({
  key: m[1],
  w: Number(m[2]),
  d: Number(m[3]),
}));

const KEYS = CATALOG.map((c) => c.key);

/** Keys added with the devices, fixtures and aircon (they must not be drawn as boxes). */
const DEVICE_KEYS = KEYS.slice(KEYS.indexOf("floor-drain"), KEYS.indexOf("aircon-window") + 1);

/** Keys of catalog v2: everything after the last aircon, furniture to built-ins and the site kit. */
const V2_KEYS = KEYS.slice(KEYS.indexOf("aircon-window") + 1);

/** Records what a symbol draws. Enough of CanvasRenderingContext2D for symbols.ts. */
function recorder() {
  const calls: { op: string; args: unknown[] }[] = [];
  const texts: string[] = [];
  let dash: number[] = [];
  const rec =
    (op: string) =>
    (...args: unknown[]) => {
      calls.push({ op, args });
    };
  const ctx = {
    lineWidth: 1,
    strokeStyle: "#445566",
    fillStyle: "rgba(255,255,255,0.78)",
    font: "",
    textAlign: "start",
    textBaseline: "alphabetic",
    lineJoin: "miter",
    save: rec("save"),
    restore: rec("restore"),
    beginPath: rec("beginPath"),
    closePath: rec("closePath"),
    moveTo: rec("moveTo"),
    lineTo: rec("lineTo"),
    arc: rec("arc"),
    ellipse: rec("ellipse"),
    quadraticCurveTo: rec("quadraticCurveTo"),
    rect: rec("rect"),
    fill: rec("fill"),
    stroke: rec("stroke"),
    translate: rec("translate"),
    rotate: rec("rotate"),
    scale: rec("scale"),
    setTransform: rec("setTransform"),
    getTransform: () => ({ a: 0.2, b: 0, c: 0, d: -0.2, e: 100, f: 100 }),
    setLineDash: (d: number[]) => {
      dash = d;
      calls.push({ op: "setLineDash", args: [d] });
    },
    fillText: (t: string) => {
      texts.push(t);
      calls.push({ op: "fillText", args: [t] });
    },
    measureText: (t: string) => ({ width: t.length * 6 }),
  };
  return { ctx: ctx as unknown as CanvasRenderingContext2D, calls, texts, dash: () => dash };
}


interface P2 {
  x: number;
  y: number;
}

/**
 * A context that follows the transform and keeps what a symbol draws, in the
 * frame it was drawn in: every point on a path (arcs and curves sampled), line
 * segments with the dash they were stroked with, arcs, ellipses and curves as
 * given, the fill colors, and the translate and rotate calls (a chair).
 */
function tracer() {
  type M = [number, number, number, number, number, number];
  let m: M = [1, 0, 0, 1, 0, 0];
  let dash: number[] = [];
  const stack: { m: M; dash: number[] }[] = [];
  const pts: P2[] = [];
  const top: P2[] = []; // points drawn outside any save/restore
  const segs: { a: P2; b: P2; dash: number[] }[] = [];
  const arcs: { x: number; y: number; r: number; a0: number; a1: number; dash: number[] }[] = [];
  const ellipses: { x: number; y: number; rx: number; ry: number; rot: number }[] = [];
  const curves: { from: P2; c: P2; to: P2 }[] = [];
  const fills: unknown[] = [];
  const moves: { op: string; args: number[] }[] = [];
  let path: { a: P2; b: P2 }[] = [];
  let cur: P2 | null = null;
  let closed = 0;
  const TAU = Math.PI * 2;
  const map = (x: number, y: number): P2 => ({ x: m[0] * x + m[2] * y + m[4], y: m[1] * x + m[3] * y + m[5] });
  const mul = (n: M): void => {
    m = [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3], m[1] * n[2] + m[3] * n[3], m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5]];
  };
  const add = (x: number, y: number): P2 => {
    const p = map(x, y);
    pts.push(p);
    if (stack.length === 0) top.push(p);
    return p;
  };
  const sweep = (a0: number, a1: number, acw: boolean): number => {
    if (!acw) return a1 - a0 >= TAU ? TAU : (((a1 - a0) % TAU) + TAU) % TAU;
    return a0 - a1 >= TAU ? -TAU : -((((a0 - a1) % TAU) + TAU) % TAU);
  };
  const sample = (cx: number, cy: number, rx: number, ry: number, rot: number, a0: number, a1: number, acw: boolean): void => {
    const sw = sweep(a0, a1, acw);
    for (let i = 0; i <= 64; i++) {
      const t = a0 + (sw * i) / 64;
      cur = add(cx + rx * Math.cos(t) * Math.cos(rot) - ry * Math.sin(t) * Math.sin(rot), cy + rx * Math.cos(t) * Math.sin(rot) + ry * Math.sin(t) * Math.cos(rot));
    }
  };
  let local: P2 | null = null;
  const ctx = {
    lineWidth: 1,
    strokeStyle: "#445566",
    fillStyle: "rgba(255,255,255,0.78)",
    lineJoin: "miter",
    save: () => stack.push({ m: [...m] as M, dash }),
    restore: () => {
      const f = stack.pop();
      if (f) {
        m = f.m;
        dash = f.dash;
      }
    },
    beginPath: () => {
      path = [];
      cur = null;
      local = null;
    },
    closePath: () => {
      closed++;
    },
    moveTo: (x: number, y: number) => {
      cur = add(x, y);
      local = { x, y };
    },
    lineTo: (x: number, y: number) => {
      const p = add(x, y);
      if (cur) path.push({ a: cur, b: p });
      cur = p;
      local = { x, y };
    },
    quadraticCurveTo: (cx: number, cy: number, x: number, y: number) => {
      if (local) {
        curves.push({ from: local, c: { x: cx, y: cy }, to: { x, y } });
        for (let i = 1; i <= 16; i++) {
          const t = i / 16;
          add((1 - t) * (1 - t) * local.x + 2 * (1 - t) * t * cx + t * t * x, (1 - t) * (1 - t) * local.y + 2 * (1 - t) * t * cy + t * t * y);
        }
      }
      cur = map(x, y);
      local = { x, y };
    },
    arc: (x: number, y: number, r: number, a0: number, a1: number, acw = false) => {
      arcs.push({ x, y, r, a0, a1, dash });
      sample(x, y, r, r, 0, a0, a1, acw);
    },
    ellipse: (x: number, y: number, rx: number, ry: number, rot: number, a0: number, a1: number, acw = false) => {
      const c = map(x, y);
      ellipses.push({ x: c.x, y: c.y, rx, ry, rot });
      sample(x, y, rx, ry, rot, a0, a1, acw);
    },
    fill: () => fills.push(ctx.fillStyle),
    stroke: () => {
      for (const s of path) segs.push({ a: s.a, b: s.b, dash });
      path = [];
    },
    translate: (x: number, y: number) => {
      moves.push({ op: "translate", args: [x, y] });
      mul([1, 0, 0, 1, x, y]);
    },
    rotate: (a: number) => {
      moves.push({ op: "rotate", args: [a] });
      mul([Math.cos(a), Math.sin(a), -Math.sin(a), Math.cos(a), 0, 0]);
    },
    scale: (x: number, y: number) => mul([x, 0, 0, y, 0, 0]),
    setTransform: (a: number, b: number, c: number, d: number, e: number, f: number) => {
      m = [a, b, c, d, e, f];
    },
    getTransform: () => ({ a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] }),
    setLineDash: (d: number[]) => {
      dash = d;
    },
    measureText: (t: string) => ({ width: t.length * 6 }),
    fillText: () => undefined,
  };
  const bounds = () => ({
    minX: Math.min(...pts.map((p) => p.x)),
    maxX: Math.max(...pts.map((p) => p.x)),
    minY: Math.min(...pts.map((p) => p.y)),
    maxY: Math.max(...pts.map((p) => p.y)),
  });
  return { ctx: ctx as unknown as CanvasRenderingContext2D, pts, top, segs, arcs, ellipses, curves, fills, moves, closed: () => closed, bounds };
}

const D = 300;
const sym = (key: string, w = 70, d = 40, threeWay = false): DeviceSymbol => {
  const s = deviceSymbol(key, w, d, { symbolMm: D, threeWay });
  if (!s) throw new Error(`no symbol for ${key}`);
  return s;
};
const of = <K extends SymbolPrim["kind"]>(s: DeviceSymbol, kind: K) => s.prims.filter((p): p is Extract<SymbolPrim, { kind: K }> => p.kind === kind);
const texts = (s: DeviceSymbol) => of(s, "text").map((t) => t.text);
const close = (a: number, b: number) => expect(a).toBeCloseTo(b, 6);

describe("catalog coverage", () => {
  it("reads the whole catalog from catalog.rs", () => {
    expect(CATALOG.length).toBeGreaterThanOrEqual(57);
    expect(DEVICE_KEYS).toContain("aircon-window");
    expect(DEVICE_KEYS).toContain("floor-drain");
    expect(DEVICE_KEYS).not.toContain("car-sedan");
    expect(DEVICE_KEYS).not.toContain("sofa-leather");
    // The v2 block: the photoscanned furniture first, the site kit last.
    expect(V2_KEYS[0]).toBe("sofa-leather");
    expect(V2_KEYS).toContain("ceiling-fan");
    expect(V2_KEYS).toContain("kitchen-run");
    expect(V2_KEYS[V2_KEYS.length - 1]).toBe("lamp-post");
    expect(V2_KEYS.length).toBeGreaterThanOrEqual(51);
  });

  it("draws a symbol, never the labelled box, for every catalog key", () => {
    for (const { key, w, d } of CATALOG) {
      const r = recorder();
      expect(drawAssetSymbol(r.ctx, key, w, d, 5, { symbolMm: D }), key).not.toBeNull();
      expect(r.calls.some((c) => c.op === "stroke" || c.op === "fill" || c.op === "fillText"), key).toBe(true);
    }
  });

  it("builds a device symbol for every device, fixture and aircon key", () => {
    for (const key of DEVICE_KEYS) {
      const item = CATALOG.find((c) => c.key === key)!;
      expect(deviceSymbol(key, item.w, item.d, { symbolMm: D }), key).not.toBeNull();
    }
    expect(deviceSymbol("sofa-3", 2100, 900)).toBeNull();
    expect(deviceSymbol("unknown-thing", 900, 600)).toBeNull();
  });

  it("keeps the fallback box for unknown keys", () => {
    expect(drawAssetSymbol(recorder().ctx, "unknown-thing", 900, 600, 5)).toBeNull();
  });
});

describe("contract symbols", () => {
  it("sizes D from the plan scale: 3 mm on paper", () => {
    expect(symbolSizeMm(100)).toBe(300);
    expect(symbolSizeMm(50)).toBe(150);
    expect(symbolSizeMm(0)).toBe(300);
    const at50 = deviceSymbol("light-ceiling", 300, 300, { symbolMm: symbolSizeMm(50) })!;
    close(of(at50, "circle")[0].r, 75);
  });

  it("never scales a D symbol with the object", () => {
    const small = sym("light-ceiling", 300, 300);
    const big = sym("light-ceiling", 900, 900);
    expect(of(small, "circle")[0].r).toBe(of(big, "circle")[0].r);
  });

  it("ceiling and pendant: circle D with an X; pendant adds P", () => {
    for (const key of ["light-ceiling", "light-pendant"]) {
      const s = sym(key, 300, 300);
      const c = of(s, "circle")[0];
      close(c.r, D / 2);
      close(c.x, 0);
      close(c.y, 0);
      const lines = of(s, "line");
      expect(lines).toHaveLength(2);
      for (const l of lines) {
        close(Math.hypot(l.b.x - l.a.x, l.b.y - l.a.y), D);
        close(Math.abs(l.b.x - l.a.x), Math.abs(l.b.y - l.a.y));
      }
    }
    expect(texts(sym("light-ceiling", 300, 300))).toEqual([]);
    expect(texts(sym("light-pendant", 350, 350))).toEqual(["P"]);
  });

  it("downlight: circle 0.6 D with a center dot", () => {
    const s = sym("light-downlight", 150, 150);
    const [ring, dot] = of(s, "circle");
    close(ring.r, 0.3 * D);
    expect(dot.fill).toBe("ink");
    close(dot.x, 0);
    close(dot.y, 0);
  });

  it("tube: its own rectangle with a line along it", () => {
    const s = sym("light-tube", 1200, 100);
    const poly = of(s, "poly")[0];
    expect(poly.points.map((p) => [p.x, p.y])).toEqual([
      [-600, -50],
      [600, -50],
      [600, 50],
      [-600, 50],
    ]);
    const [l] = of(s, "line");
    close(l.a.y, 0);
    close(l.b.y, 0);
    close(l.b.x - l.a.x, 1200);
  });

  it("wall lights: half circle on the wall face with a line; outdoor adds WP", () => {
    for (const [key, d] of [
      ["light-wall", 120],
      ["light-outdoor", 150],
    ] as const) {
      const s = sym(key, 200, d);
      const arc = of(s, "arc")[0];
      close(arc.y, d / 2);
      close(arc.r, D / 2);
      // The arc bulges into the room, -y.
      expect(Math.sin((arc.from + arc.to) / 2)).toBeLessThan(0);
      expect(symbolBounds(s).maxY).toBeLessThanOrEqual(d / 2 + 1e-6);
      expect(of(s, "line")).toHaveLength(1);
    }
    expect(texts(sym("light-outdoor", 150, 150))).toEqual(["WP"]);
  });

  it("plug-in lamps: circle 0.6 D with an X, thin", () => {
    for (const key of ["light-floor-lamp", "light-table-lamp"]) {
      const s = sym(key, 400, 400);
      close(of(s, "circle")[0].r, 0.3 * D);
      expect(of(s, "line")).toHaveLength(2);
      expect(s.weight).toBeLessThan(1);
    }
  });

  it("outlets: circle 0.5 D on the wall face with two parallel lines through it", () => {
    const tags: Record<string, string[]> = {
      "outlet-duplex": [],
      "outlet-counter": [],
      "outlet-outdoor": ["WP"],
      "outlet-spo": ["SPO"],
      "outlet-aircon": ["ACO"],
    };
    for (const [key, tag] of Object.entries(tags)) {
      const d = key === "outlet-outdoor" ? 60 : 40;
      const s = sym(key, 70, d);
      const c = of(s, "circle")[0];
      close(c.r, 0.25 * D);
      close(c.y + c.r, d / 2); // touches the wall face from the room side
      const lines = of(s, "line");
      expect(lines).toHaveLength(2);
      close(lines[0].b.x - lines[0].a.x, lines[1].b.x - lines[1].a.x);
      for (const l of lines) {
        close(l.a.y, l.b.y);
        expect(Math.abs(l.a.y - c.y)).toBeLessThan(c.r);
        expect(l.b.x - l.a.x).toBeGreaterThan(2 * c.r);
      }
      expect(texts(s)).toEqual(tag);
      const half = of(s, "arc").filter((a) => a.fill === "ink");
      expect(half.length, key).toBe(key === "outlet-spo" || key === "outlet-aircon" ? 1 : 0);
      expect(symbolBounds(s).maxY).toBeLessThanOrEqual(d / 2 + 1e-6);
    }
  });

  it("switches: S beside the wall face with one to three dots; S3 when shared", () => {
    for (const n of [1, 2, 3]) {
      const s = sym(`switch-${n}`);
      expect(texts(s)).toEqual(["S"]);
      const [t] = of(s, "text");
      // One label, the S with its dots under it, off the wall face into the room.
      expect(t.dots).toBe(n);
      close(t.y, 20);
      expect(t.away).toEqual({ x: 0, y: -1 });
      expect(t.gap).toBeGreaterThan(0);
      expect(symbolBounds(s).maxY).toBeLessThanOrEqual(20 + 1e-6);
    }
    expect(texts(sym("switch-2", 70, 40, true))).toEqual(["S3"]);
  });

  it("puts tags past the room side edge, clear of the wall", () => {
    for (const key of ["outlet-outdoor", "outlet-spo", "outlet-aircon", "panelboard", "doorbell-button", "light-outdoor", "water-heater", "electric-meter"]) {
      const s = sym(key, 150, 100);
      const tag = of(s, "text").find((t) => t.away)!;
      expect(tag.away, key).toEqual({ x: 0, y: -1 });
      expect(symbolBounds(s).maxY, key).toBeLessThanOrEqual(50 + 1e-6);
    }
  });

  it("panelboard: rectangle on the wall face, half filled on a diagonal, PB", () => {
    const s = sym("panelboard", 350, 100);
    const [box, half] = of(s, "poly");
    expect(box.fill).toBe("body");
    expect(Math.max(...box.points.map((p) => p.y))).toBe(50);
    expect(half.fill).toBe("ink");
    expect(half.points).toHaveLength(3);
    expect(texts(s)).toEqual(["PB"]);
  });

  it("smoke detector, doorbell button and chime", () => {
    const sd = sym("smoke-detector", 120, 120);
    close(of(sd, "circle")[0].r, 0.3 * D);
    expect(texts(sd)).toEqual(["SD"]);
    const pb = sym("doorbell-button", 50, 30);
    expect(of(pb, "circle").map((c) => c.fill)).toEqual(["body", "ink"]);
    expect(texts(pb)).toEqual(["PB"]);
    const ch = sym("doorbell-chime", 120, 50);
    const sq = of(ch, "poly")[0].points;
    close(sq[1].x - sq[0].x, sq[2].y - sq[1].y);
    expect(texts(ch)).toEqual(["CH"]);
  });

  it("aircon: indoor arrow away from the wall, outdoor fan circle, window unit", () => {
    for (const key of ["aircon-indoor-1hp", "aircon-indoor-2hp", "aircon-indoor-3hp"]) {
      const item = CATALOG.find((c) => c.key === key)!;
      const s = sym(key, item.w, item.d);
      expect(texts(s)).toEqual(["ACU"]);
      const shaft = of(s, "line")[0];
      close(shaft.a.y, -item.d / 2);
      expect(shaft.b.y).toBeLessThan(shaft.a.y); // away from the wall, into the room
    }
    for (const key of ["aircon-outdoor-1hp", "aircon-outdoor-3hp"]) {
      const item = CATALOG.find((c) => c.key === key)!;
      const s = sym(key, item.w, item.d);
      expect(of(s, "circle")).toHaveLength(1);
      expect(texts(s)).toEqual(["CU"]);
    }
    // The window unit crosses the wall: its tag sits inside, against the room side edge.
    const win = sym("aircon-window", 471, 482);
    expect(texts(win)).toEqual(["AC"]);
    const ac = of(win, "text")[0];
    expect(ac.away).toEqual({ x: 0, y: 1 });
    close(ac.y, -241);
    const b = symbolBounds(win);
    expect(b.minY).toBeGreaterThanOrEqual(-241 - 1e-6);
  });

  it("septic tank is dashed; the other fixtures are not", () => {
    expect(sym("septic-tank", 1800, 1200).dashed).toBe(true);
    for (const key of ["floor-drain", "water-heater", "water-meter", "water-tank", "lpg-cylinder", "electric-meter"]) {
      expect(sym(key, 300, 300).dashed, key).toBe(false);
    }
    const r = recorder();
    drawAssetSymbol(r.ctx, "septic-tank", 1800, 1200, 5, { symbolMm: D });
    expect(r.calls.find((c) => c.op === "setLineDash")?.args[0]).toEqual([30, 20]);
  });
});

describe("drawing", () => {
  it("fills ink parts with the stroke color, so tints reach them", () => {
    const r = recorder();
    r.ctx.strokeStyle = "#0e8a8f";
    const fills: unknown[] = [];
    const ctx = r.ctx as unknown as { fill: () => void; fillStyle: unknown };
    ctx.fill = () => fills.push(ctx.fillStyle);
    drawAssetSymbol(r.ctx, "outlet-spo", 70, 40, 5, { symbolMm: D });
    expect(fills).toContain("#0e8a8f");
    expect(fills).toContain("rgba(255,255,255,0.78)");
  });

  it("prints the tags upright and skips them when too small to read", () => {
    const r = recorder();
    drawAssetSymbol(r.ctx, "switch-2", 70, 40, 5, { symbolMm: D, threeWay: true });
    expect(r.texts).toEqual(["S3"]);
    expect(r.calls.some((c) => c.op === "setTransform")).toBe(true);
    // Two gang dots under the S3.
    expect(r.calls.filter((c) => c.op === "arc")).toHaveLength(2);
    const far = recorder();
    drawAssetSymbol(far.ctx, "switch-1", 70, 40, 200, { symbolMm: D });
    expect(far.texts).toEqual([]);
  });

  it("pushes a wide tag clear of its symbol when the object is turned", () => {
    // Screen position of the "SPO" tag with the editor's transform at 0.2 px
    // per mm: rotate(-rot) then scale(0.2, -0.2), as drawAsset does.
    const vs = 0.2;
    const tagAt = (rotDeg: number): { x: number; y: number } => {
      const t = (rotDeg * Math.PI) / 180;
      type M = { a: number; b: number; c: number; d: number; e: number; f: number };
      let m: M = { a: vs * Math.cos(t), b: -vs * Math.sin(t), c: -vs * Math.sin(t), d: -vs * Math.cos(t), e: 0, f: 0 };
      const stack: M[] = [];
      let at = { x: NaN, y: NaN };
      const r = recorder();
      Object.assign(r.ctx, {
        save: () => stack.push({ ...m }),
        restore: () => {
          m = stack.pop() ?? m;
        },
        translate: (x: number, y: number) => {
          m = { ...m, e: m.e + m.a * x + m.c * y, f: m.f + m.b * x + m.d * y };
        },
        getTransform: () => ({ ...m }),
        setTransform: (a: number, b: number, c: number, d: number, e: number, f: number) => {
          m = { a, b, c, d, e, f };
        },
        fillText: (text: string, x: number, y: number) => {
          if (text === "SPO") at = { x: m.e + m.a * x + m.c * y, y: m.f + m.b * x + m.d * y };
        },
      });
      drawAssetSymbol(r.ctx, "outlet-spo", 70, 40, 1 / vs, { symbolMm: D });
      return at;
    };
    // The outlet circle: center 55 mm into the room, radius 75 mm, so its far
    // edge is 130 mm (26 px) from the object's center.
    const edge = (0.25 * D * 2 - 20) * vs;
    const unturned = tagAt(0);
    close(unturned.x, 0);
    expect(unturned.y - (0.24 * D * vs) / 2).toBeGreaterThan(edge);
    // Turned a quarter the tag goes sideways on screen, but stays upright: it
    // is pushed out by half its width (18 px in the mock), not its height.
    const turned = tagAt(90);
    close(turned.y, 0);
    expect(turned.x - 9).toBeGreaterThan(edge);
  });

  it("draws plug-in lamps thinner", () => {
    const r = recorder();
    const widths: number[] = [];
    const ctx = r.ctx as unknown as { stroke: () => void; lineWidth: number };
    ctx.lineWidth = 2;
    ctx.stroke = () => widths.push(ctx.lineWidth);
    drawAssetSymbol(r.ctx, "light-table-lamp", 300, 300, 5, { symbolMm: D });
    expect(Math.max(...widths)).toBeCloseTo(1.1, 6);
  });
});

describe("anchors, outlines and 3-way switches", () => {
  const asset = (key: string, x: number, y: number, rot: number, links: string[] = [], id = key): Asset => ({
    id,
    level_id: "L1",
    catalog_key: key,
    name: key,
    category: "electrical",
    position: { x, y },
    rotation_deg: rot,
    width_mm: 70,
    depth_mm: 40,
    height_mm: 115,
    elevation_mm: 1143,
    light: null,
    links,
    circuit: "",
  });

  it("anchors a switch at its label, turned with the object", () => {
    // The S with one dot is 1.48 text heights tall, 0.08 D off the wall face.
    const off = 20 - 0.08 * D - 0.74 * 0.36 * D;
    const a = assetSymbolAnchor(asset("switch-1", 1000, 2000, 0), { symbolMm: D });
    close(a.x, 1000);
    close(a.y, 2000 + off);
    const turned = assetSymbolAnchor(asset("switch-1", 1000, 2000, 90), { symbolMm: D });
    close(turned.x, 1000 - off);
    close(turned.y, 2000);
  });

  it("anchors a light at its center", () => {
    const light = { ...asset("light-ceiling", 3000, 4000, 30), width_mm: 300, depth_mm: 300 };
    const a = assetSymbolAnchor(light, { symbolMm: D });
    close(a.x, 3000);
    close(a.y, 4000);
    close(a.r, D / 2);
  });

  it("outlines the whole symbol, not only the small footprint", () => {
    const o = assetSymbolOutline(asset("outlet-duplex", 0, 0, 0), { symbolMm: D })!;
    const ys = o.map((p) => p.y);
    expect(Math.min(...ys)).toBeLessThanOrEqual(20 - 0.5 * D);
    expect(Math.max(...ys)).toBeCloseTo(20, 6);
    expect(assetSymbolOutline({ ...asset("sofa-3", 0, 0, 0) }, { symbolMm: D })).toBeNull();
  });

  it("finds switches that share a light that exists", () => {
    const light = (id: string, key = "light-ceiling"): Element => ({ kind: "asset", ...asset(key, 0, 0, 0, [], id), category: "lighting" });
    const els: Element[] = [
      light("L-a"),
      light("L-c", "light-downlight"),
      { kind: "asset", ...asset("switch-1", 0, 0, 0, ["L-a"], "s1") },
      { kind: "asset", ...asset("switch-2", 0, 0, 0, ["L-a", "L-b"], "s2") },
      // An outlet on the same light does not make a 3-way.
      { kind: "asset", ...asset("switch-1", 0, 0, 0, ["L-c"], "s3") },
      { kind: "asset", ...asset("outlet-aircon", 0, 0, 0, ["L-c"], "o1") },
      // Two switches still pointing at a deleted light are not a 3-way either.
      { kind: "asset", ...asset("switch-1", 0, 0, 0, ["gone"], "s4") },
      { kind: "asset", ...asset("switch-1", 0, 0, 0, ["gone"], "s5") },
    ];
    expect([...threeWaySwitches(els)].sort()).toEqual(["s1", "s2"]);
  });
});

describe("catalog v2 symbols", () => {
  const TOL = 0.5;
  const PX = 5;
  const item = (key: string) => CATALOG.find((c) => c.key === key)!;
  const run = (key: string) => {
    const { w, d } = item(key);
    const t = tracer();
    expect(drawAssetSymbol(t.ctx, key, w, d, PX, { symbolMm: D }), key).not.toBeNull();
    return { t, w, d, b: t.bounds() };
  };
  /** The calls a symbol makes, to say two keys draw the same thing. */
  const calls = (key: string, w: number, d: number) => {
    const r = recorder();
    drawAssetSymbol(r.ctx, key, w, d, PX, { symbolMm: D });
    return r.calls;
  };
  const horizontal = (s: { a: P2; b: P2 }, y: number) => Math.abs(s.a.y - y) < 1e-6 && Math.abs(s.b.y - y) < 1e-6;

  it("keeps every new symbol inside its own footprint", () => {
    // Out on purpose, -y mm: the desk's chair is pulled out in front (120 + half of 440),
    // a gate swings into the lot, a mirror's tick marks its face.
    const front: Record<string, number> = { "desk-metal": 340, "gate-iron": 1475, "mirror-wall": 4 * PX };
    for (const key of V2_KEYS) {
      const { w, d, b } = run(key);
      expect(b.minX, `${key} left`).toBeGreaterThanOrEqual(-w / 2 - TOL);
      expect(b.maxX, `${key} right`).toBeLessThanOrEqual(w / 2 + TOL);
      expect(b.maxY, `${key} back`).toBeLessThanOrEqual(d / 2 + TOL);
      expect(b.minY, `${key} front`).toBeGreaterThanOrEqual(-d / 2 - (front[key] ?? 0) - TOL);
    }
  });

  it("fills most of the footprint, so no symbol is drawn a fraction of its size", () => {
    for (const key of V2_KEYS) {
      const { w, d, b } = run(key);
      if (key === "gate-iron" || key === "desk-metal") continue; // the swing and the chair reach out of it
      expect(b.maxX - b.minX, `${key} width`).toBeGreaterThanOrEqual(w * 0.5);
      expect(b.maxY - b.minY, `${key} depth`).toBeGreaterThanOrEqual(Math.min(d, Math.max(d * 0.6, PX * 2.5)) - TOL);
    }
  });

  it("draws the leather and wood sofas, the lounge and classic chairs and the electric range like their plain kin", () => {
    expect(calls("sofa-leather", 2100, 900)).toEqual(calls("sofa-3", 2100, 900));
    expect(calls("sofa-wood", 2100, 900)).toEqual(calls("sofa-3", 2100, 900));
    expect(calls("lounge-chair", 850, 850)).toEqual(calls("armchair", 850, 850));
    expect(calls("armchair-classic", 850, 850)).toEqual(calls("armchair", 850, 850));
    expect(calls("tree-flowering", 3000, 3000)).toEqual(calls("tree", 3000, 3000));
    expect(calls("range-electric", 600, 600)).toEqual(calls("range", 600, 600));
  });

  it("rocking chair: an armchair between two bowed rockers on its sides", () => {
    const { t, w, d } = run("rocking-chair");
    const rockers = t.curves.filter((c) => Math.abs(c.c.x - c.to.x) > 1 && Math.abs(c.c.y - c.to.y) > 1);
    expect(rockers).toHaveLength(2);
    expect(Math.sign(rockers[0].c.x)).toBe(-Math.sign(rockers[1].c.x));
    for (const r of rockers) {
      expect(Math.abs(r.to.y - r.from.y)).toBeGreaterThan(d * 0.9);
      expect(Math.abs(r.from.x)).toBeGreaterThan(w / 2 - 80);
      expect(Math.abs(r.c.x)).toBeGreaterThan(Math.abs(r.from.x)); // bows out, away from the seat
    }
  });

  it("round pieces are circles with an inner ring", () => {
    for (const key of ["coffee-table-round", "side-table-round", "vase-ceramic", "stool-wood"]) {
      const { t, w } = run(key);
      expect(t.ellipses, key).toHaveLength(2);
      const [outer, inner] = t.ellipses;
      close(outer.rx, w / 2);
      close(outer.ry, w / 2);
      expect(inner.rx).toBeLessThan(outer.rx);
      close(inner.x, 0);
      close(inner.y, 0);
    }
  });

  it("square tables and the ottoman: a slab with an inset line", () => {
    for (const key of ["coffee-table-square", "tea-table", "side-table", "table-wood", "ottoman"]) {
      const { t, w, d, b } = run(key);
      expect(t.fills, key).toHaveLength(1);
      close(b.maxX - b.minX, w);
      close(b.maxY - b.minY, d);
      // The inner outline: an edge at the inset from the left, shorter than the slab.
      const k = Math.min(70, Math.min(w, d) * 0.15);
      expect(
        t.pts.some((p) => Math.abs(p.x - (-w / 2 + k)) < 1e-6 && Math.abs(p.y) < d / 2 - k),
        key,
      ).toBe(true);
    }
  });

  it("round bistro table with two chairs, all inside the square", () => {
    const { t, w, d, b } = run("dining-round-2");
    const cs = w * 0.36;
    expect(t.moves.filter((m) => m.op === "translate").map((m) => m.args)).toEqual([
      [0, d / 2 - cs / 2],
      [0, -d / 2 + cs / 2],
    ]);
    expect(t.moves.filter((m) => m.op === "rotate").map((m) => m.args[0])).toEqual([0, Math.PI]);
    const table = t.ellipses[0];
    close(table.rx, table.ry);
    expect(table.rx).toBeLessThan(w / 2); // shrunk to leave room for the chairs
    close(b.maxY - b.minY, d);
  });

  it("outdoor set: a small round table in the middle, a chair at each end of the depth", () => {
    const { t, w, d, b } = run("outdoor-set");
    const chairs = t.moves.filter((m) => m.op === "translate").map((m) => m.args[1]);
    expect(chairs).toHaveLength(2);
    expect(Math.sign(chairs[0])).toBe(-Math.sign(chairs[1]));
    close(t.ellipses[0].x, 0);
    close(t.ellipses[0].y, 0);
    expect(t.ellipses[0].rx).toBeLessThan(w / 2);
    close(b.maxY - b.minY, d);
  });

  it("dining sets: the table between a row of chairs on each long side, inside the depth", () => {
    for (const [key, seats, tableDepth] of [
      ["dining-set-4", 4, 800],
      ["dining-set-6", 6, 900],
    ] as const) {
      const { t, w, d, b } = run(key);
      const ys = t.moves.filter((m) => m.op === "translate").map((m) => m.args[1]);
      expect(ys, key).toHaveLength(seats);
      expect(ys.filter((y) => y > 0)).toHaveLength(seats / 2);
      expect(ys.filter((y) => y < 0)).toHaveLength(seats / 2);
      for (const y of ys) expect(Math.abs(y)).toBeGreaterThan(tableDepth / 2);
      // The table itself is drawn outside any chair transform: centered, full width.
      const xs = t.top.map((p) => p.x);
      const yt = t.top.map((p) => p.y);
      close(Math.max(...xs) - Math.min(...xs), w);
      close(Math.max(...yt) - Math.min(...yt), tableDepth);
      close(b.maxY - b.minY, d);
    }
  });

  it("dining chairs fill their footprint with the back rest at the back", () => {
    for (const key of ["chair-dining", "chair-wood-white", "chair-monobloc"]) {
      const { t, w, d, b } = run(key);
      close(b.maxX - b.minX, w);
      close(b.maxY - b.minY, d);
      const rest = t.segs.filter((s) => horizontal(s, d / 2 - d * 0.18));
      expect(rest, key).toHaveLength(1);
      close(Math.abs(rest[0].b.x - rest[0].a.x), w * 0.8);
    }
  });

  it("case furniture marks its doors or drawers on the front half only", () => {
    const cells: Record<string, number> = { sideboard: 4, "cabinet-white": 2, dresser: 3, "cabinet-drawers": 2, nightstand: 1 };
    for (const [key, n] of Object.entries(cells)) {
      const { t, w, d } = run(key);
      const ticks = t.segs.filter((s) => Math.abs(s.a.x - s.b.x) < 1e-6 && Math.abs(s.a.x) < w / 2 - 1);
      expect(ticks, key).toHaveLength(n - 1);
      for (const s of ticks) {
        expect(Math.max(s.a.y, s.b.y), key).toBeLessThanOrEqual(1e-6);
        expect(Math.min(s.a.y, s.b.y), key).toBeCloseTo(-d / 2, 6);
      }
      // Drawers have a knob each, doors none.
      expect(t.ellipses, key).toHaveLength(["dresser", "cabinet-drawers", "nightstand"].includes(key) ? n : 0);
    }
  });

  it("shelves: cube shelves cross-braced, steel shelves with a post at each corner", () => {
    const cube = run("shelf-cube");
    const diagonals = cube.t.segs.filter((s) => Math.abs(s.b.y - s.a.y) > 1 && Math.abs(Math.abs(s.b.x - s.a.x) - (cube.w - 80)) < 1e-6);
    expect(diagonals).toHaveLength(2);
    const steel = run("shelf-steel");
    expect(steel.t.ellipses).toHaveLength(4);
    expect(steel.t.ellipses.map((e) => [Math.sign(e.x), Math.sign(e.y)]).sort()).toEqual([
      [-1, -1],
      [-1, 1],
      [1, -1],
      [1, 1],
    ]);
    // Two shelf lines between the end frames.
    const slats = steel.t.segs.filter((s) => horizontal(s, s.a.y) && Math.abs(s.a.y) < steel.d / 2 - 1 && Math.abs(s.b.x - s.a.x) > steel.w - 1);
    expect(slats).toHaveLength(2);
  });

  it("rattan basket: a rounded basket with a diagonal weave", () => {
    const { t, w, d } = run("basket-rattan");
    const weave = t.segs.filter((s) => Math.abs(Math.abs(s.b.x - s.a.x) - Math.abs(s.b.y - s.a.y)) < 1e-6 && Math.abs(s.b.x - s.a.x) > 1);
    expect(weave.length).toBeGreaterThanOrEqual(4);
    for (const s of weave) {
      for (const p of [s.a, s.b]) {
        expect(Math.abs(p.x)).toBeLessThan(w / 2 - 50);
        expect(Math.abs(p.y)).toBeLessThan(d / 2 - 50);
      }
    }
  });

  it("plants are a ring of leaves around a core, out to the smaller side", () => {
    const leaves: Record<string, number> = { "plant-leafy": 9, "plant-succulent": 12 };
    for (const [key, n] of Object.entries(leaves)) {
      const { t, w, d } = run(key);
      expect(t.ellipses, key).toHaveLength(n + 1);
      const reach = Math.max(...t.pts.map((p) => Math.hypot(p.x, p.y)));
      expect(reach, key).toBeCloseTo(Math.min(w, d) / 2, 0);
    }
  });

  it("ceiling fan: a hub and five thin blades out to r, lines only", () => {
    const { t, w, d } = run("ceiling-fan");
    const r = Math.min(w, d) / 2;
    expect(t.ellipses).toHaveLength(6);
    expect(t.fills).toHaveLength(0); // outlines only
    const [hub, ...blades] = t.ellipses;
    expect(hub.rx).toBeLessThan(r * 0.2);
    for (const bl of blades) {
      expect(bl.ry).toBeLessThan(bl.rx * 0.3); // long and narrow
      expect(Math.hypot(bl.x, bl.y) + bl.rx).toBeCloseTo(r, 6); // reaches the circle
    }
    expect(Math.max(...t.pts.map((p) => Math.hypot(p.x, p.y)))).toBeCloseTo(r, 3);
  });

  it("wall art and mirror: a thin rectangle across the full width, on the wall half", () => {
    for (const key of ["wall-art", "mirror-wall"]) {
      const { w, d, b } = run(key);
      close(b.maxX - b.minX, w);
      expect(b.maxY, key).toBeLessThanOrEqual(d / 2 + TOL); // nothing behind the wall face
      // The slab is as thin as the object (2.5 pixels at least); only the mirror's tick leaves it.
      expect(b.minY, key).toBeGreaterThanOrEqual(d / 2 - Math.max(d, 2.5 * PX) - (key === "mirror-wall" ? 4 * PX : 0) - TOL);
    }
    // Only the mirror has the tick toward the room, at the center.
    const art = run("wall-art");
    const mirror = run("mirror-wall");
    const ticks = (r: typeof art) => r.t.segs.filter((s) => Math.abs(s.a.x) < 1e-6 && Math.abs(s.b.x) < 1e-6);
    expect(ticks(art)).toHaveLength(0);
    const [tick] = ticks(mirror);
    expect(tick.b.y).toBeLessThan(tick.a.y);
    expect(tick.a.y - tick.b.y).toBeCloseTo(4 * PX, 6);
    // Zoomed far out the slab keeps 2.5 pixels, growing toward the room, not into the wall.
    const far = tracer();
    drawAssetSymbol(far.ctx, "wall-art", 600, 20, 40, { symbolMm: D });
    expect(far.bounds().maxY).toBeCloseTo(10, 6);
    expect(far.bounds().minY).toBeCloseTo(10 - 100, 6);
  });

  it("desk-metal draws like the desk, its chair pulled out in front only", () => {
    expect(calls("desk-metal", 1600, 760)).toEqual(calls("desk", 1600, 760));
    const { t, w, d, b } = run("desk-metal");
    expect(b.minY).toBeLessThan(-d / 2 - 100);
    expect(b.maxY).toBeLessThanOrEqual(d / 2 + TOL);
    expect(b.maxX).toBeLessThanOrEqual(w / 2 + TOL);
    expect(t.moves.filter((m) => m.op === "translate").map((m) => m.args)).toEqual([[0, -d / 2 - 120]]);
  });

  it("microwave: a body with a door and a control strip of three buttons on the right", () => {
    const { t, w, d } = run("microwave");
    const buttons = t.segs.filter(
      (s) => Math.abs(s.a.y - s.b.y) < 1e-6 && Math.abs(s.a.y) < d / 2 - 31 && s.a.x > w / 2 - 120 && s.b.x > w / 2 - 120 && Math.abs(s.b.x - s.a.x) > 30,
    );
    expect(buttons).toHaveLength(3);
  });

  it("bench: a back rest against +y and seat slats in front of it", () => {
    const { t, w, d } = run("bench-wood");
    expect(t.segs.some((s) => horizontal(s, d / 2 - 90) && Math.abs(s.b.x - s.a.x) > w - 60)).toBe(true);
    const slats = t.segs.filter((s) => horizontal(s, s.a.y) && s.a.y < d / 2 - 90 - 1 && s.a.y > -d / 2 + 1 && Math.abs(s.b.x - s.a.x) > w - 100);
    expect(slats).toHaveLength(2);
  });

  it("planter box: an inner box with a few plant dots in a row", () => {
    const { t, w, d } = run("planter-box");
    const dots = t.ellipses.filter((e) => e.rx > 30);
    expect(dots.length).toBeGreaterThanOrEqual(3);
    for (const e of dots) {
      close(e.y, 0);
      expect(Math.abs(e.x) + e.rx).toBeLessThan(w / 2 - 50);
      expect(e.ry).toBeLessThan((d - 100) / 2);
    }
  });

  it("covered car: the car, with its mirrors at the footprint edge", () => {
    const { t, w, d, b } = run("car-covered");
    close(b.maxX, w / 2);
    close(b.minX, -w / 2);
    expect(b.maxY).toBeLessThanOrEqual(d / 2 + TOL);
    expect(t.fills.length).toBeGreaterThan(2);
  });

  it("iron gate: two leaves on the posts, each with a dashed quarter-circle swing into the lot", () => {
    const { t, w } = run("gate-iron");
    expect(t.arcs).toHaveLength(2);
    const leaves = t.segs.filter((s) => horizontal(s, 0) && s.dash.length === 0);
    expect(leaves).toHaveLength(2);
    for (const l of leaves) expect(Math.abs(l.b.x - l.a.x)).toBeGreaterThan(w * 0.45);
    for (const a of t.arcs) {
      expect(a.dash).toEqual([6 * PX, 4 * PX]);
      close(a.y, 0);
      close(Math.abs(a.a1 - a.a0), Math.PI / 2); // a quarter turn
      // It swings toward the room, -y.
      expect(Math.sin((a.a0 + a.a1) / 2)).toBeLessThan(0);
    }
    expect(Math.sign(t.arcs[0].x)).toBe(-Math.sign(t.arcs[1].x));
    close(t.arcs[0].r, t.arcs[1].r);
    // The swings meet at the center, where the leaves close.
    close(t.arcs[0].x + t.arcs[0].r, -(t.arcs[1].x - t.arcs[1].r));
  });

  it("steel gate: the frame, a tick for each bar and an arrow along x for the slide", () => {
    const { t, w, d } = run("gate-steel");
    const bars = t.segs.filter((s) => Math.abs(s.a.x - s.b.x) < 1e-6 && Math.abs(s.a.x) < w / 2 - 1);
    expect(bars.length).toBeGreaterThanOrEqual(20);
    for (const s of bars) expect(Math.abs(s.b.y - s.a.y)).toBeLessThan(d / 2);
    const shaft = t.segs.filter((s) => horizontal(s, 0));
    expect(shaft).toHaveLength(1);
    expect(shaft[0].b.x - shaft[0].a.x).toBeGreaterThan(w * 0.5);
    // Two head strokes start at the shaft's tip and fan out either side of it.
    const tip = shaft[0].b;
    const head = t.segs.filter((s) => Math.abs(s.a.x - tip.x) < 1e-6 && Math.abs(s.a.y - tip.y) < 1e-6);
    expect(head).toHaveLength(2);
    expect(Math.sign(head[0].b.y)).toBe(-Math.sign(head[1].b.y));
    expect(t.segs.every((s) => s.dash.length === 0)).toBe(true);
  });

  it("hedge: a closed, filled cloud with scallops along both long sides", () => {
    const { t, w, d } = run("hedge");
    const top = t.arcs.filter((a) => a.y > 0);
    const bottom = t.arcs.filter((a) => a.y < 0);
    expect(top.length).toBeGreaterThanOrEqual(5);
    expect(bottom).toHaveLength(top.length);
    expect(t.closed()).toBe(1);
    expect(t.fills).toHaveLength(1);
    // The scallops overlap: neighbors are closer than a diameter.
    const xs = top.map((a) => a.x).sort((p, q) => p - q);
    expect(xs[1] - xs[0]).toBeLessThan(2 * top[0].r);
    const b = t.bounds();
    close(b.maxX - b.minX, w);
    close(b.maxY - b.minY, d);
  });

  it("lamp posts: a circle to the smaller side with an X and a dot in the stroke color", () => {
    for (const key of ["lamp-post", "lamp-post-classic"]) {
      const { t, w, d } = run(key);
      const [ring, center] = t.ellipses;
      close(ring.rx, Math.min(w, d) / 2);
      close(ring.ry, Math.min(w, d) / 2);
      const diag = t.segs.filter((s) => Math.abs(Math.abs(s.b.x - s.a.x) - Math.abs(s.b.y - s.a.y)) < 1e-6);
      expect(diag, key).toHaveLength(2);
      expect(center.rx, key).toBeLessThan(ring.rx * 0.3);
      expect(t.fills[t.fills.length - 1], key).toBe("#445566");
    }
  });
});

describe("built-ins", () => {
  const PX = 5;
  const TOL = 0.5;
  const W = 2400;
  const Dp = 600;
  const params = (p: Partial<BuiltInParams>): BuiltInParams => ({ upper: false, sink_at_mm: null, cooktop_at_mm: null, doors: 1, ...p });
  const run = (key: string, w: number, d: number, builtIn: BuiltInParams | null = null) => {
    const t = tracer();
    expect(drawAssetSymbol(t.ctx, key, w, d, PX, { symbolMm: D, builtIn }), key).toBe("");
    return t;
  };
  const horizontal = (s: { a: P2; b: P2 }, y: number) => Math.abs(s.a.y - y) < 1e-6 && Math.abs(s.b.y - y) < 1e-6;
  /** Vertical ticks that start on the front edge, inside the ends: the door joints. */
  const joints = (t: ReturnType<typeof tracer>, w: number, d: number) =>
    t.segs.filter((s) => Math.abs(s.a.x - s.b.x) < 1e-6 && Math.abs(s.a.y + d / 2) < 1e-6 && Math.abs(s.a.x) < w / 2 - 1);

  it("kitchen run draws its sink where sink_at_mm says, from the left end, and moves it", () => {
    // Only the tap is a circle when there is no cooktop.
    const at = (mm: number | null, w = W) => run("kitchen-run", w, Dp, params({ sink_at_mm: mm })).ellipses;
    const tap = at(700);
    expect(tap).toHaveLength(1);
    close(tap[0].x, -W / 2 + 700);
    close(at(1500)[0].x, -W / 2 + 1500);
    close(at(500, 3000)[0].x, -3000 / 2 + 500);
    expect(at(null)).toHaveLength(0);
    // The two bowls sit either side of the tap: their sides are centered on it.
    const t = run("kitchen-run", W, Dp, params({ sink_at_mm: 1500 }));
    const sides = t.pts.filter((p) => Math.abs(p.x) < W / 2 - 1 && p.y > -Dp * 0.26 + 1 && p.y < Dp * 0.24 - 1);
    close((Math.min(...sides.map((p) => p.x)) + Math.max(...sides.map((p) => p.x))) / 2, -W / 2 + 1500);
  });

  it("kitchen run uses the defaults for its width when it has no options", () => {
    // 2400 wide: sink at 700, cooktop at 1800 (builtins.ts), wall cabinets on, five doors.
    const t = run("kitchen-run", W, Dp);
    const tap = t.ellipses.reduce((a, e) => (e.rx < a.rx ? e : a));
    close(tap.x, -W / 2 + 700);
    expect(t.ellipses).toHaveLength(5);
    expect(t.segs.some((s) => s.dash.length > 0)).toBe(true);
    expect(joints(t, W, Dp)).toHaveLength(4);
    // A short run has a sink at 450 and no cooktop.
    const small = run("kitchen-run", 1200, Dp);
    expect(small.ellipses).toHaveLength(1);
    close(small.ellipses[0].x, -1200 / 2 + 450);
  });

  it("kitchen run puts four burners on the cooktop, centered at cooktop_at_mm", () => {
    const t = run("kitchen-run", W, Dp, params({ cooktop_at_mm: 1800 }));
    expect(t.ellipses).toHaveLength(4);
    close(t.ellipses.reduce((a, e) => a + e.x, 0) / 4, -W / 2 + 1800);
    const moved = run("kitchen-run", W, Dp, params({ cooktop_at_mm: 600 }));
    close(moved.ellipses.reduce((a, e) => a + e.x, 0) / 4, -W / 2 + 600);
    // A sink and a cooktop together: five circles.
    expect(run("kitchen-run", W, Dp, params({ sink_at_mm: 700, cooktop_at_mm: 1800 })).ellipses).toHaveLength(5);
  });

  it("kitchen run keeps the sink and the cooktop inside its ends", () => {
    const b = run("kitchen-run", 1500, Dp, params({ sink_at_mm: 20, cooktop_at_mm: 4000 })).bounds();
    expect(b.minX).toBeGreaterThanOrEqual(-750 - TOL);
    expect(b.maxX).toBeLessThanOrEqual(750 + TOL);
  });

  it("kitchen run draws the wall cabinet line, dashed, 350 mm from the back, only when upper", () => {
    const withUpper = run("kitchen-run", W, Dp, params({ upper: true }));
    const dashed = withUpper.segs.filter((s) => s.dash.length > 0);
    expect(dashed).toHaveLength(1);
    expect(horizontal(dashed[0], Dp / 2 - 350)).toBe(true);
    close(Math.abs(dashed[0].b.x - dashed[0].a.x), W);
    expect(dashed[0].dash).toEqual([8 * PX, 5 * PX]);
    const without = run("kitchen-run", W, Dp, params({ upper: false }));
    expect(without.segs.filter((s) => s.dash.length > 0)).toHaveLength(0);
    // Too shallow for a 350 mm cabinet: the line would leave the footprint, so it is not drawn.
    expect(run("kitchen-run", W, 340, params({ upper: true })).segs.filter((s) => s.dash.length > 0)).toHaveLength(0);
  });

  it("kitchen run, counter, vanity and wardrobe mark one joint fewer than their doors", () => {
    for (const key of ["kitchen-run", "counter", "vanity", "wardrobe-builtin"]) {
      for (const doors of [1, 3, 6]) {
        const t = run(key, 1800, 600, params({ doors }));
        expect(joints(t, 1800, 600), `${key} ${doors}`).toHaveLength(doors - 1);
      }
    }
  });

  it("kitchen run and counter have the counter's front line 40 mm in", () => {
    for (const key of ["kitchen-run", "counter"]) {
      const front = run(key, 1500, 600).segs.filter((s) => horizontal(s, -300 + 40));
      expect(front, key).toHaveLength(1);
      close(Math.abs(front[0].b.x - front[0].a.x), 1500);
    }
  });

  it("counter has no sink and no cooktop", () => {
    expect(run("counter", 1500, 600).ellipses).toHaveLength(0);
    expect(run("counter", 1500, 600, params({ sink_at_mm: 500, cooktop_at_mm: 900 })).ellipses).toHaveLength(0);
  });

  it("vanity: a basin oval at sink_at_mm from the left with its tap behind it", () => {
    const at = (mm: number | null) => run("vanity", 900, 500, params({ upper: true, sink_at_mm: mm })).ellipses;
    const [basin, tap] = at(300);
    close(basin.x, -450 + 300);
    close(tap.x, basin.x);
    expect(basin.rx).toBeGreaterThan(basin.ry); // an oval
    expect(tap.y).toBeGreaterThan(basin.y + basin.ry); // behind it, +y
    close(at(600)[0].x, -450 + 600);
    close(at(880)[0].x, -450 + 620); // kept 20 mm clear of the end
    close(at(null)[0].x, 0); // no option: the middle
    close(run("vanity", 900, 500).ellipses[0].x, 0); // the default is the middle too
  });

  it("wardrobe: the rod and hanger ticks of a wardrobe, plus front joints for its doors", () => {
    const t = run("wardrobe-builtin", 1800, 600);
    expect(t.segs.filter((s) => horizontal(s, 0))).toHaveLength(1);
    const hangers = t.segs.filter((s) => Math.abs(Math.abs(s.b.x - s.a.x) - 80) < 1e-6);
    expect(hangers.length).toBeGreaterThanOrEqual(3);
    expect(joints(t, 1800, 600)).toHaveLength(3); // 1800 / 500 = 4 doors by default
  });

  it("shelf: dashed outline with two or three shelf lines parallel to the wall", () => {
    for (const [d, lines] of [
      [250, 2],
      [400, 3],
    ] as const) {
      const t = run("shelf-wall", 1200, d);
      expect(t.segs.length).toBeGreaterThan(0);
      expect(t.segs.every((s) => s.dash.length > 0)).toBe(true);
      const shelves = t.segs.filter((s) => horizontal(s, s.a.y) && Math.abs(s.a.y) < d / 2 - 1 && Math.abs(s.b.x - s.a.x) > 1199);
      expect(shelves, `d ${d}`).toHaveLength(lines);
      expect(t.fills).toHaveLength(0); // hollow: it is above the floor
    }
  });

  it("every built-in stays inside its footprint, with its options or without", () => {
    const cases: [string, number, number, BuiltInParams | null][] = [
      ["kitchen-run", 2400, 600, null],
      ["kitchen-run", 3600, 650, params({ upper: true, sink_at_mm: 800, cooktop_at_mm: 2600, doors: 7 })],
      ["kitchen-run", 900, 600, null],
      ["wardrobe-builtin", 1800, 600, null],
      ["vanity", 900, 500, null],
      ["vanity", 600, 450, params({ sink_at_mm: 100, doors: 1 })],
      ["counter", 1500, 600, null],
      ["shelf-wall", 1200, 250, null],
    ];
    for (const [key, w, d, builtIn] of cases) {
      const b = run(key, w, d, builtIn).bounds();
      expect(b.minX, key).toBeGreaterThanOrEqual(-w / 2 - TOL);
      expect(b.maxX, key).toBeLessThanOrEqual(w / 2 + TOL);
      expect(b.minY, key).toBeGreaterThanOrEqual(-d / 2 - TOL);
      expect(b.maxY, key).toBeLessThanOrEqual(d / 2 + TOL);
    }
  });
});
