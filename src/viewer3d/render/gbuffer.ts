// Albedo, normals and depth of a render's view, drawn with the raster scene
// the tracer traced, in the job's own renderer, for the denoiser
// (render/denoise.ts).
//
// - Albedo: base color times its texture, as the tracer reads it, averaged
//   over jittered frames with the tracer's own pixel filter (a tent one pixel
//   wide each way), so edges and texture detail line up with the traced
//   image. Glass is blended over what is behind it at its opacity: the
//   tracer sees through glass that often. Alpha is how much of the pixel is
//   covered by the model rather than sky.
// - Normal and depth: one frame through the pixel centres, view space. Glass
//   is left out where there is something behind it (the tracer mostly sees
//   through it) and drawn where there is only sky, so a window at night is
//   filtered as the pane it is. Depth 0 is sky; a negative depth marks a
//   glowing surface (a lit diffuser), which the filter leaves as traced.
//   Normals carry the normal maps: the relief the tracer shades.
// - Coverage: how much of each pixel, through the same tent filter, is the
//   surface at its centre. The filter gives a pixel the light of the surface
//   at its centre; at an edge the traced pixel mixed two surfaces, and the
//   coverage lets the last pass mix them back (render/denoise.ts), so edges
//   keep the tracer's anti-aliasing instead of turning into stairs.
//
// Nothing here changes the scene for good: every mesh gets its own material
// back, and the sky comes back on.

import * as THREE from "three";
import { compileScene, ScreenPass } from "./screenPass";

export interface GBuffer {
  /** rgb: albedo. Half float. */
  albedo: THREE.WebGLRenderTarget;
  /** xyz: view space normal, w: view depth in meters (0 sky, negative for a glowing surface). Float. */
  normalDepth: THREE.WebGLRenderTarget;
  /** r: share of the pixel covered by the surface at its centre. Half float. */
  coverage: THREE.WebGLRenderTarget;
  dispose(): void;
}

/** Jittered frames averaged for the albedo. */
export const ALBEDO_FRAMES = 24;
/** Jittered frames for the coverage. */
export const COVERAGE_FRAMES = 16;
/** Two samples are the same surface when their normals are at least this alike and their planes agree. */
export const SAME_NORMAL = 0.8;

/** The tracer's pixel filter (three-gpu-pathtracer `tentFilter`): a uniform number to an offset in -1..1 pixels. */
export function tent(u: number): number {
  return u < 0.5 ? Math.sqrt(2 * u) - 1 : 1 - Math.sqrt(2 - 2 * u);
}

/** Low discrepancy sequence, base `b`. */
function radical(i: number, b: number): number {
  let f = 1;
  let r = 0;
  for (let n = i; n > 0; n = Math.floor(n / b)) {
    f /= b;
    r += f * (n % b);
  }
  return r;
}

/** Sub-pixel offset of albedo frame `i`, pixels: tent distributed, like the tracer's rays. */
export function albedoJitter(i: number): [number, number] {
  return [tent(radical(i + 1, 2)), tent(radical(i + 1, 3))];
}

/** True for a surface the tracer mostly sees through. */
function isGlass(m: THREE.Material): boolean {
  return m.transparent && m.opacity < 0.999;
}

/** True for a surface that gives light of its own. */
function glows(m: THREE.Material): boolean {
  const s = m as THREE.MeshStandardMaterial;
  return !!s.emissive && (s.emissiveIntensity ?? 0) > 0 && s.emissive.r + s.emissive.g + s.emissive.b > 0;
}

type Textured = THREE.Material & {
  color?: THREE.Color;
  map?: THREE.Texture | null;
  alphaMap?: THREE.Texture | null;
  vertexColors?: boolean;
};

