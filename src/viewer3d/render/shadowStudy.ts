// Shadow study (`bus.emit("shadow_study")`): frames of the live 3D view with
// the sun at every step from morning to evening, on one or more dates, laid
// out on a contact sheet with the time, date, place and a north arrow on each
// frame (docs/CONTRACT.md, "Render"). Raster frames from the live view, no
// path tracer: a study is about where the shadows fall.
//
// Each frame is drawn by `ViewerEngine.captureCanvas` with the study's light
// put on the scene for that one frame (`LightRig.withLight`), so the live
// light is back the moment the study ends. The exposure offset is locked for
// the whole sheet.

import * as THREE from "three";
import type { Camera, DocState, Site } from "../../contract/bindings";
import { siteLabel } from "../../shell/site";
import { cameraToPose } from "../geom/coords";
import { MANILA, clockLabel, dateLabel } from "../light/sun";
import { useViewer, type LiveLight } from "../viewerStore";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { toViewLight } from "../light/model";

export interface StudyOptions {
  /** "current", "top", or a saved view's id. */
  view: string;
  dates: Array<[number, number]>;
  fromMin: number;
  toMin: number;
  stepMin: 30 | 60;
}

export interface StudyResult {
  png: string;
  camera: Camera;
  frames: number;
  width: number;
  height: number;
}

/** Frame size on the sheet, and the size it is drawn at (twice, for clean edges). */
const CELL_W = 480;
const CELL_H = 300;
const DRAW_SCALE = 2;
const COLS = 5;
const GAP = 16;
const MARGIN = 32;
const LABEL_H = 46;
const HEADER_H = 104;
const DATE_H = 40;

const INK = "#14283f";
const INK2 = "#44566b";
const INK3 = "#7b8896";
const ACCENT = "#0e8a8f";
const PAPER = "#ffffff";

export function studyTimes(opts: Pick<StudyOptions, "fromMin" | "toMin" | "stepMin">): number[] {
  const out: number[] = [];
  for (let m = opts.fromMin; m <= opts.toMin + 0.5; m += opts.stepMin) out.push(m);
  return out;
}

export function studyFrameCount(opts: StudyOptions): number {
  return studyTimes(opts).length * opts.dates.length;
}

function studyCamera(engine: ViewerEngine, doc: DocState, view: string, current: Camera): Camera {
  if (view === "current") return current;
  if (view === "top") {
    const pose = engine.presetPose("top", []);
    if (pose) return { ...current, id: "", name: "Top", preset: "top", position: pose.position, target: pose.target, fov_deg: pose.fov_deg };
    return current;
  }
  const el = doc.project.elements.find((e) => e.id === view);
  return el && el.kind === "camera" ? { ...el } : current;
}

/** Screen angle of true north in a frame, radians, 0 pointing up the frame, clockwise. */
function northAngle(engine: ViewerEngine, camera: Camera, northDeg: number): number {
  const pose = cameraToPose(camera);
  const cam = new THREE.PerspectiveCamera(pose.fovDeg, CELL_W / CELL_H, 0.05, 3000);
  cam.position.set(...pose.position);
  cam.lookAt(new THREE.Vector3(...pose.target));
  cam.updateMatrixWorld();
  const center = new THREE.Vector3(...pose.target);
  const n = (northDeg * Math.PI) / 180;
  // True north in plan is +y turned counter-clockwise by the north angle; in world, -z.
  const north = new THREE.Vector3(-Math.sin(n), 0, -Math.cos(n));
  const a = center.clone().project(cam);
  const b = center.clone().addScaledVector(north, Math.max(engine.bounds().maxX - engine.bounds().minX, 4000) / 4000).project(cam);
  const dx = b.x - a.x;
  const dy = (b.y - a.y) * (CELL_H / CELL_W);
  if (Math.hypot(dx, dy) < 1e-4) return 0;
  return Math.atan2(dx, dy);
}

