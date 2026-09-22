// Plan symbols for the asset catalog (crates/guhit-model/src/defaults.rs).
// Every symbol draws in local millimeters: origin at the footprint center,
// +x right, +y toward the BACK of the object (bed head, sofa back, WC tank).
// The caller sets the transform, stroke and fill styles. `px` is the size of
// one screen pixel in mm, used for line widths and minimum detail sizes.

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

/**
 * Draws the symbol for `key`. Returns a short label to print on top (may be
 * empty), or null when the key is unknown and the caller should draw the
 * labelled box fallback.
 */
export function drawAssetSymbol(ctx: Ctx, key: string, w: number, d: number, px: number): string | null {
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
