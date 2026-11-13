import { describe, expect, it } from "vitest";
import { captureInfo, countIds, depthGray, flipRows, hexOf, idColor, idIndexOf, legendFrom, linesFrom, mapsSize, normalRgba, pngSizeOfDataUrl } from "./conditioning";

/** An RGBA ids buffer of `w` x `h` where `at(x, y)` gives the element index. */
function ids(w: number, h: number, at: (x: number, y: number) => number): Uint8Array {
  const out = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const [r, g, b] = idColor(at(x, y));
      out.set([r, g, b, 255], (y * w + x) * 4);
    }
  }
  return out;
}

/** Normal and depth floats: one normal and depth for the whole buffer, or per pixel. */
function nd(w: number, h: number, at: (x: number, y: number) => [number, number, number, number]): Float32Array {
  const out = new Float32Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) out.set(at(x, y), (y * w + x) * 4);
  return out;
}

describe("ids map colors", () => {
  it("round trip for every index the encoding holds, with rounding slack", () => {
    for (const i of [1, 2, 31, 32, 1023, 1024, 4321, 32767]) {
      const [r, g, b] = idColor(i);
      expect(idIndexOf(r, g, b)).toBe(i);
      // A value off by up to 3 in each channel still reads back.
      expect(idIndexOf(r + 3, g - 3, b + 3)).toBe(i);
    }
    expect(idColor(0)).toEqual([0, 0, 0]);
    expect(idIndexOf(0, 0, 0)).toBe(0);
    expect(hexOf([12, 20, 255])).toBe("#0c14ff");
  });

  it("the legend counts each element's pixels", () => {
    const buf = ids(4, 2, (x) => (x < 1 ? 0 : x < 3 ? 2 : 3));
    const legend = legendFrom(countIds(buf), ["", "", "wall-a", "window-b"]);
    expect(legend).toEqual([
      { color: hexOf(idColor(2)), element_id: "wall-a", pixels: 4 },
      { color: hexOf(idColor(3)), element_id: "window-b", pixels: 2 },
    ]);
  });
});

describe("linesFrom", () => {
  it("draws where the element changes, and only there", () => {
    // A 6 x 4 wall with a 2 x 2 window inside.
    const w = 6;
    const h = 4;
    const buf = ids(w, h, (x, y) => (x >= 2 && x < 4 && y >= 1 && y < 3 ? 3 : 2));
    const lines = linesFrom(buf, null, w, h);
    const on = (x: number, y: number) => lines[y * w + x] === 0;
    expect(on(0, 0)).toBe(false);
    expect(on(1, 1) && on(2, 1) && on(4, 2)).toBe(true);
    expect(on(5, 3)).toBe(false);
  });

  it("draws a crease where one element turns a corner, and a depth jump", () => {
    const w = 4;
    const h = 1;
    const one = ids(w, h, () => 2);
    // Two faces of one wall: facing the camera, then facing right.
    const corner = nd(w, h, (x) => (x < 2 ? [0, 0, 1, 5] : [1, 0, 0, 5]));
    expect([...linesFrom(one, corner, w, h)]).toEqual([255, 0, 0, 255]);
    // One face, but the right half is much farther away.
    const step = nd(w, h, (x) => [0, 0, 1, x < 2 ? 5 : 8]);
    expect([...linesFrom(one, step, w, h)]).toEqual([255, 0, 0, 255]);
    // A gentle slope stays one surface.
    const slope = nd(w, h, (x) => [0, 0.1, 1, 5 + x * 0.05]);
    expect([...linesFrom(one, slope, w, h)]).toEqual([255, 255, 255, 255]);
  });
});

describe("depth and normals", () => {
  it("near is light, far is darker, sky is black", () => {
    const d = depthGray(nd(3, 1, (x) => [0, 0, 1, [2, 10, 0][x]]), 3, 1);
    expect(d[0]).toBe(255);
    expect(d[1]).toBe(24);
    expect(d[2]).toBe(0);
  });

  it("a surface facing the camera is the usual normal map blue", () => {
    const n = normalRgba(nd(1, 1, () => [0, 0, 1, 3]), 1, 1);
    expect([...n]).toEqual([128, 128, 255, 255]);
  });

  it("flips rows from WebGL order", () => {
    const flipped = flipRows(new Uint8Array([1, 2, 3, 4, 5, 6]), 3, 2, 1);
    expect([...flipped]).toEqual([4, 5, 6, 1, 2, 3]);
  });
});

describe("sizes", () => {
  it("keeps the image's shape at most 1024 on the long side", () => {
    expect(mapsSize(1920, 1080)).toEqual({ width: 1024, height: 576 });
    expect(mapsSize(2048, 2048)).toEqual({ width: 1024, height: 1024 });
    expect(mapsSize(800, 600)).toEqual({ width: 800, height: 600 });
  });

  it("reads a PNG's size from its header", () => {
    // IHDR of a 1920 x 1080 PNG.
    const bytes = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 7, 0x80, 0, 0, 4, 0x38, 8, 6, 0, 0, 0, 0, 0, 0, 0];
    const url = `data:image/png;base64,${btoa(String.fromCharCode(...bytes))}`;
    expect(pngSizeOfDataUrl(url)).toEqual({ width: 1920, height: 1080 });
    expect(pngSizeOfDataUrl("data:image/jpeg;base64,/9j/")).toBeNull();
  });

  it("gives a capture its size and the roof and cutaway it shows", () => {
    const bytes = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 7, 0x80, 0, 0, 4, 0x38, 8, 6, 0, 0, 0, 0, 0, 0, 0];
    const url = `data:image/png;base64,${btoa(String.fromCharCode(...bytes))}`;
    const visibility = { roof: false, cutaway: true };
    expect(captureInfo(url, visibility)).toEqual({
      kind: "capture",
      width: 1920,
      height: 1080,
      samples: 0,
      seconds: 0,
      quality: null,
      gpu: "",
      visibility,
    });
    // A size it cannot read: no info, so the record claims nothing.
    expect(captureInfo("data:image/jpeg;base64,/9j/", visibility)).toBeUndefined();
  });
});
