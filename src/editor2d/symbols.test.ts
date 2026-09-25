import { describe, expect, it } from "vitest";
import type { Asset, Element } from "../contract/bindings";
import catalogSource from "../../crates/guhit-model/src/defaults.rs?raw";
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

/** Keys added with the devices, fixtures and aircon (they must not be drawn as boxes). */
const DEVICE_KEYS = CATALOG.map((c) => c.key).filter((k) => CATALOG.findIndex((c) => c.key === "floor-drain") <= CATALOG.findIndex((c) => c.key === k));

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
  it("reads the whole catalog from defaults.rs", () => {
    expect(CATALOG.length).toBeGreaterThanOrEqual(57);
    expect(DEVICE_KEYS).toContain("aircon-window");
    expect(DEVICE_KEYS).toContain("floor-drain");
    expect(DEVICE_KEYS).not.toContain("car-sedan");
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
