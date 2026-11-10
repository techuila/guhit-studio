// The swarm draft in the 3D view (DECISIONS D56, D57). Its own group, beside
// the model and outside the scene key, so a new draft never rebuilds the
// model. Each draft element is one small form in its part's color at 55%,
// built from the draft view; a wall carries the draft's openings, so a draft
// door shows in a model wall. Only elements whose form changed are rebuilt.
//
// The engine owns it (ViewerEngine.setDraftOverlay), rebuilds it at most 4
// times a second, shows each element once the reveal reaches it with the
// usual rise and fade, and hides the model's meshes of what the draft changes.

import * as THREE from "three";
import type { DocState, Element, Project } from "../contract/bindings";
import { MeshData } from "../viewer3d/geom/meshData";
import { buildWallMesh } from "../viewer3d/geom/wallMesh";
import { buildGhosts, PLINTH_MM } from "../viewer3d/scene/buildScene";
import { Kit } from "../viewer3d/scene/kit";
import { MaterialLibrary } from "../viewer3d/scene/materials";

type Wall = Extract<Element, { kind: "wall" }>;
type OpeningEl = Extract<Element, { kind: "opening" }>;

/** The draft group rebuilds at most 4 times a second. */
export const DRAFT_MIN_MS = 250;

/**
 * Runs `run` at most once per `minMs`: a request inside the window waits for
 * its end, and requests meanwhile share that one run.
 */