function albedoMaterial(m: THREE.Material): THREE.Material {
  const t = m as Textured;
  return new THREE.MeshBasicMaterial({
    color: t.color ?? new THREE.Color(1, 1, 1),
    map: t.map ?? null,
    alphaMap: t.alphaMap ?? null,
    alphaTest: m.alphaTest,
    transparent: m.transparent,
    opacity: m.opacity,
    side: m.side,
    vertexColors: t.vertexColors ?? false,
    depthWrite: m.depthWrite,
    polygonOffset: m.polygonOffset,
    polygonOffsetFactor: m.polygonOffsetFactor,
    polygonOffsetUnits: m.polygonOffsetUnits,
    toneMapped: false,
    fog: false,
  });
}

const ND_VERT = /* glsl */ `
  varying vec3 vNormal;
  varying vec3 vView;
  varying vec2 vUv;
  varying vec2 vNormalUv;
  uniform mat3 alphaTransform;
  uniform mat3 normalTransform;
  void main() {
    vNormal = normalMatrix * normal;
    vec4 mv = modelViewMatrix * vec4(position, 1.0);
    vView = mv.xyz;
    #ifdef USE_CUTOUT
      vUv = (alphaTransform * vec3(uv, 1.0)).xy;
    #endif
    #ifdef USE_NORMAL_MAP
      vNormalUv = (normalTransform * vec3(uv, 1.0)).xy;
    #endif
    gl_Position = projectionMatrix * mv;
  }
`;

const ND_FRAG = /* glsl */ `
  varying vec3 vNormal;
  varying vec3 vView;
  varying vec2 vUv;
  varying vec2 vNormalUv;
  uniform float glow;
  uniform sampler2D cutout;
  uniform float cutoutAt;
  uniform float cutoutChannel;
  uniform sampler2D normalMap;
  uniform vec2 normalScale;
  // three's tangent frame from screen derivatives (normal_fragment_begin),
  // for meshes without tangents: the raster view shades normal maps with it.
  mat3 tangentFrame(vec3 eye, vec3 n, vec2 uv) {
    vec3 q0 = dFdx(eye);
    vec3 q1 = dFdy(eye);
    vec2 st0 = dFdx(uv);
    vec2 st1 = dFdy(uv);
    vec3 q1perp = cross(q1, n);
    vec3 q0perp = cross(n, q0);
    vec3 T = q1perp * st0.x + q0perp * st1.x;
    vec3 B = q1perp * st0.y + q0perp * st1.y;
    float det = max(dot(T, T), dot(B, B));
    float scale = det == 0.0 ? 0.0 : inversesqrt(det);
    return mat3(T * scale, B * scale, n);
  }
  void main() {
    #ifdef USE_CUTOUT
      vec4 c = texture2D(cutout, vUv);
      float a = cutoutChannel > 0.5 ? c.g : c.a;
      if (a < cutoutAt) discard;
    #endif
    vec3 n = vNormal;
    // A mesh without normals: the face normal from the position's slope.
    if (dot(n, n) < 1e-8) n = cross(dFdx(vView), dFdy(vView));
    n = normalize(n);
    float face = gl_FrontFacing ? 1.0 : -1.0;
    n *= face;
    #ifdef USE_NORMAL_MAP
      // The relief the tracer shades (tile grout, plaster), so the filter
      // does not smooth it away as if it were noise.
      mat3 tbn = tangentFrame(vView, n, vNormalUv);
      tbn[0] *= face;
      tbn[1] *= face;
      vec3 mapN = texture2D(normalMap, vNormalUv).xyz * 2.0 - 1.0;
      mapN.xy *= normalScale;
      n = normalize(tbn * mapN);
    #endif
    float depth = -vView.z;
    gl_FragColor = vec4(n, glow > 0.5 ? -depth : depth);
  }
`;

