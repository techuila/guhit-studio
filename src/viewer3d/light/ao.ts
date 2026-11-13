// Screen space ambient occlusion for the live view: corners, the floor under
// furniture and the ground along the walls darken the way bounce light does.
//
// Per live frame, inside the frame's own render call (a `scene.onBeforeRender`
// hook armed by the engine, so there is still exactly one render per frame
// and no requestAnimationFrame of its own):
// 1. the model's opaque meshes (those on `AO_LAYER`) are drawn once more, at
//    half resolution, as view space normals and depth;
// 2. a 12 tap spiral around each pixel measures how much of its hemisphere
//    nearby geometry covers (within `AO_RADIUS_M`, so a far wall never
//    shadows a near chair);
// 3. a 4 x 4 depth aware blur takes out the rotation pattern.
// The lit materials then read the result at their pixel and dim their
// indirect light with it (sky, image based light and the bounce fill), and
// their direct light a little (`AO_DIRECT`): the sun still lights a sunlit
// corner, as it does in reality. Refine samples reuse the frame's result.
//
// The uniforms belong to a MaterialLibrary (`MaterialLibrary.ao`), so a
// render job's own library never reads the live view's texture.

import * as THREE from "three";

/** The layer the occluders are on: opaque model meshes and the ground. */
export const AO_LAYER = 4;
/** How far occlusion reaches, meters: a room corner, a sofa against a wall. */
export const AO_RADIUS_M = 0.55;
/** Strength on indirect light (1 is the full measured occlusion). */
export const AO_STRENGTH = 0.9;
/** Share of the occlusion applied to direct light: lamps and sun still reach into a corner. */
export const AO_DIRECT = 0.15;
const SAMPLES = 12;
/** How dark full cover gets: the measured cover (0 to about 0.5 in a corner) times this, off 1. */
const AO_GAIN = 3.2;
const TURNS = 7;

/** The uniforms every hooked material shares. `params`: strength, direct share, 1 / width, 1 / height of the full frame. */
export interface AoUniforms {
  map: THREE.IUniform<THREE.Texture | null>;
  params: THREE.IUniform<THREE.Vector4>;
}

export function aoUniforms(): AoUniforms {
  return { map: { value: null }, params: { value: new THREE.Vector4(0, 0, 0, 0) } };
}

/** The GLSL the materials take, after three's own ambient occlusion map. */
export const AO_PARS = /* glsl */ `
uniform sampler2D guhitAoMap;
uniform vec4 guhitAoParams;
`;
export const AO_APPLY = /* glsl */ `
if (guhitAoParams.x > 0.0) {
  float guhitAo = texture2D(guhitAoMap, gl_FragCoord.xy * guhitAoParams.zw).r;
  float guhitOcc = mix(1.0, guhitAo, guhitAoParams.x);
  reflectedLight.indirectDiffuse *= guhitOcc;
  reflectedLight.indirectSpecular *= mix(1.0, guhitOcc, 0.75);
  reflectedLight.directDiffuse *= mix(1.0, guhitAo, guhitAoParams.y);
}
`;

/**
 * Puts the screen space occlusion into a lit material's shader. A material
 * with its own `onBeforeCompile` runs that first. Every hooked material of
 * one library shares one program per base program (the cache key).
 */
export function hookAo(mat: THREE.Material, u: AoUniforms): void {
  const m = mat as THREE.MeshStandardMaterial;
  if (!m.isMeshStandardMaterial || m.userData.aoHooked) return;
  m.userData.aoHooked = true;
  const before = m.onBeforeCompile.bind(m);
  const key = m.customProgramCacheKey.bind(m);
  m.onBeforeCompile = (shader, renderer) => {
    before(shader, renderer);
    shader.uniforms.guhitAoMap = u.map;
    shader.uniforms.guhitAoParams = u.params;
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <aomap_pars_fragment>", `#include <aomap_pars_fragment>\n${AO_PARS}`)
      .replace("#include <aomap_fragment>", `#include <aomap_fragment>\n${AO_APPLY}`);
  };
  m.customProgramCacheKey = () => `${key()}|guhit-ao`;
  m.needsUpdate = true;
}

