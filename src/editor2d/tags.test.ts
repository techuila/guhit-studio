import { describe, expect, it } from "vitest";
import type { Box, TagCandidate } from "./tags";
import { TAG_PRIORITY, boxAround, boxesOverlap, layoutTags, pointSpots, segmentSpots } from "./tags";

const tag = (id: string, priority: number, spots: { x: number; y: number }[], w = 40, h = 15): TagCandidate => ({ id, priority, w, h, spots });

function noOverlaps(boxes: readonly Box[]): boolean {
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) if (boxesOverlap(boxes[i], boxes[j])) return false;
  return true;
}

describe("tag layout", () => {
  it("keeps the first spot when it is clear", () => {
    const placed = layoutTags([tag("a", 1, [{ x: 10, y: 10 }])], []);
    expect(placed).toEqual([{ id: "a", box: { x: 10, y: 10, w: 40, h: 15 } }]);
  });

  it("moves a tag to its next spot instead of overlapping another", () => {
    const placed = layoutTags(
      [tag("a", 1, [{ x: 10, y: 10 }]), tag("b", 1, [{ x: 20, y: 12 }, { x: 20, y: 40 }])],
      [],
    );
    expect(placed.map((p) => p.id)).toEqual(["a", "b"]);
    expect(placed[1].box.y).toBe(40);
  });

  it("keeps clear of dimension text", () => {
    const dimText: Box = { x: 0, y: 0, w: 100, h: 14 };
    const placed = layoutTags([tag("a", 1, [{ x: 10, y: 5 }, { x: 10, y: 30 }])], [dimText]);
    expect(placed[0].box.y).toBe(30);
  });

  it("drops the least important tag when crowded", () => {
    const spot = [{ x: 10, y: 10 }];
    const placed = layoutTags(
      [tag("height", TAG_PRIORITY.height, spot), tag("warn", TAG_PRIORITY.fallProblem, spot), tag("riser", TAG_PRIORITY.riser, spot)],
      [],
    );
    expect(placed.map((p) => p.id)).toEqual(["warn"]);
  });

  it("stays inside the canvas", () => {
    const placed = layoutTags([tag("a", 1, [{ x: -5, y: 10 }, { x: 5, y: 10 }])], [], { w: 200, h: 100 });
    expect(placed[0].box.x).toBe(5);
  });

  it("never overlaps in a crowded cluster", () => {
    // Twelve nodes 12 px apart: every tag that survives is clear of the others.
    const cands: TagCandidate[] = [];
    for (let i = 0; i < 12; i++) cands.push(tag(`n${i}`, i % 3 === 0 ? TAG_PRIORITY.riser : TAG_PRIORITY.height, pointSpots(100 + i * 12, 100, 44, 15), 44, 15));
    const dims: Box[] = [{ x: 60, y: 60, w: 80, h: 14 }];
    const placed = layoutTags(cands, dims);
    expect(placed.length).toBeGreaterThan(0);
    expect(placed.length).toBeLessThan(12);
    expect(noOverlaps([...placed.map((p) => p.box)])).toBe(true);
    expect(placed.every((p) => !boxesOverlap(p.box, dims[0], 2))).toBe(true);
    // Risers outrank plain heights: every riser that had room is kept first.
    expect(placed[0].id).toBe("n0");
  });
});

describe("tag spots", () => {
  it("puts point tags around the point, up left first", () => {
    const s = pointSpots(100, 100, 40, 15);
    expect(s[0]).toEqual({ x: 51, y: 78 });
    expect(s).toHaveLength(4);
  });

  it("puts fall tags beside the segment, clear of the line at any angle", () => {
    for (const [a, b] of [
      [{ x: 0, y: 0 }, { x: 200, y: 0 }],
      [{ x: 0, y: 0 }, { x: 0, y: 200 }],
      [{ x: 0, y: 0 }, { x: 150, y: 150 }],
    ]) {
      for (const s of segmentSpots(a, b, 60, 15)) {
        // The segment's middle stays outside every spot's box.
        const box = { x: s.x, y: s.y, w: 60, h: 15 };
        const m = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
        const inside = m.x > box.x && m.x < box.x + box.w && m.y > box.y && m.y < box.y + box.h;
        expect(inside).toBe(false);
      }
    }
    // Right of the flow first: flowing right on screen, below the line.
    expect(segmentSpots({ x: 0, y: 0 }, { x: 200, y: 0 }, 60, 15)[0].y).toBeGreaterThan(0);
  });

  it("boxes a rotated shape", () => {
    expect(boxAround([{ x: 1, y: 5 }, { x: 4, y: 2 }])).toEqual({ x: 1, y: 2, w: 3, h: 3 });
  });
});