function normalDepthMaterial(m: THREE.Material): THREE.Material {
  const t = m as Textured;
  const glass = isGlass(m);
  // Cut-outs: an alpha map (the ground's soft edge) or an alpha tested map.
  const cutout = t.alphaMap ?? (m.alphaTest > 0 ? (t.map ?? null) : null);
  const normalMap = (m as THREE.MeshStandardMaterial).normalMap ?? null;
  const normalScale = (m as THREE.MeshStandardMaterial).normalScale ?? new THREE.Vector2(1, 1);
  if (cutout) cutout.updateMatrix();
  if (normalMap) normalMap.updateMatrix();
  const defines: Record<string, string> = {};
  if (cutout) defines.USE_CUTOUT = "";
  if (normalMap) defines.USE_NORMAL_MAP = "";
  const mat = new THREE.ShaderMaterial({
    uniforms: {
      glow: { value: glows(m) ? 1 : 0 },
      cutout: { value: cutout },
      cutoutAt: { value: m.alphaTest > 0 ? m.alphaTest : 0.5 },
      // alphaMap reads green, a map reads alpha (three's alphamap_fragment, map_fragment).
      cutoutChannel: { value: t.alphaMap ? 1 : 0 },
      alphaTransform: { value: cutout ? cutout.matrix.clone() : new THREE.Matrix3() },
      normalMap: { value: normalMap },
      normalScale: { value: normalScale.clone() },
      normalTransform: { value: normalMap ? normalMap.matrix.clone() : new THREE.Matrix3() },
    },
    defines,
    vertexShader: ND_VERT,
    fragmentShader: ND_FRAG,
    side: m.side,
    polygonOffset: m.polygonOffset,
    polygonOffsetFactor: m.polygonOffsetFactor,
    polygonOffsetUnits: m.polygonOffsetUnits,
    // Solid surfaces mark the stencil; glass draws only where none did.
    stencilWrite: true,
    stencilRef: glass ? 0 : 1,
    stencilFunc: glass ? THREE.EqualStencilFunc : THREE.AlwaysStencilFunc,
    stencilZPass: glass ? THREE.KeepStencilOp : THREE.ReplaceStencilOp,
  });
  return mat;
}

const BLEND_FRAG = /* glsl */ `
  uniform sampler2D map;
  uniform float weight;
  varying vec2 vUv;
  void main() {
    gl_FragColor = texture2D(map, vUv) * weight;
  }
`;

const VERT = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

/** Adds `weight` where a jittered normal-depth sample is the same surface as the pixel centre's. */
const SAME_FRAG = /* glsl */ `
  uniform sampler2D tCentre;
  uniform sampler2D tJitter;
  uniform vec2 jitter;
  uniform vec2 size;
  uniform vec2 tanHalf;
  uniform float planeShare;
  uniform float weight;
  vec3 viewPos(vec2 pixel, float depth) {
    return vec3((pixel / size * 2.0 - 1.0) * tanHalf, -1.0) * depth;
  }
  void main() {
    ivec2 p = ivec2(gl_FragCoord.xy);
    vec4 a = texelFetch(tCentre, p, 0);
    vec4 b = texelFetch(tJitter, p, 0);
    float same = 0.0;
    if (a.w == 0.0 || b.w == 0.0) {
      same = a.w == 0.0 && b.w == 0.0 ? 1.0 : 0.0;
    } else if (sign(a.w) == sign(b.w) && dot(a.xyz, b.xyz) > ${SAME_NORMAL.toFixed(3)}) {
      // setViewOffset moves the view right by x and down by y pixels.
      vec3 pa = viewPos(vec2(p) + 0.5, abs(a.w));
      vec3 pb = viewPos(vec2(p) + 0.5 + vec2(jitter.x, -jitter.y), abs(b.w));
      same = abs(dot(a.xyz, pb - pa)) < 2.0 * planeShare * abs(a.w) ? 1.0 : 0.0;
    }
    gl_FragColor = vec4(same * weight, 0.0, 0.0, 0.0);
  }
`;

/**
 * Draws the G-buffer in steps: `pace` is awaited after each few frames, so
 * the job can wait on the GPU the way it does between slices. Resolves with
 * the targets, or null when `cancelled` turned true meanwhile.
 */
