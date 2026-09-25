// The lights of the model's fixtures, plus two kinds of view-only room light:
// the light a room's lamps bounce off its floor and walls, and a soft ghost
// light in rooms that have no fixture. Built from the scene's lamp and room
// specs (scene/buildScene.ts), lit by the light rig: a fixture gives light
// when `Asset::light.on` and the lamps are lit (docs/CONTRACT.md, "Sun and
// light").
//
// Bounce. The live view has no global illumination, and fixtures that shine
// down leave their ceilings black. Each lit room gets one light on its floor
// facing up with a flat diffuser's spread: a share of the room's lit lumens
// coming back off the floor, as real bounce light does. It lights the ceiling
// and the upper walls and never the floor it stands on. The path tracer
// computes real bounce light and does not get these.
//
// Shader cost. Every light in the scene is a loop iteration in every lit
// fragment, and changing the number of lights recompiles every material. So
// the lights are only in the scene while a lamp can be lit (night, or lamps
// on), and inside that a fixture switched off keeps its light at intensity 0:
// walking past switches never recompiles anything. At most MAX_LIGHTS lights;
// room lights take at most a third of them, and past the rest nearby
// fixtures share one light.
//
// Shadows: the sun plus up to SHADOW_LAMPS lamps, the lit ones that matter
// most from where the camera stands (brightest over distance squared). The
// rest cast none. Lamp shadow maps are redrawn only when the model changes,
// never for a sun jitter or a turn of the camera (a point lamp's is six
// renders). The number of shadowed spots and of shadowed points is fixed for
// a lamp set (`shadowSplit`): then moving the shadows to other lamps never
// changes a shader, only which lamps the same shaders read.
//
// Turning the lamps on or off does change the shaders (the number of lights).
// The light rig compiles the other set in the background first
// (`configKey`, `proxyLights`) and switches only once it is ready, so the
// view never stalls on a compile.

import * as THREE from "three";
import { ghostLumens, type LampSpec, type RoomSpec } from "./fixtures";
import { lampCandela, lampTint, LUX } from "./model";

/** Most lights the rig puts in the scene. */
export const MAX_LIGHTS = 24;
/** Lamps that cast shadows, besides the sun. */
export const SHADOW_LAMPS = 4;
/** A ghost light is warm white. */
const GHOST_KELVIN = 3000;
/** Emissive glow of a lit diffuser, cd/m2 per lumen per square meter of diffuser (Lambertian: 1 / pi). */
const GLOW_PER_LM_M2 = 1 / Math.PI;
/** Glowing area of a fixture whose form did not say, m2. */
const GLOW_AREA_FALLBACK = 0.03;
/** Widest field a spot's shadow camera gets, degrees: past it the map's middle would be too coarse. */
const SPOT_SHADOW_FOV = 120;
/**
 * Share of a room's lamp light that comes back up as bounce light: floors,
 * furniture and walls reflect 20 to 50 percent. A room lit by one 900 lm
 * ceiling light gets about 10 lux on its ceiling.
 */
export const BOUNCE_SHARE = 0.35;
/** A ghost light hangs this far under its room's ceiling, meters. */
const GHOST_DROP = 0.1;
/** A bounce light stands this far above its room's floor, meters. */
const BOUNCE_LIFT = 0.05;

type Role = "fixture" | "ghost" | "bounce";

interface LampLight {
  role: Role;
  /** Fixtures: the ids this light stands for (several when merged). Bounce: the room's fixtures. */
  ids: string[];
  kind: "point" | "spot";
  light: THREE.PointLight | THREE.SpotLight;
  /** Fixtures: candela at full on per id, before LUX and exposure. Ghost: one value. */
  candela: number[];
  /** Fixtures: per id, on in the model. */
  modelOn: boolean[];
  position: THREE.Vector3;
  lumens: number;
  roomId: string | null;
  /** Bounce: the room's ghost light, whose light it bounces too. */
  ghost: LampLight | null;
}

export interface LampState {
  /** Fixtures give light (night on auto, or lamps on). */
  lampsLit: boolean;
  /** Ghost lights are on. */
  ghostLit: boolean;
  /** Pre-exposure scale (light/model.ts). */
  scale: number;
  /** View only switch states from walk mode, by fixture id. */
  overrides: ReadonlyMap<string, boolean>;
  /** Diffuser glow multiplier: 1 in the live view, lower in the path tracer so glows do not light the room twice. */
  glowGain?: number;
}