/** Marks what casts occlusion: opaque meshes, not glass, pipes, glows or anything fading. */
export function markOccluders(root: THREE.Object3D): void {
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh || mesh.userData.pipe || mesh.userData.batch || mesh.userData.lampPart) return;
    const mat = mesh.material as THREE.Material | THREE.Material[];
    if (Array.isArray(mat) || mat.transparent || mat.userData?.glass || mat.userData?.lampGlow) return;
    mesh.layers.enable(AO_LAYER);
  });
}

const VERT = /* glsl */ `
varying vec2 vUv;
void main() {
  vUv = uv;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}
`;

const AO_FRAG = /* glsl */ `
uniform sampler2D tDepth;
uniform sampler2D tNormal;
uniform mat4 invProj;
uniform vec2 size;
uniform float projScale;
uniform float radius;
varying vec2 vUv;

vec3 viewAt(vec2 uv, float d) {
  vec4 v = invProj * vec4(uv * 2.0 - 1.0, d * 2.0 - 1.0, 1.0);
  return v.xyz / v.w;
}

// 4 x 4 ordered rotation: the blur averages all sixteen.
float bayer4(vec2 p) {
  vec2 q = mod(floor(p), 4.0);
  float a = mod(q.x, 2.0) * 2.0 + mod(q.y, 2.0);
  float b = floor(q.x / 2.0) * 2.0 + floor(q.y / 2.0);
  return (a * 4.0 + b + 0.5) / 16.0;
}

void main() {
  float d = texture2D(tDepth, vUv).r;
  if (d >= 0.99999) { gl_FragColor = vec4(1.0); return; }
  vec3 P = viewAt(vUv, d);
  vec3 N = normalize(texture2D(tNormal, vUv).xyz * 2.0 - 1.0);
  float rPx = min(radius * projScale / max(-P.z, 1e-3), 90.0);
  if (rPx < 1.5) { gl_FragColor = vec4(1.0); return; }
  float a0 = bayer4(gl_FragCoord.xy) * 6.2831853;
  float occ = 0.0;
  float r2 = radius * radius;
  for (int i = 0; i < ${SAMPLES}; i++) {
    float t = (float(i) + 0.5) / float(${SAMPLES});
    float ang = a0 + t * ${TURNS}.0 * 6.2831853;
    vec2 uv = vUv + vec2(cos(ang), sin(ang)) * (t * rPx) / size;
    float dq = texture2D(tDepth, uv).r;
    vec3 v = viewAt(uv, dq) - P;
    float vv = dot(v, v);
    float fall = max(1.0 - vv / r2, 0.0);
    occ += fall * max(dot(v, N) * inversesqrt(vv + 1e-6) - 0.05, 0.0);
  }
  float ao = clamp(1.0 - ${AO_GAIN.toFixed(2)} * occ / float(${SAMPLES}), 0.0, 1.0);
  gl_FragColor = vec4(ao, ao, ao, 1.0);
}
`;

const BLUR_FRAG = /* glsl */ `
uniform sampler2D tAo;
uniform sampler2D tDepth;
uniform vec2 size;
uniform vec2 clip;
varying vec2 vUv;

float viewZ(float d) {
  float z = d * 2.0 - 1.0;
  return 2.0 * clip.x * clip.y / (clip.y + clip.x - z * (clip.y - clip.x));
}

void main() {
  float z0 = viewZ(texture2D(tDepth, vUv).r);
  float sum = 0.0;
  float wsum = 0.0;
  for (int y = 0; y < 4; y++) {
    for (int x = 0; x < 4; x++) {
      vec2 uv = vUv + (vec2(float(x), float(y)) - 1.5) / size;
      float z = viewZ(texture2D(tDepth, uv).r);
      float w = exp(-abs(z - z0) / max(z0 * 0.04, 0.02));
      sum += texture2D(tAo, uv).r * w;
      wsum += w;
    }
  }
  float ao = wsum > 0.0 ? sum / wsum : 1.0;
  gl_FragColor = vec4(ao, ao, ao, 1.0);
}
`;

