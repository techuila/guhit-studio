// Sun path overlay (`useViewer().sunPath`): the day's arc with hour ticks, the
// June 21 and December 21 arcs, the sun where it is now, and a compass ring on
// the ground around the model, turned to true north. It lives in the scene, so
// captures and the shadow study show it; path traced renders leave it out.
//
// Lines are screen-space wide (LineMaterial), labels are sprites of a fixed
// screen size, nothing is tone mapped, and nothing can be picked or casts a
// shadow.

import * as THREE from "three";
import { Line2 } from "three/examples/jsm/lines/Line2.js";
import { LineGeometry } from "three/examples/jsm/lines/LineGeometry.js";
import { LineMaterial } from "three/examples/jsm/lines/LineMaterial.js";
import type { Site } from "../../contract/bindings";
import { dayArc, sunPosition, sunVector, type SunPosition } from "./sun";

const DAY_COLOR = "#e08a2e";
const SOLSTICE_COLOR = "#7b8896";
const COMPASS_COLOR = "#44566b";
const NORTH_COLOR = "#0e8a8f";
/** Labels drawn at every third hour, dots at every hour. */
const LABEL_HOURS = [6, 9, 12, 15, 18];

export interface SunPathInput {
  site: Site;
  year: number;
  month: number;
  day: number;
  minutes: number;
  northAngleDeg: number;
  /** World center of the model at ground level, meters. */
  center: THREE.Vector3;
  /** Dome radius, meters. */
  radius: number;
}

const noRaycast = () => {};

function textSprite(text: string, color: string, px = 28, bold = false): THREE.Sprite {
  const pad = 6;
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  const font = `${bold ? 700 : 600} ${px}px Inter, system-ui, -apple-system, "Segoe UI", sans-serif`;
  let w = 64;
  if (ctx) {
    ctx.font = font;
    w = Math.ceil(ctx.measureText(text).width) + pad * 2;
  }
  canvas.width = w;
  canvas.height = px + pad * 2;
  if (ctx) {
    ctx.font = font;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    // A soft halo so labels read on sky and on walls.
    ctx.lineWidth = 6;
    ctx.strokeStyle = "rgba(255,255,255,0.85)";
    ctx.strokeText(text, canvas.width / 2, canvas.height / 2);
    ctx.fillStyle = color;
    ctx.fillText(text, canvas.width / 2, canvas.height / 2);
  }
  const tex = new THREE.CanvasTexture(canvas);
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.minFilter = THREE.LinearFilter;
  tex.generateMipmaps = false;
  const mat = new THREE.SpriteMaterial({ map: tex, sizeAttenuation: false, toneMapped: false, depthWrite: false, transparent: true });
  const sprite = new THREE.Sprite(mat);
  // Screen size: sizeAttenuation off measures in viewport heights.
  const h = 0.028 * (px / 28);
  sprite.scale.set((h * canvas.width) / canvas.height, h, 1);
  sprite.renderOrder = 21;
  sprite.raycast = noRaycast;
  return sprite;
}

export class SunPathOverlay {
  readonly group = new THREE.Group();
  private materials: LineMaterial[] = [];
  private disposables: { dispose(): void }[] = [];
  private key = "";

  constructor() {
    this.group.name = "sun path";
    this.group.visible = false;
  }