function makeLight(
  kind: "point" | "spot",
  color: [number, number, number],
  coneDeg: number,
  penumbra: number,
  radius: number,
): THREE.PointLight | THREE.SpotLight {
  const c = new THREE.Color(color[0], color[1], color[2]);
  if (kind === "spot") {
    const cone = Math.min(Math.max(coneDeg, 10), 180);
    const s = new THREE.SpotLight(c, 0, 0, (cone / 2) * (Math.PI / 180), Math.min(Math.max(penumbra, 0), 1), 2);
    // Read by three-gpu-pathtracer: the emitter size softens its shadows.
    (s as THREE.SpotLight & { radius?: number }).radius = radius;
    // One render per map, so a finer map than the points'. A cone wider than
    // the shadow camera's field leaves its outer rim unshadowed, where the
    // spot has faded to a fraction anyway.
    s.shadow.mapSize.set(1024, 1024);
    s.shadow.focus = Math.min(1, SPOT_SHADOW_FOV / cone);
    s.shadow.bias = -0.0015;
    s.shadow.normalBias = 0.02;
    s.shadow.radius = 3;
    s.shadow.camera.near = 0.05;
    s.shadow.camera.far = 12;
    return s;
  }
  const p = new THREE.PointLight(c, 0, 0, 2);
  p.shadow.mapSize.set(512, 512);
  p.shadow.bias = -0.003;
  p.shadow.normalBias = 0.02;
  p.shadow.radius = 3;
  p.shadow.camera.near = 0.05;
  p.shadow.camera.far = 14;
  return p;
}

/** A spot facing straight up or down with a flat diffuser's spread: pi steradians. */
function diffuser(color: [number, number, number], at: THREE.Vector3, up: boolean): THREE.SpotLight {
  const s = makeLight("spot", color, 180, 1, 0) as THREE.SpotLight;
  s.position.copy(at);
  s.target.position.copy(at).add(new THREE.Vector3(0, up ? 1 : -1, 0));
  return s;
}

/**
 * Which rooms get a bounce light and which a ghost light, within a third of
 * the light budget: rooms with fixtures first (one light each, largest
 * first), then rooms without (a ghost and its bounce, largest first).
 */
export function planRooms(specs: readonly LampSpec[], rooms: readonly RoomSpec[], max = MAX_LIGHTS): { bounce: RoomSpec[]; ghosts: RoomSpec[] } {
  const withFixtures = new Set(specs.map((s) => s.roomId).filter((id): id is string => id !== null));
  const byArea = (a: RoomSpec, b: RoomSpec) => b.areaM2 - a.areaM2;
  let left = Math.floor(max / 3);
  const bounce: RoomSpec[] = [];
  const ghosts: RoomSpec[] = [];
  for (const r of rooms.filter((r) => withFixtures.has(r.roomId)).sort(byArea)) {
    if (left < 1) break;
    bounce.push(r);
    left--;
  }
  for (const r of rooms.filter((r) => !withFixtures.has(r.roomId)).sort(byArea)) {
    if (left < 1) break;
    ghosts.push(r);
    left--;
    if (left >= 1) {
      bounce.push(r);
      left--;
    }
  }
  return { bounce, ghosts };
}

/** Merges fixtures beyond `room` lights into shared lights, nearest first. */
function cluster(specs: LampSpec[], room: number): LampSpec[][] {
  if (specs.length <= room) return specs.map((s) => [s]);
  // Grow a 2.5 m grid until the clusters fit.
  for (let cell = 2.5; ; cell *= 1.5) {
    const groups = new Map<string, LampSpec[]>();
    for (const s of specs) {
      const key = [s.levelId, s.kind, Math.floor(s.position[0] / cell), Math.floor(s.position[2] / cell)].join("|");
      const list = groups.get(key) ?? [];
      list.push(s);
      groups.set(key, list);
    }
    if (groups.size <= room || cell > 200) return [...groups.values()].slice(0, Math.max(room, 0));
  }
}