export function rateLimit(minMs: number, run: () => void, clock: () => number = () => performance.now()): { request: () => void; cancel: () => void } {
  let last = Number.NEGATIVE_INFINITY;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const fire = () => {
    timer = null;
    last = clock();
    run();
  };
  return {
    request: () => {
      if (timer !== null) return;
      const wait = minMs - (clock() - last);
      if (wait <= 0) fire();
      else timer = setTimeout(fire, wait);
    },
    cancel: () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}

/** The draft's strength in 3D. */
export const DRAFT_OPACITY = 0.55;

/** Model walls whose look changes with the draft (their new openings): a quiet grey. */
export const CONTEXT_COLOR = "#8a96a3";

/** What the 3D view draws of a draft. */
export interface DraftOverlay {
  /** Changes when the draft does (its revision). */
  key: string;
  /** The draft view. */
  state: DocState;
  /** Per part, its color and the draft elements it added or changed. */
  parts: Array<{ color: string; ids: string[] }>;
  /** Model walls whose look changes with the draft although no step changed them (`display_modified`). */
  context: string[];
  /** Model elements whose own meshes hide while their draft version shows. */
  hide: string[];
}

interface Built {
  group: THREE.Group;
  kit: Kit;
  sig: string;
  shown: boolean;
}

export class DraftGroup {
  readonly root = new THREE.Group();
  private items = new Map<string, Built>();
  /** Its own library: the model's rebuilds dispose materials they did not ask for. */
  private lib = new MaterialLibrary();
  private mats = new Map<string, THREE.MeshStandardMaterial>();

  constructor() {
    this.root.name = "swarm-draft";
  }

  /** Builds what is new or changed, drops what left. New forms start hidden. True when anything changed. */
  sync(o: DraftOverlay): boolean {
    let changed = false;
    const project = o.state.project;
    const byId = new Map(project.elements.map((e) => [e.id, e]));
    const want = new Map<string, string>();
    for (const p of o.parts) for (const id of p.ids) want.set(id, p.color);
    for (const id of o.context) if (!want.has(id)) want.set(id, CONTEXT_COLOR);
    for (const id of [...this.items.keys()]) {
      if (want.has(id) && byId.has(id)) continue;
      this.drop(id);
      changed = true;
    }
    const outlines = new Map((o.state.derived.walls ?? []).map((g) => [g.wall_id, g.outline]));
    this.lib.begin(project.materials ?? []);
    for (const [id, color] of want) {
      const el = byId.get(id);
      if (!el) continue;
      const sig = `${color}|${signature(el, project)}`;
      const prev = this.items.get(id);
      if (prev?.sig === sig) continue;
      const shown = prev?.shown ?? false;
      if (prev) this.drop(id);
      changed = true;
      const kit = new Kit();
      const group = el.kind === "wall" ? this.wall(el, project, outlines.get(el.id) ?? null, kit) : buildGhosts(alone(project, el, byId), [id], this.lib, kit);
      if (group.children.length === 0) {
        kit.dispose();
        continue;
      }
      this.tint(group, color);
      group.visible = shown;
      group.userData.draftId = id;
      this.root.add(group);
      this.items.set(id, { group, kit, sig, shown });
    }
    // Nothing on screen holds a library material: the forms wear the draft's own.
    this.lib.end();
    return changed;
  }

  /** Shows what `shown` allows. Returns the meshes that just came in, for their fade. */
  show(shown: (id: string) => boolean): THREE.Mesh[] {
    const fresh: THREE.Mesh[] = [];
    for (const [id, b] of this.items) {
      const want = shown(id);
      if (want === b.shown) continue;
      b.shown = want;
      b.group.visible = want;
      if (want) b.group.traverse((x) => (x as THREE.Mesh).isMesh && fresh.push(x as THREE.Mesh));
    }
    return fresh;
  }

  /** The draft elements on screen. */
  shownIds(): string[] {
    return [...this.items].filter(([, b]) => b.shown).map(([id]) => id);
  }

  /** Every draft element built, shown or not. */
  ids(): string[] {
    return [...this.items.keys()];
  }

  dispose(): void {
    for (const id of [...this.items.keys()]) this.drop(id);
    this.root.removeFromParent();
    for (const m of this.mats.values()) m.dispose();
    this.mats.clear();
    this.lib.dispose();
  }

  private drop(id: string): void {
    const b = this.items.get(id);
    if (!b) return;
    b.group.removeFromParent();
    b.kit.dispose();
    this.items.delete(id);
  }

  private material(color: string): THREE.MeshStandardMaterial {
    let m = this.mats.get(color);
    if (!m) {
      m = new THREE.MeshStandardMaterial({ color: new THREE.Color(color), roughness: 0.9, metalness: 0, transparent: true, opacity: DRAFT_OPACITY, depthWrite: false });
      this.mats.set(color, m);
    }
    return m;
  }

  /** One flat color, no shadows, never picked: a draft is looked at, not edited. */
  private tint(group: THREE.Group, color: string): void {
    const mat = this.material(color);
    group.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (mesh.isMesh) {
        mesh.material = mat;
        mesh.castShadow = false;
        mesh.receiveShadow = false;
        mesh.renderOrder = 3;
        mesh.raycast = () => {};
      } else if ((o as THREE.Line).isLine || (o as THREE.Points).isPoints || (o as THREE.Sprite).isSprite) {
        o.visible = false;
      }
    });
  }

  /** A draft wall with the draft's openings cut in it. */
  private wall(w: Wall, project: Project, outline: Array<{ x: number; y: number }> | null, kit: Kit): THREE.Group {
    const group = new THREE.Group();
    const levels = [...(project.levels ?? [])].sort((a, b) => a.elevation_mm - b.elevation_mm);
    const level = levels.find((l) => l.id === w.level_id) ?? levels[0];
    if (!level) return group;
    const drop = level === levels[0] ? PLINTH_MM : 0;
    const openings = project.elements.filter((e): e is OpeningEl => e.kind === "opening" && e.wall_id === w.id);
    const md = new MeshData();
    const ok = buildWallMesh(
      {
        start: w.start,
        end: w.end,
        thickness: w.thickness_mm,
        height: (w.height_mm ?? level.height_mm) + drop,
        elevation: level.elevation_mm - drop,
        outline,
        openings: openings.map((o) => ({ offset: o.offset_mm, width: o.width_mm, height: o.height_mm, sill: o.sill_mm + drop })),
        maxHeight: null,
      },
      md,
    );
    if (ok) group.add(kit.mesh(kit.fromMeshData(md), this.material(CONTEXT_COLOR)));
    return group;
  }
}

/** What a draft element's form depends on: itself, a wall's openings, an opening's wall. */
function signature(el: Element, project: Project): string {
  if (el.kind === "wall") {
    const openings = project.elements.filter((e) => e.kind === "opening" && e.wall_id === el.id);
    return JSON.stringify([el, openings]);
  }
  if (el.kind === "opening") {
    const host = project.elements.find((e) => e.id === el.wall_id);
    return JSON.stringify([el, host]);
  }
  return JSON.stringify(el);
}

/** The project with only this element (and an opening's wall), so a ghost builds in O(1). */
function alone(project: Project, el: Element, byId: Map<string, Element>): Project {
  const host = el.kind === "opening" ? byId.get(el.wall_id) : undefined;
  return { ...project, elements: host ? [el, host] : [el] };
}
