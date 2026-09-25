// Label placement for the fall and height tags of service runs. A simple
// greedy pass in screen space: the most important tag goes first, each tag
// takes the first of its spots that is clear of dimension text, room labels
// and the tags already placed, and a tag with no clear spot is dropped.
// Pure, tested in tags.test.ts.

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface TagCandidate {
  /** Stable per tag, so its fade survives a relayout. */
  id: string;
  priority: number;
  w: number;
  h: number;
  /** Top left corners to try, best first. */
  spots: readonly { x: number; y: number }[];
}

export interface PlacedTag {
  id: string;
  box: Box;
}

/** Importance of each tag. Higher places first and is dropped last. */
export const TAG_PRIORITY = {
  /** A fall below the default, flat or uphill: the tag that warns. */
  fallProblem: 100,
  /** "+300 to +1200" at a riser. */
  riser: 80,
  /** Height at the first and last point of a run. */
  endHeight: 60,
  /** A fall at or above the default. */
  fall: 50,
  /** Height at a bend. */
  height: 40,
} as const;

/** Space kept between a tag and anything else, in pixels. */
export const TAG_GAP = 2;

export function boxesOverlap(a: Box, b: Box, gap = 0): boolean {
  return a.x < b.x + b.w + gap && b.x < a.x + a.w + gap && a.y < b.y + b.h + gap && b.y < a.y + a.h + gap;
}

/** The axis aligned box around a set of screen points. */
export function boxAround(points: readonly { x: number; y: number }[]): Box {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of points) {
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x);
    maxY = Math.max(maxY, p.y);
  }
  return { x: minX, y: minY, w: Math.max(0, maxX - minX), h: Math.max(0, maxY - minY) };
}

/**
 * Places tags greedily by priority (ties keep their input order). A spot
 * must stay inside `bounds` when given. Returns the placed tags in placing
 * order; the ones missing were dropped.
 */
export function layoutTags(cands: readonly TagCandidate[], obstacles: readonly Box[], bounds?: { w: number; h: number }): PlacedTag[] {
  const order = cands.map((c, i) => ({ c, i })).sort((a, b) => b.c.priority - a.c.priority || a.i - b.i);
  const placed: PlacedTag[] = [];
  for (const { c } of order) {
    for (const s of c.spots) {
      const box: Box = { x: s.x, y: s.y, w: c.w, h: c.h };
      if (bounds && (box.x < 0 || box.y < 0 || box.x + box.w > bounds.w || box.y + box.h > bounds.h)) continue;
      if (obstacles.some((o) => boxesOverlap(box, o, TAG_GAP))) continue;
      if (placed.some((p) => boxesOverlap(box, p.box, TAG_GAP))) continue;
      placed.push({ id: c.id, box });
      break;
    }
  }
  return placed;
}

/**
 * Spots around a point for a height tag: up left first (snap labels take the
 * right, cursor readouts the lower right), then down left, up right, down right.
 */
export function pointSpots(x: number, y: number, w: number, h: number): { x: number; y: number }[] {
  return [
    { x: x - 9 - w, y: y - 7 - h },
    { x: x - 9 - w, y: y + 7 },
    { x: x + 9, y: y - 7 - h },
    { x: x + 9, y: y + 7 },
  ];
}

/**
 * Spots beside a segment a-b for a fall tag, centered on it: the middle on
 * the right of the direction of flow first, then the left, then a quarter
 * along each way on both sides.
 */
export function segmentSpots(a: { x: number; y: number }, b: { x: number; y: number }, w: number, h: number, off = 14): { x: number; y: number }[] {
  const L = Math.hypot(b.x - a.x, b.y - a.y);
  if (L < 1e-9) return [];
  const nx = -(b.y - a.y) / L;
  const ny = (b.x - a.x) / L;
  // Far enough out that the whole box clears the line, whatever the angle.
  const D = Math.max(off, Math.abs(nx) * (w / 2) + Math.abs(ny) * (h / 2) + 5);
  const at = (t: number, side: number): { x: number; y: number } => {
    const cx = a.x + (b.x - a.x) * t + nx * side * D;
    const cy = a.y + (b.y - a.y) * t + ny * side * D;
    return { x: cx - w / 2, y: cy - h / 2 };
  };
  return [at(0.5, 1), at(0.5, -1), at(0.3, 1), at(0.7, 1), at(0.3, -1), at(0.7, -1)];
}