type RenderFn = (scene: THREE.Object3D, camera: THREE.Camera) => void;

/** One triangle over the whole target, drawn with the unwrapped render (FullScreenQuad would go through `renderer.render`). */
class Pass {
  readonly mesh: THREE.Mesh;
  private static geometry: THREE.BufferGeometry | null = null;
  private static camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  constructor(readonly material: THREE.ShaderMaterial) {
    Pass.geometry ??= (() => {
      const g = new THREE.BufferGeometry();
      g.setAttribute("position", new THREE.Float32BufferAttribute([-1, 3, 0, -1, -1, 0, 3, -1, 0], 3));
      g.setAttribute("uv", new THREE.Float32BufferAttribute([0, 2, 0, 0, 2, 0], 2));
      return g;
    })();
    this.mesh = new THREE.Mesh(Pass.geometry, material);
    this.mesh.frustumCulled = false;
  }
  draw(render: RenderFn): void {
    render(this.mesh, Pass.camera);
  }
  dispose(): void {
    this.material.dispose();
  }
}

export class ScreenAO {
  private prepass: THREE.WebGLRenderTarget | null = null;
  private aoTarget: THREE.WebGLRenderTarget | null = null;
  private blurTarget: THREE.WebGLRenderTarget | null = null;
  private normalMat = new THREE.MeshNormalMaterial();
  private aoQuad: Pass;
  private blurQuad: Pass;
  private layers = new THREE.Layers();
  private armed = false;
  private clearColor = new THREE.Color();
  private size = new THREE.Vector2();
  /** Off: X-ray and hidden shells, or a GPU without what it needs. */
  enabled = true;
  /** The cutaway's clipping plane while it animates: the prepass is cut with the model. */
  clip: THREE.Plane | null = null;
  /** Time the last pass took on the CPU, ms, for the dev readout. */
  lastMs = 0;

  constructor(
    private uniforms: AoUniforms,
    /** The renderer's own render, as it was before anything wrapped it: these passes are part of the frame's one render call. */
    private render: RenderFn,
  ) {
    this.normalMat.side = THREE.FrontSide;
    this.aoQuad = new Pass(
      new THREE.ShaderMaterial({
        uniforms: {
          tDepth: { value: null },
          tNormal: { value: null },
          invProj: { value: new THREE.Matrix4() },
          size: { value: new THREE.Vector2() },
          projScale: { value: 1 },
          radius: { value: AO_RADIUS_M },
        },
        vertexShader: VERT,
        fragmentShader: AO_FRAG,
        depthTest: false,
        depthWrite: false,
        toneMapped: false,
      }),
    );
    this.blurQuad = new Pass(
      new THREE.ShaderMaterial({
        uniforms: { tAo: { value: null }, tDepth: { value: null }, size: { value: new THREE.Vector2() }, clip: { value: new THREE.Vector2(0.1, 3000) } },
        vertexShader: VERT,
        fragmentShader: BLUR_FRAG,
        depthTest: false,
        depthWrite: false,
        toneMapped: false,
      }),
    );
    this.layers.set(AO_LAYER);
  }

  /** The next render of the scene computes occlusion first (`beforeRender`). */
  arm(): void {
    this.armed = true;
  }

  /** Stops the materials reading occlusion, for renders it was not computed for. */
  off(): void {
    this.uniforms.params.value.x = 0;
  }