export class LampRig {
  readonly group = new THREE.Group();
  private lights: LampLight[] = [];
  private glows = new Map<string, { mat: THREE.MeshStandardMaterial; lumens: number; areaM2: number }>();
  /** Every fixture's `Asset::light.on` and lumens, including fixtures whose light was merged away. */
  private fixtures = new Map<string, { on: boolean; lumens: number }>();
  private shadowIds = new Set<THREE.PointLight | THREE.SpotLight>();
  /** Shadowed points and spots for this lamp set, fixed until the next build. */
  private split = { points: 0, spots: 0 };
  private assignedAt = new THREE.Vector3(Infinity, Infinity, Infinity);
  private key = "";

  constructor() {
    this.group.name = "lamps";
    this.group.visible = false;
  }

  /**
   * Rebuilds the lights for a new scene. Cheap: lights are plain objects. A
   * scene with the same lamps keeps its lights, so a wall edit never
   * recompiles a shader.
   */
  build(specs: LampSpec[], rooms: RoomSpec[], glowMats: Map<string, THREE.MeshStandardMaterial>): void {
    this.glows.clear();
    this.fixtures.clear();
    for (const s of specs) {
      this.fixtures.set(s.id, { on: s.on, lumens: s.lumens });
      const mat = glowMats.get(s.id);
      if (mat) this.glows.set(s.id, { mat, lumens: s.lumens, areaM2: s.glowAreaM2 ?? GLOW_AREA_FALLBACK });
    }
    const key = JSON.stringify([
      specs.map((s) => [s.id, s.kind, s.position.map((v) => v.toFixed(3)), s.direction, s.coneDeg, s.penumbra, s.lumens, s.kelvin, s.on, s.roomId]),
      rooms.map((r) => [r.roomId, r.floor.map((v) => v.toFixed(3)), r.heightM.toFixed(3), r.areaM2.toFixed(1)]),
    ]);
    if (key === this.key) return;
    this.key = key;
    this.clear();
    const plan = planRooms(specs, rooms, MAX_LIGHTS);
    for (const group of cluster(specs, MAX_LIGHTS - plan.bounce.length - plan.ghosts.length)) {
      const first = group[0];
      const lumens = group.reduce((a, s) => a + s.lumens, 0);
      const pos = new THREE.Vector3();
      for (const s of group) pos.add(new THREE.Vector3(...s.position).multiplyScalar(s.lumens / Math.max(lumens, 1)));
      const kelvin = group.reduce((a, s) => a + s.kelvin * s.lumens, 0) / Math.max(lumens, 1);
      const light = makeLight(first.kind, lampTint(kelvin), first.coneDeg, first.penumbra, first.radius);
      light.position.copy(pos);
      if (light instanceof THREE.SpotLight) {
        const dir = new THREE.Vector3(...(first.direction ?? [0, -1, 0]));
        light.target.position.copy(pos).add(dir);
      }
      light.name = `lamp:${group.map((s) => s.id).join(",")}`;
      this.add({
        role: "fixture",
        ids: group.map((s) => s.id),
        kind: first.kind,
        light,
        candela: group.map((s) => lampCandela(s.lumens, first.kind, first.coneDeg, first.penumbra)),
        modelOn: group.map((s) => s.on),
        position: pos,
        lumens,
        roomId: first.roomId,
        ghost: null,
      });
    }
    const ghostOf = new Map<string, LampLight>();
    for (const r of plan.ghosts) {
      // A virtual ceiling light: lights the floor and walls, no hot spot on the ceiling.
      const at = new THREE.Vector3(r.floor[0], r.floor[1] + Math.max(r.heightM - GHOST_DROP, 0.5), r.floor[2]);
      const light = diffuser(lampTint(GHOST_KELVIN), at, false);
      light.name = `ghost:${r.roomId}`;
      const lm = ghostLumens(r.areaM2);
      const l: LampLight = { role: "ghost", ids: [], kind: "spot", light, candela: [lampCandela(lm, "spot", 180, 1)], modelOn: [true], position: at, lumens: lm, roomId: r.roomId, ghost: null };
      this.add(l);
      ghostOf.set(r.roomId, l);
    }
    for (const r of plan.bounce) {
      const own = specs.filter((s) => s.roomId === r.roomId);
      const lm = own.reduce((a, s) => a + s.lumens, 0);
      const kelvin = own.length ? own.reduce((a, s) => a + s.kelvin * s.lumens, 0) / Math.max(lm, 1) : GHOST_KELVIN;
      const at = new THREE.Vector3(r.floor[0], r.floor[1] + BOUNCE_LIFT, r.floor[2]);
      const light = diffuser(lampTint(kelvin), at, true);
      light.name = `bounce:${r.roomId}`;
      this.add({ role: "bounce", ids: own.map((s) => s.id), kind: "spot", light, candela: [], modelOn: [], position: at, lumens: lm, roomId: r.roomId, ghost: ghostOf.get(r.roomId) ?? null });
    }
    this.split = shadowSplit(this.shadowCandidates("point").length, this.shadowCandidates("spot").length);
    this.shadowIds.clear();
    this.assignedAt.set(Infinity, Infinity, Infinity);
  }