function northArrow(ctx: CanvasRenderingContext2D, x: number, y: number, angle: number, size: number): void {
  ctx.save();
  ctx.translate(x, y);
  ctx.beginPath();
  ctx.arc(0, 0, size, 0, Math.PI * 2);
  ctx.fillStyle = "rgba(255,255,255,0.9)";
  ctx.fill();
  ctx.strokeStyle = INK3;
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.rotate(angle);
  ctx.beginPath();
  ctx.moveTo(0, -size * 0.78);
  ctx.lineTo(size * 0.38, size * 0.5);
  ctx.lineTo(0, size * 0.22);
  ctx.lineTo(-size * 0.38, size * 0.5);
  ctx.closePath();
  ctx.fillStyle = ACCENT;
  ctx.fill();
  ctx.rotate(-angle);
  ctx.fillStyle = INK;
  ctx.font = `700 ${Math.round(size * 0.62)}px Inter, system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  const lx = Math.sin(angle) * size * 1.55;
  const ly = -Math.cos(angle) * size * 1.55;
  ctx.fillText("N", lx, ly);
  ctx.restore();
}

/**
 * Runs the study on the live view. Resolves null when cancelled. Progress is
 * reported after every frame; a macrotask between frames keeps the app
 * responsive.
 */
export async function runShadowStudy(
  engine: ViewerEngine,
  doc: DocState,
  opts: StudyOptions,
  onProgress: (done: number, total: number) => void,
  cancelled: () => boolean,
): Promise<StudyResult | null> {
  const site: Site = doc.project.settings?.site ?? MANILA;
  const northDeg = Number.isFinite(doc.project.settings?.north_angle_deg) ? doc.project.settings.north_angle_deg : 0;
  const times = studyTimes(opts);
  const total = times.length * opts.dates.length;
  const rows = Math.ceil(times.length / COLS);
  const width = MARGIN * 2 + COLS * CELL_W + (COLS - 1) * GAP;
  const height = MARGIN * 2 + HEADER_H + opts.dates.length * (DATE_H + rows * (CELL_H + LABEL_H + GAP));
  const sheet = document.createElement("canvas");
  sheet.width = width;
  sheet.height = height;
  const ctx = sheet.getContext("2d");
  if (!ctx) throw new Error("This computer cannot draw the contact sheet.");

  const rig = engine.lightRig();
  const live: LiveLight = useViewer.getState().light;
  const lockedEv = rig.lockedEv();
  const before = engine.currentCamera("View");
  const camera = studyCamera(engine, doc, opts.view, before);
  const moved = opts.view !== "current";
  if (moved) {
    engine.flyToCamera(camera, 0);
    // The jump lands the position and target; aim the camera now, not next frame.
    engine.controls.update();
  }
  const arrow = northAngle(engine, camera, northDeg);

  ctx.fillStyle = PAPER;
  ctx.fillRect(0, 0, width, height);
  // Header: what, where, when, and what it is not.
  ctx.fillStyle = INK;
  ctx.textBaseline = "alphabetic";
  ctx.font = "700 30px Inter, system-ui, sans-serif";
  ctx.fillText(`Shadow study, ${doc.project.name}`, MARGIN, MARGIN + 30);
  ctx.font = "500 17px Inter, system-ui, sans-serif";
  ctx.fillStyle = INK2;
  const lat = `${Math.abs(site.latitude_deg).toFixed(2)} ${site.latitude_deg >= 0 ? "N" : "S"}`;
  const lng = `${Math.abs(site.longitude_deg).toFixed(2)} ${site.longitude_deg >= 0 ? "E" : "W"}`;
  ctx.fillText(
    `${siteLabel(site)}, ${lat}, ${lng}. Local time. ${camera.name || "View"}. ${clockLabel(opts.fromMin)} to ${clockLabel(opts.toMin)}, every ${opts.stepMin} minutes.`,
    MARGIN,
    MARGIN + 60,
  );
  ctx.fillStyle = INK3;
  ctx.font = "500 14px Inter, system-ui, sans-serif";
  ctx.fillText("Sun positions from NOAA's solar equations; shadows from the 3D model. A study aid for design review, not an energy or code assessment.", MARGIN, MARGIN + 84);

  let done = 0;
  try {
    for (let di = 0; di < opts.dates.length; di++) {
      const [month, day] = opts.dates[di];
      const top = MARGIN + HEADER_H + di * (DATE_H + rows * (CELL_H + LABEL_H + GAP));
      ctx.fillStyle = INK;
      ctx.font = "700 20px Inter, system-ui, sans-serif";
      ctx.fillText(dateLabel(month, day), MARGIN, top + 26);
      for (let ti = 0; ti < times.length; ti++) {
        if (cancelled()) return null;
        const minutes = times[ti];
        const x = MARGIN + (ti % COLS) * (CELL_W + GAP);
        const y = top + DATE_H + Math.floor(ti / COLS) * (CELL_H + LABEL_H + GAP);
        const light: LiveLight = { ...live, month, day, minutes, lamps: "auto", exposureEv: lockedEv };
        rig.withLight(light, () =>
          engine.captureCanvas(CELL_W * DRAW_SCALE, CELL_H * DRAW_SCALE, (canvas) => {
            ctx.drawImage(canvas, x, y, CELL_W, CELL_H);
          }),
        );
        ctx.strokeStyle = "#d9d5cb";
        ctx.lineWidth = 1;
        ctx.strokeRect(x + 0.5, y + 0.5, CELL_W - 1, CELL_H - 1);
        northArrow(ctx, x + CELL_W - 26, y + 26, arrow, 15);
        // The label under the frame: time, date, place.
        ctx.fillStyle = INK;
        ctx.font = "700 17px Inter, system-ui, sans-serif";
        ctx.fillText(clockLabel(minutes), x, y + CELL_H + 22);
        ctx.fillStyle = INK2;
        ctx.font = "500 13px Inter, system-ui, sans-serif";
        ctx.fillText(`${dateLabel(month, day)}, ${siteLabel(site)}`, x, y + CELL_H + 40);
        done++;
        onProgress(done, total);
        await new Promise((r) => setTimeout(r, 0));
      }
    }
  } finally {
    if (moved) {
      engine.flyToCamera(before, 0);
      engine.controls.update();
    }
  }

  const first = opts.dates[0] ?? [live.month, live.day];
  const record: Camera = {
    ...camera,
    name: `Shadow study, ${camera.name || "View"}`,
    light: toViewLight({ ...live, month: first[0], day: first[1], minutes: opts.fromMin, lamps: "auto", exposureEv: lockedEv }, lockedEv),
  };
  return { png: sheet.toDataURL("image/png"), camera: record, frames: done, width, height };
}