  /**
   * The scene's `onBeforeRender`: runs the passes when armed, for this
   * camera at the renderer's current drawing size, and leaves the result
   * bound for the materials.
   */
  beforeRender(renderer: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera): void {
    if (!this.armed) return;
    this.armed = false;
    if (!this.enabled || !(camera as THREE.PerspectiveCamera).isPerspectiveCamera) {
      this.off();
      return;
    }
    const t0 = performance.now();
    const cam = camera as THREE.PerspectiveCamera;
    const full = renderer.getRenderTarget() ? new THREE.Vector2(renderer.getRenderTarget()!.width, renderer.getRenderTarget()!.height) : renderer.getDrawingBufferSize(this.size);
    const w = Math.max(1, Math.ceil(full.x / 2));
    const h = Math.max(1, Math.ceil(full.y / 2));
    this.ensure(w, h);
    const prepass = this.prepass!;
    const aoTarget = this.aoTarget!;
    const blurTarget = this.blurTarget!;

    const prevTarget = renderer.getRenderTarget();
    const prevAlpha = renderer.getClearAlpha();
    renderer.getClearColor(this.clearColor);
    const prevBackground = scene.background;
    const prevOverride = scene.overrideMaterial;
    const prevLayers = cam.layers.mask;
    const prevShadow = renderer.shadowMap.enabled;
    const prevHook = scene.onBeforeRender;
    try {
      scene.onBeforeRender = () => {};
      scene.background = null;
      scene.overrideMaterial = this.normalMat;
      this.normalMat.clippingPlanes = this.clip ? [this.clip] : null;
      cam.layers.mask = this.layers.mask;
      renderer.shadowMap.enabled = false;
      renderer.setRenderTarget(prepass);
      renderer.setClearColor(0x8080ff, 1);
      renderer.clear(true, true, false);
      this.render(scene, cam);
    } finally {
      renderer.shadowMap.enabled = prevShadow;
      cam.layers.mask = prevLayers;
      scene.overrideMaterial = prevOverride;
      scene.background = prevBackground;
      scene.onBeforeRender = prevHook;
      renderer.setClearColor(this.clearColor, prevAlpha);
    }

    const ao = this.aoQuad.material as THREE.ShaderMaterial;
    ao.uniforms.tDepth.value = prepass.depthTexture;
    ao.uniforms.tNormal.value = prepass.texture;
    ao.uniforms.invProj.value.copy(cam.projectionMatrixInverse);
    ao.uniforms.size.value.set(w, h);
    ao.uniforms.projScale.value = h / (2 * Math.tan((cam.fov * Math.PI) / 360));
    renderer.setRenderTarget(aoTarget);
    this.aoQuad.draw(this.render);
    const blur = this.blurQuad.material as THREE.ShaderMaterial;
    blur.uniforms.tAo.value = aoTarget.texture;
    blur.uniforms.tDepth.value = prepass.depthTexture;
    blur.uniforms.size.value.set(w, h);
    blur.uniforms.clip.value.set(cam.near, cam.far);
    renderer.setRenderTarget(blurTarget);
    this.blurQuad.draw(this.render);
    renderer.setRenderTarget(prevTarget);

    this.uniforms.map.value = blurTarget.texture;
    this.uniforms.params.value.set(AO_STRENGTH, AO_DIRECT, 1 / full.x, 1 / full.y);
    this.lastMs = performance.now() - t0;
  }

  private ensure(w: number, h: number): void {
    if (this.prepass && this.prepass.width === w && this.prepass.height === h) return;
    this.disposeTargets();
    const depth = new THREE.DepthTexture(w, h, THREE.UnsignedIntType);
    this.prepass = new THREE.WebGLRenderTarget(w, h, { depthBuffer: true, depthTexture: depth, generateMipmaps: false, minFilter: THREE.NearestFilter, magFilter: THREE.NearestFilter });
    const opts = { depthBuffer: false, generateMipmaps: false, minFilter: THREE.LinearFilter, magFilter: THREE.LinearFilter };
    this.aoTarget = new THREE.WebGLRenderTarget(w, h, opts);
    this.blurTarget = new THREE.WebGLRenderTarget(w, h, opts);
  }

  private disposeTargets(): void {
    this.prepass?.depthTexture?.dispose();
    this.prepass?.dispose();
    this.aoTarget?.dispose();
    this.blurTarget?.dispose();
    this.prepass = this.aoTarget = this.blurTarget = null;
  }

  /** The WebGL context came back: targets are made again on the next pass. */
  contextRestored(): void {
    this.disposeTargets();
    this.off();
  }

  dispose(): void {
    this.disposeTargets();
    this.off();
    this.uniforms.map.value = null;
    this.normalMat.dispose();
    this.aoQuad.dispose();
    this.blurQuad.dispose();
  }
}