  private add(l: LampLight): void {
    l.light.castShadow = false;
    this.group.add(l.light);
    if (l.light instanceof THREE.SpotLight) this.group.add(l.light.target);
    this.lights.push(l);
  }

  /** Number of lights in the rig, for the dev readout. */
  get count(): number {
    return this.lights.length;
  }

  /**
   * Whether a fixture is lit: a switch flipped in walk mode wins, else it
   * follows the model's `on` while the lamps are lit.
   */
  isOn(id: string, state: Pick<LampState, "lampsLit" | "overrides">): boolean {
    return state.overrides.get(id) ?? (state.lampsLit && (this.fixtures.get(id)?.on ?? true));
  }

  /** True when this fixture has a light in the rig. */
  has(id: string): boolean {
    return this.fixtures.has(id);
  }

  /** True when a light state needs the lamps in the scene: any fixture or ghost light lit. */
  wantsVisible(state: LampState): boolean {
    if (this.lights.length === 0) return false;
    if (state.lampsLit || state.ghostLit) return true;
    for (const on of state.overrides.values()) if (on) return true;
    return false;
  }

  get visible(): boolean {
    return this.group.visible;
  }

  /** Puts the lamps in the scene or takes them out. The rig calls it once the shaders are ready. */
  setVisible(on: boolean): void {
    this.group.visible = on;
  }

  /** Fixture lights of one kind: the ones that may cast a shadow. */
  private shadowCandidates(kind: "point" | "spot"): LampLight[] {
    return this.lights.filter((l) => l.role === "fixture" && l.kind === kind);
  }

  /**
   * What the shaders depend on for this lamp set: the number of point and
   * spot lights and of shadowed ones, with the lamps in the scene or out of
   * it. Two states with the same key share every shader.
   */
  configKey(visible: boolean): string {
    if (!visible || this.lights.length === 0) return "off";
    const points = this.lights.filter((l) => l.kind === "point").length;
    const spots = this.lights.length - points;
    return `p${points}s${spots}ps${this.split.points}ss${this.split.spots}`;
  }

  /** Copies of the lamps as `configKey(true)` has them, for compiling that set ahead. */
  proxyLights(): THREE.Light[] {
    const left = { ...this.split };
    return this.lights.map((l) => {
      const c = l.light.clone();
      const kind = l.kind === "point" ? "points" : "spots";
      c.castShadow = l.role === "fixture" && left[kind] > 0;
      if (c.castShadow) left[kind]--;
      return c;
    });
  }

  /** Sets every intensity and glow for a light state. The rig decides whether the lamps are in the scene. */
  apply(state: LampState): void {
    const k = LUX * state.scale;
    for (const l of this.lights) {
      let cd = 0;
      if (l.role === "fixture") {
        l.ids.forEach((id, i) => {
          if (state.overrides.get(id) ?? (state.lampsLit && l.modelOn[i])) cd += l.candela[i];
        });
      } else if (l.role === "ghost") {
        cd = state.ghostLit ? l.candela[0] : 0;
      } else {
        let lm = l.ghost && state.ghostLit ? l.ghost.lumens : 0;
        for (const id of l.ids) if (this.isOn(id, state)) lm += this.fixtures.get(id)?.lumens ?? 0;
        cd = (lm * BOUNCE_SHARE) / Math.PI;
      }
      l.light.intensity = cd * k;
    }
    const gain = state.glowGain ?? 1;
    for (const [id, g] of this.glows) {
      // Luminance of the diffuser: flux over pi times its area.
      const cdm2 = this.isOn(id, state) ? (g.lumens * GLOW_PER_LM_M2) / Math.max(g.areaM2, 0.004) : 0;
      g.mat.emissiveIntensity = cdm2 * k * gain;
    }
  }