export async function drawGBuffer(
  r: THREE.WebGLRenderer,
  scene: THREE.Scene,
  camera: THREE.PerspectiveCamera,
  width: number,
  height: number,
  pace: () => Promise<void>,
  cancelled: () => boolean,
  planeShare: number,
  frames = ALBEDO_FRAMES,
): Promise<GBuffer | null> {
  const normalDepth = new THREE.WebGLRenderTarget(width, height, {
    type: THREE.FloatType,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    depthBuffer: true,
    stencilBuffer: true,
    generateMipmaps: false,
  });
  const coverage = new THREE.WebGLRenderTarget(width, height, {
    type: THREE.HalfFloatType,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    depthBuffer: false,
    generateMipmaps: false,
  });
  // A jittered normal-depth frame; half floats are enough to tell surfaces apart.
  const jittered = new THREE.WebGLRenderTarget(width, height, {
    type: THREE.HalfFloatType,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    depthBuffer: true,
    stencilBuffer: true,
    generateMipmaps: false,
  });
  const albedo = new THREE.WebGLRenderTarget(width, height, {
    type: THREE.HalfFloatType,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    depthBuffer: false,
    generateMipmaps: false,
  });
  const frame = new THREE.WebGLRenderTarget(width, height, {
    type: THREE.HalfFloatType,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    depthBuffer: true,
    generateMipmaps: false,
  });
  const out: GBuffer = {
    albedo,
    normalDepth,
    coverage,
    dispose: () => {
      albedo.dispose();
      normalDepth.dispose();
      coverage.dispose();
    },
  };

  // Every mesh's own material, and its two stand-ins.
  const meshes: THREE.Mesh[] = [];
  scene.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (mesh.isMesh && !Array.isArray(mesh.material)) meshes.push(mesh);
  });
  const own = new Map<THREE.Mesh, THREE.Material>();
  const albedoMats = new Map<THREE.Material, THREE.Material>();
  const ndMats = new Map<THREE.Material, THREE.Material>();
  for (const mesh of meshes) {
    const m = mesh.material as THREE.Material;
    own.set(mesh, m);
    if (!albedoMats.has(m)) albedoMats.set(m, albedoMaterial(m));
    if (!ndMats.has(m)) ndMats.set(m, normalDepthMaterial(m));
  }
  const hidden: THREE.Mesh[] = [];
  const background = scene.background;
  const environment = scene.environment;
  const clearColor = new THREE.Color();
  r.getClearColor(clearColor);
  const clearAlpha = r.getClearAlpha();
  const prevTarget = r.getRenderTarget();
  const blend = new ScreenPass(
    new THREE.ShaderMaterial({
      uniforms: { map: { value: frame.texture }, weight: { value: 1 } },
      vertexShader: VERT,
      fragmentShader: BLEND_FRAG,
      blending: THREE.CustomBlending,
      blendEquation: THREE.AddEquation,
      blendSrc: THREE.OneFactor,
      blendDst: THREE.OneFactor,
      depthTest: false,
      depthWrite: false,
    }),
  );
  const tanY = Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2) / camera.zoom;
  const same = new ScreenPass(
    new THREE.ShaderMaterial({
      uniforms: {
        tCentre: { value: normalDepth.texture },
        tJitter: { value: jittered.texture },
        jitter: { value: new THREE.Vector2() },
        size: { value: new THREE.Vector2(width, height) },
        tanHalf: { value: new THREE.Vector2(tanY * camera.aspect, tanY) },
        planeShare: { value: planeShare },
        weight: { value: 1 / COVERAGE_FRAMES },
      },
      vertexShader: VERT,
      fragmentShader: SAME_FRAG,
      blending: THREE.CustomBlending,
      blendEquation: THREE.AddEquation,
      blendSrc: THREE.OneFactor,
      blendDst: THREE.OneFactor,
      depthTest: false,
      depthWrite: false,
    }),
  );
  const restore = () => {
    for (const [mesh, m] of own) mesh.material = m;
    for (const mesh of hidden) mesh.visible = true;
    hidden.length = 0;
    scene.background = background;
    scene.environment = environment;
    camera.clearViewOffset();
    r.setClearColor(clearColor, clearAlpha);
    r.setRenderTarget(prevTarget);
  };
  let ok = false;
  try {
    scene.background = null;
    scene.environment = null;

    // Normal and depth: pixel centres. Solid surfaces first, then glass
    // where they left sky.
    for (const mesh of meshes) mesh.material = ndMats.get(own.get(mesh)!)!;
    await compileScene(r, scene, camera, normalDepth);
    if (cancelled()) return null;
    const hideWhere = (glass: boolean) => {
      for (const mesh of meshes) {
        if (isGlass(own.get(mesh)!) === glass && mesh.visible) {
          mesh.visible = false;
          hidden.push(mesh);
        }
      }
    };
    const showAll = () => {
      for (const mesh of hidden) mesh.visible = true;
      hidden.length = 0;
    };
    const drawNormalDepth = (target: THREE.WebGLRenderTarget) => {
      r.setClearColor(0x000000, 0);
      r.setRenderTarget(target);
      r.clear(true, true, true);
      const autoClear = r.autoClear;
      r.autoClear = false;
      hideWhere(true);
      r.render(scene, camera);
      showAll();
      hideWhere(false);
      r.render(scene, camera);
      showAll();
      r.autoClear = autoClear;
    };
    drawNormalDepth(normalDepth);
    await pace();
    if (cancelled()) return null;

    // Coverage: jittered normal-depth frames, each compared with the centre.
    await Promise.all([compileScene(r, scene, camera, jittered), same.compile(r, coverage)]);
    if (cancelled()) return null;
    r.setClearColor(0x000000, 0);
    r.setRenderTarget(coverage);
    r.clear(true, false, false);
    for (let i = 0; i < COVERAGE_FRAMES; i++) {
      const [jx, jy] = albedoJitter(i);
      camera.setViewOffset(width, height, jx, jy, width, height);
      drawNormalDepth(jittered);
      camera.clearViewOffset();
      same.material.uniforms.jitter.value.set(jx, jy);
      r.setRenderTarget(coverage);
      const autoClear = r.autoClear;
      r.autoClear = false;
      same.render(r);
      r.autoClear = autoClear;
      if (i % 4 === 3) {
        await pace();
        if (cancelled()) return null;
      }
    }
    jittered.dispose();

    // Albedo: jittered frames summed at 1/frames each.
    for (const mesh of meshes) mesh.material = albedoMats.get(own.get(mesh)!)!;
    await Promise.all([compileScene(r, scene, camera, frame), blend.compile(r, albedo)]);
    if (cancelled()) return null;
    r.setRenderTarget(albedo);
    r.clear(true, false, false);
    const weight = blend.material.uniforms.weight;
    weight.value = 1 / frames;
    for (let i = 0; i < frames; i++) {
      const [jx, jy] = albedoJitter(i);
      camera.setViewOffset(width, height, jx, jy, width, height);
      // Sky reads as white albedo with no coverage.
      r.setClearColor(0xffffff, 0);
      r.setRenderTarget(frame);
      r.clear(true, true, false);
      r.render(scene, camera);
      camera.clearViewOffset();
      r.setRenderTarget(albedo);
      // The blend adds to what is there: an automatic clear would throw it away.
      const autoClear = r.autoClear;
      r.autoClear = false;
      blend.render(r);
      r.autoClear = autoClear;
      if (i % 6 === 5) {
        await pace();
        if (cancelled()) return null;
      }
    }
    ok = true;
    return out;
  } finally {
    restore();
    frame.dispose();
    jittered.dispose();
    blend.dispose();
    same.dispose();
    for (const m of albedoMats.values()) m.dispose();
    for (const m of ndMats.values()) m.dispose();
    if (!ok) out.dispose();
  }
}
