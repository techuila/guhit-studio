// Hover, selection and AI preview tints for the 3D view, as animated values
// rather than a material swap. One entry per element: it clones the element's
// materials once, then blends them from the plain look (k = 0) to the full
// highlight (k = 1) every frame. Fading out runs the same blend backwards and
// disposes the clones at 0, so nothing is left behind.

import * as THREE from "three";
import { LineMaterial } from "three/examples/jsm/lines/LineMaterial.js";
import { LineSegments2 } from "three/examples/jsm/lines/LineSegments2.js";
import type { LineSegmentsGeometry } from "three/examples/jsm/lines/LineSegmentsGeometry.js";

export type HighlightState = "selected" | "selected-soft" | "hover" | "preview" | "preview-selected" | "flash";

export const SELECT_COLOR = new THREE.Color("#0e8a8f"); // --draw-selection
export const PREVIEW_COLOR = new THREE.Color("#6b4fbb"); // --draw-preview

interface StateSpec {
  tint: THREE.Color;
  /** How far the base color moves towards `tint` at full strength. */
  mix: number;
  emissive: THREE.Color;
  emissiveIntensity: number;
  /** Opacity at full strength, or null to keep the material's own. */
  opacity: number | null;
  /** Drops the texture, so a preview ghost reads as one flat mass. */
  dropMap: boolean;
  outline: boolean;
  /** Purple previews breathe. Nothing else does. */
  breathes: boolean;
}

const SPECS: Record<HighlightState, StateSpec> = {
  selected: { tint: SELECT_COLOR, mix: 0.55, emissive: SELECT_COLOR, emissiveIntensity: 0.4, opacity: null, dropMap: false, outline: true, breathes: false },
  "selected-soft": { tint: SELECT_COLOR, mix: 0.18, emissive: SELECT_COLOR, emissiveIntensity: 0.12, opacity: null, dropMap: false, outline: true, breathes: false },
  hover: { tint: SELECT_COLOR, mix: 0, emissive: SELECT_COLOR, emissiveIntensity: 0.22, opacity: null, dropMap: false, outline: false, breathes: false },
  preview: { tint: PREVIEW_COLOR, mix: 0.6, emissive: PREVIEW_COLOR, emissiveIntensity: 0.35, opacity: 0.78, dropMap: true, outline: false, breathes: true },
  "preview-selected": { tint: PREVIEW_COLOR, mix: 0.6, emissive: PREVIEW_COLOR, emissiveIntensity: 0.6, opacity: 0.78, dropMap: true, outline: true, breathes: true },
  flash: { tint: SELECT_COLOR, mix: 0.12, emissive: SELECT_COLOR, emissiveIntensity: 0.5, opacity: null, dropMap: false, outline: false, breathes: false },
};

export function spec(state: HighlightState): StateSpec {
  return SPECS[state];
}

interface Member {
  mesh: THREE.Mesh;
  base: THREE.MeshStandardMaterial;
  clone: THREE.MeshStandardMaterial;
  baseColor: THREE.Color;
  baseOpacity: number;
  baseTransparent: boolean;
}

const scratch = new THREE.Color();

/** One element's tint. Created on the way in, released on the way out. */
export class Highlight {
  readonly members: Member[] = [];
  private outlines: LineSegments2[] = [];
  private outlineMaterial: LineMaterial | null = null;
  private released = false;

  constructor(
    readonly id: string,
    public state: HighlightState,
    meshes: THREE.Mesh[],
  ) {
    for (const mesh of meshes) {
      const base = mesh.material as THREE.MeshStandardMaterial;
      if (Array.isArray(mesh.material) || !base?.isMeshStandardMaterial) continue;
      const clone = base.clone();
      clone.userData = { ...base.userData };
      this.members.push({
        mesh,
        base,
        clone,
        baseColor: base.color.clone(),
        baseOpacity: base.opacity,
        baseTransparent: base.transparent,
      });
      mesh.material = clone;
    }
  }

  get empty(): boolean {
    return this.members.length === 0;
  }

  /** Switches look without restarting the fade: `k` carries over. */
  retarget(state: HighlightState): void {
    this.state = state;
  }

  /**
   * Outline geometry is cached by the engine; this only owns the line objects
   * and their material, whose opacity follows the same `k`.
   */
  addOutline(mesh: THREE.Mesh, edges: LineSegmentsGeometry, resolution: THREE.Vector2): void {
    this.outlineMaterial ??= new LineMaterial({
      color: SELECT_COLOR.getHex(),
      linewidth: 2.2,
      toneMapped: false,
      transparent: true,
      opacity: 0,
    });
    this.outlineMaterial.resolution.copy(resolution);
    const line = new LineSegments2(edges, this.outlineMaterial);
    line.raycast = () => {};
    line.renderOrder = 3;
    mesh.add(line);
    this.outlines.push(line);
  }

  hasOutline(): boolean {
    return this.outlines.length > 0;
  }

  setResolution(w: number, h: number): void {
    this.outlineMaterial?.resolution.set(w, h);
  }

  /**
   * `k` is the fade 0..1. `breath` is the absolute opacity the slow AI pulse
   * currently asks for; it only reaches the states that breathe.
   */
  apply(k: number, breath?: number): void {
    if (this.released) return;
    const s = SPECS[this.state];
    const strength = Math.min(Math.max(k, 0), 1);
    // Dropping the texture changes the shader, so it only flips once per
    // direction instead of every frame.
    const wantMapOff = s.dropMap && strength > 0.02;
    for (const m of this.members) {
      const c = m.clone;
      c.color.copy(m.baseColor);
      if (s.mix > 0) c.color.lerp(scratch.copy(s.tint), s.mix * strength);
      c.emissive.copy(s.emissive);
      c.emissiveIntensity = s.emissiveIntensity * strength;
      if (s.opacity !== null) {
        const want = s.breathes && breath !== undefined ? breath : s.opacity;
        const target = Math.min(m.baseOpacity, want);
        const opacity = m.baseOpacity + (target - m.baseOpacity) * strength;
        c.opacity = opacity;
        const seeThrough = m.baseTransparent || opacity < 0.999;
        c.transparent = seeThrough;
        c.depthWrite = seeThrough ? false : m.base.depthWrite;
      }
      const nextMap = wantMapOff ? null : m.base.map;
      if (c.map !== nextMap) {
        c.map = nextMap;
        c.needsUpdate = true;
      }
    }
    if (this.outlineMaterial) this.outlineMaterial.opacity = strength;
  }

  /** Puts the original materials back and disposes everything this created. */
  release(): void {
    if (this.released) return;
    this.released = true;
    for (const m of this.members) {
      m.mesh.material = m.base;
      m.clone.dispose();
    }
    this.members.length = 0;
    for (const line of this.outlines) line.parent?.remove(line);
    this.outlines.length = 0;
    this.outlineMaterial?.dispose();
    this.outlineMaterial = null;
  }
}