  /**
   * Picks the lamps that cast shadows: of each kind, the ones that matter
   * most from the camera, lit ones first (brightest over distance squared).
   * Always the same number of each kind (`shadowSplit`), so shaders never
   * change. Only reconsiders once the camera has moved a couple of meters.
   * Returns true when the set changed (their maps are drawn on the next
   * render).
   */
  assignShadows(camera: THREE.Camera, force = false): boolean {
    const eye = camera.getWorldPosition(new THREE.Vector3());
    if (!force && eye.distanceTo(this.assignedAt) < 2.5) return false;
    this.assignedAt.copy(eye);
    const best = (kind: "point" | "spot", n: number) =>
      this.shadowCandidates(kind)
        .map((l) => ({ l, score: (l.light.intensity > 0 ? 1e6 : 0) + l.lumens / Math.max(eye.distanceToSquared(l.position), 0.25) }))
        .sort((a, b) => b.score - a.score)
        .slice(0, n);
    const scored = [...best("point", this.split.points), ...best("spot", this.split.spots)];
    const next = new Set(scored.map((s) => s.l.light));
    let changed = next.size !== this.shadowIds.size;
    for (const l of next) if (!this.shadowIds.has(l)) changed = true;
    if (!changed) return false;
    for (const l of this.lights) {
      const on = next.has(l.light);
      if (l.light.castShadow !== on) {
        l.light.castShadow = on;
        // The map is drawn once now and again only when the model changes.
        l.light.shadow.autoUpdate = false;
        l.light.shadow.needsUpdate = on;
      }
    }
    this.shadowIds = next;
    return true;
  }

  /** The model changed: every lamp shadow map is drawn again on the next frame. */
  shadowsDirty(): void {
    for (const l of this.shadowIds) l.shadow.needsUpdate = true;
  }

  /** True while any lamp casts a shadow. */
  get shadowing(): boolean {
    return this.shadowIds.size > 0;
  }

  /**
   * Copies of the lights for another scene (the path tracer), at their
   * current intensities. Not the bounce lights: the tracer bounces the light
   * itself.
   */
  cloneLights(): THREE.Light[] {
    const out: THREE.Light[] = [];
    for (const l of this.lights) {
      if (l.role === "bounce" || l.light.intensity <= 0) continue;
      const c = l.light.clone();
      if (c instanceof THREE.SpotLight && l.light instanceof THREE.SpotLight) {
        c.target = new THREE.Object3D();
        c.target.position.copy(l.light.target.position);
        (c as THREE.SpotLight & { radius?: number }).radius = (l.light as THREE.SpotLight & { radius?: number }).radius ?? 0;
      }
      c.castShadow = false;
      out.push(c);
    }
    return out;
  }

  /** The lights by role, for tests and the dev readout. */
  roles(): { fixture: number; ghost: number; bounce: number } {
    const n = { fixture: 0, ghost: 0, bounce: 0 };
    for (const l of this.lights) n[l.role]++;
    return n;
  }

  /** Intensity of the light of a role in a room, for tests. */
  roomIntensity(roomId: string, role: "ghost" | "bounce"): number {
    return this.lights.find((l) => l.role === role && l.roomId === roomId)?.light.intensity ?? 0;
  }

  private clear(): void {
    for (const l of this.lights) {
      l.light.dispose();
      l.light.removeFromParent();
      if (l.light instanceof THREE.SpotLight) l.light.target.removeFromParent();
    }
    this.lights = [];
    this.shadowIds.clear();
  }

  dispose(): void {
    this.clear();
    this.glows.clear();
    this.group.removeFromParent();
  }
}

/**
 * How many shadowed lamps of each kind a lamp set gets: SHADOW_LAMPS in all,
 * mostly spots (one render per map, and the ceiling fixtures that shade the
 * furniture), and at least one point when there is one (a lamp or a wall
 * light near the camera).
 */
export function shadowSplit(points: number, spots: number): { points: number; spots: number } {
  const p = points > 0 ? Math.min(points, Math.max(1, SHADOW_LAMPS - spots)) : 0;
  return { points: p, spots: Math.min(spots, SHADOW_LAMPS - p) };
}