  /** Rebuilds the overlay when its inputs changed. */
  update(input: SunPathInput): void {
    const key = [
      input.site.latitude_deg,
      input.site.longitude_deg,
      input.site.utc_offset_min,
      input.year,
      input.month,
      input.day,
      Math.round(input.minutes),
      input.northAngleDeg,
      input.center.toArray().map((v) => v.toFixed(2)),
      input.radius.toFixed(2),
    ].join("|");
    if (key === this.key) return;
    this.key = key;
    this.clear();
    const { site, year, northAngleDeg, center, radius } = input;
    const at = (p: SunPosition): THREE.Vector3 => {
      const v = sunVector(p, northAngleDeg);
      // Plan (x east, y north, z up) to world (x, up, -north).
      return new THREE.Vector3(v.x, v.z, -v.y).multiplyScalar(radius).add(center);
    };
    const arc = (month: number, day: number, color: string, width: number, dashed: boolean) => {
      const pts = dayArc(site, year, month, day, 6, -0.5).map((p) => at(p));
      if (pts.length < 2) return;
      this.line(pts, color, width, dashed, radius);
    };
    arc(6, 21, SOLSTICE_COLOR, 1.5, true);
    arc(12, 21, SOLSTICE_COLOR, 1.5, true);
    arc(input.month, input.day, DAY_COLOR, 2.6, false);

    // Solstice labels at their noon.
    for (const [month, day, text] of [
      [6, 21, "Jun 21"],
      [12, 21, "Dec 21"],
    ] as const) {
      const noon = sunPosition(site, year, month, day, 12 * 60);
      if (noon.altitudeDeg <= 0) continue;
      const s = textSprite(text, SOLSTICE_COLOR, 22);
      s.position.copy(at(noon)).add(new THREE.Vector3(0, radius * 0.04, 0));
      this.add(s);
    }

    // Hour dots on today's arc, labels every third hour.
    const dotGeo = new THREE.SphereGeometry(1, 12, 8);
    this.disposables.push(dotGeo);
    const dotMat = new THREE.MeshBasicMaterial({ color: DAY_COLOR, toneMapped: false, depthWrite: false });
    this.disposables.push(dotMat);
    for (let h = 5; h <= 19; h++) {
      const p = sunPosition(site, year, input.month, input.day, h * 60);
      if (p.altitudeDeg < 0) continue;
      const dot = new THREE.Mesh(dotGeo, dotMat);
      dot.scale.setScalar(radius * 0.008);
      dot.position.copy(at(p));
      dot.renderOrder = 20;
      this.add(dot);
      if (LABEL_HOURS.includes(h)) {
        const s = textSprite(String(h), DAY_COLOR, 24, true);
        s.position.copy(at(p)).add(new THREE.Vector3(0, radius * 0.05, 0));
        this.add(s);
      }
    }

    // The sun, where it is now.
    const now = sunPosition(site, year, input.month, input.day, input.minutes);
    if (now.altitudeDeg > -0.5) {
      const sunMat = new THREE.MeshBasicMaterial({ color: "#ffb347", toneMapped: false, depthWrite: false });
      this.disposables.push(sunMat);
      const sun = new THREE.Mesh(dotGeo, sunMat);
      sun.scale.setScalar(radius * 0.026);
      sun.position.copy(at(now));
      sun.renderOrder = 22;
      this.add(sun);
    }

    // Compass ring on the ground, true north marked.
    const ring: THREE.Vector3[] = [];
    for (let i = 0; i <= 96; i++) {
      const a = (i / 96) * Math.PI * 2;
      ring.push(new THREE.Vector3(Math.cos(a) * radius, 0, Math.sin(a) * radius).add(center));
    }
    this.line(ring, COMPASS_COLOR, 1.4, false, radius);
    const ticks: THREE.Vector3[] = [];
    for (let deg = 0; deg < 360; deg += 15) {
      const inner = deg % 90 === 0 ? 0.9 : 0.95;
      const a = at({ azimuthDeg: deg, altitudeDeg: 0 });
      const dir = a.clone().sub(center);
      ticks.push(center.clone().addScaledVector(dir, inner), a);
    }
    this.segments(ticks, COMPASS_COLOR, 1.2);
    const north = at({ azimuthDeg: 0, altitudeDeg: 0 });
    const nDir = north.clone().sub(center).normalize();
    const side = new THREE.Vector3(-nDir.z, 0, nDir.x).multiplyScalar(radius * 0.035);
    const tip = north.clone().addScaledVector(nDir, radius * 0.1);
    this.line([north.clone().add(side), tip, north.clone().sub(side), north.clone().add(side)], NORTH_COLOR, 2.2, false, radius);
    for (const [deg, text, color] of [
      [0, "N", NORTH_COLOR],
      [90, "E", COMPASS_COLOR],
      [180, "S", COMPASS_COLOR],
      [270, "W", COMPASS_COLOR],
    ] as const) {
      const s = textSprite(text, color, 30, true);
      const a = at({ azimuthDeg: deg, altitudeDeg: 0 });
      s.position.copy(center.clone().addScaledVector(a.clone().sub(center), deg === 0 ? 1.18 : 1.1)).add(new THREE.Vector3(0, radius * 0.02, 0));
      this.add(s);
    }
  }

  /** LineMaterial draws in pixels: it needs the drawing buffer size every frame it is on screen. */
  setResolution(width: number, height: number): void {
    for (const m of this.materials) m.resolution.set(Math.max(width, 1), Math.max(height, 1));
  }

  private add(o: THREE.Object3D): void {
    o.raycast = noRaycast;
    o.castShadow = false;
    o.receiveShadow = false;
    o.frustumCulled = false;
    this.group.add(o);
  }

  private material(color: string, width: number, dashed: boolean, radius: number): LineMaterial {
    const mat = new LineMaterial({
      color: new THREE.Color(color).getHex(),
      linewidth: width,
      transparent: true,
      opacity: dashed ? 0.8 : 0.95,
      depthWrite: false,
      dashed,
      dashSize: radius * 0.03,
      gapSize: radius * 0.02,
    });
    mat.toneMapped = false;
    this.materials.push(mat);
    this.disposables.push(mat);
    return mat;
  }

  private line(points: THREE.Vector3[], color: string, width: number, dashed: boolean, radius: number): void {
    const geo = new LineGeometry();
    geo.setPositions(points.flatMap((p) => [p.x, p.y, p.z]));
    const line = new Line2(geo, this.material(color, width, dashed, radius));
    if (dashed) line.computeLineDistances();
    line.renderOrder = 20;
    this.disposables.push(geo);
    this.add(line);
  }

  private segments(points: THREE.Vector3[], color: string, width: number): void {
    for (let i = 0; i + 1 < points.length; i += 2) this.line([points[i], points[i + 1]], color, width, false, 1);
  }

  private clear(): void {
    for (const o of [...this.group.children]) {
      const sprite = o as THREE.Sprite;
      if (sprite.isSprite) {
        sprite.material.map?.dispose();
        sprite.material.dispose();
      }
      o.removeFromParent();
    }
    for (const d of this.disposables) d.dispose();
    this.disposables = [];
    this.materials = [];
  }

  dispose(): void {
    this.clear();
    this.group.removeFromParent();
    this.key = "";
  }
}
