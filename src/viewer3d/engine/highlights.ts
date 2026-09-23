// Hover, selection and AI preview tints for the 3D view, as animated values
// rather than a material swap. One entry per element: it clones the element's
// materials once, then blends them from the plain look (k = 0) to the full
// highlight (k = 1) every frame. Fading out runs the same blend backwards and
// disposes the clones at 0, so nothing is left behind.
//
// The blend starts from the base material as it is right now, not as it was
// when the highlight began: the X-ray and hidden shell modes change opacity
// and visibility of the base materials while an element stays selected.

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
}

const scratch = new THREE.Color();
const glowFrom = new THREE.Color();
const glowTo = new THREE.Color();

/**
 * Screen-space outline for thin round things (pipes), where an edge outline
 * would only find the ring at each end: the back faces of the mesh, pushed
 * out along their normals by a fixed number of pixels, show as a band around
 * the silhouette.
 */
const HULL_VERTEX = /* glsl */ `
  uniform vec2 resolution;
  uniform float width;
  void main() {
    vec4 clip = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
    vec3 n = normalize(normalMatrix * normal);
    vec2 dir = (projectionMatrix * vec4(n, 0.0)).xy * resolution;
    float len = length(dir);
    dir = len > 1e-6 ? dir / len : vec2(0.0);
    clip.xy += dir * width * 2.0 / resolution * clip.w;
    gl_Position = clip;
  }
`;

const HULL_FRAGMENT = /* glsl */ `
  uniform vec3 color;
  uniform float opacity;
  void main() {
    gl_FragColor = vec4(color, opacity);
    #include <colorspace_fragment>
  }
`;

function hullMaterial(resolution: THREE.Vector2): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    uniforms: {
      color: { value: SELECT_COLOR.clone() },
      opacity: { value: 0 },
      width: { value: 2.2 },
      resolution: { value: resolution.clone() },
    },
    vertexShader: HULL_VERTEX,
    fragmentShader: HULL_FRAGMENT,
    side: THREE.BackSide,
    transparent: true,
    depthWrite: false,
    toneMapped: false,
  });
}

/** One element's tint. Created on the way in, released on the way out. */
export class Highlight {
  readonly members: Member[] = [];
  private outlines: LineSegments2[] = [];
  private outlineMaterial: LineMaterial | null = null;
  private hulls: THREE.Mesh[] = [];
  private hullMat: THREE.ShaderMaterial | null = null;
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
      this.members.push({ mesh, base, clone });
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

  /** The silhouette band used for pipes, on the same fade as every other outline. */
  addHullOutline(mesh: THREE.Mesh, resolution: THREE.Vector2): void {
    this.hullMat ??= hullMaterial(resolution);
    const hull = new THREE.Mesh(mesh.geometry, this.hullMat);
    hull.raycast = () => {};
    hull.renderOrder = 3;
    hull.castShadow = false;
    hull.receiveShadow = false;
    hull.userData.outline = true;
    mesh.add(hull);
    this.hulls.push(hull);
  }

  hasOutline(): boolean {
    return this.outlines.length > 0 || this.hulls.length > 0;
  }

  setResolution(w: number, h: number): void {
    this.outlineMaterial?.resolution.set(w, h);
    this.hullMat?.uniforms.resolution.value.set(w, h);
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
      const b = m.base;
      c.color.copy(b.color);
      if (s.mix > 0) c.color.lerp(scratch.copy(s.tint), s.mix * strength);
      // From the material's own glow (pipes have one) to the highlight's.
      glowFrom.copy(b.emissive).multiplyScalar(b.emissiveIntensity);
      glowTo.copy(s.emissive).multiplyScalar(s.emissiveIntensity);
      c.emissive.copy(glowFrom).lerp(glowTo, strength);
      c.emissiveIntensity = 1;
      let opacity = b.opacity;
      if (s.opacity !== null) {
        const want = s.breathes && breath !== undefined ? breath : s.opacity;
        const target = Math.min(b.opacity, want);
        opacity = b.opacity + (target - b.opacity) * strength;
      }
      c.opacity = opacity;
      const seeThrough = b.transparent || opacity < 0.999;
      if (c.transparent !== seeThrough) {
        c.transparent = seeThrough;
        c.needsUpdate = true;
      }
      c.depthWrite = seeThrough ? false : b.depthWrite;
      c.visible = b.visible;
      const nextMap = wantMapOff ? null : b.map;
      if (c.map !== nextMap) {
        c.map = nextMap;
        c.needsUpdate = true;
      }
    }
    if (this.outlineMaterial) this.outlineMaterial.opacity = strength;
    if (this.hullMat) this.hullMat.uniforms.opacity.value = strength;
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
    for (const hull of this.hulls) hull.parent?.remove(hull);
    this.hulls.length = 0;
    this.hullMat?.dispose();
    this.hullMat = null;
  }
}
