// The sky, baked into a cube map: three's Preetham sky for a clear day, a CIE
// overcast dome for a cloudy one, and a night dome (a city glow at the
// horizon) that takes over through twilight. The cube is the background and,
// through a PMREM, the image based light. It is rebaked only when the light
// changes (the sun moved, the sky changed), never per frame.
//
// Everything in the cube is in "raw" sky units, the Preetham shader's own
// scale; `SKY_CDM2` turns them into luminance, and the caller scales that into
// scene units with LUX and the exposure (light/model.ts). The sun disc is left
// out of the cube: the directional light is the sun, so the image based light
// never counts it twice.

import * as THREE from "three";
import { Sky } from "three/examples/jsm/objects/Sky.js";
import { smoothstep } from "./model";

/**
 * cd/m2 per raw sky unit. Measured on this rig's Preetham sky (steps in the
 * light report): with the sun at 60 degrees the cosine weighted mean of the
 * upper hemisphere is 2.28 raw, and a clear sky at that height gives about
 * 15 000 lux on the ground from the sky alone: 15 000 / (pi * 2.28) = 2100.
 */
export const SKY_CDM2 = 2100;

/** Overcast zenith luminance in cd/m2 at full daylight (a bright overcast, about 16 000 lux). */
const OVERCAST_ZENITH_CDM2 = 6800;
/** Night sky over a lit city, cd/m2: what is left when twilight is over. */
const NIGHT_ZENITH_CDM2 = 0.1;
const NIGHT_HORIZON_CDM2 = 0.8;

/**
 * Twilight zenith luminance, cd/m2, for a sun altitude in degrees: about 400
 * at sunset, 15 at the Dusk preset (4 to 5 degrees down) and gone by
 * astronomical night. A fit to published twilight sky measurements; the
 * Preetham model alone turns black a degree or two under the horizon.
 */
export function twilightZenith(altitudeDeg: number): number {
  const h = Math.min(Math.max(altitudeDeg, -16), 2);
  return 400 * Math.pow(10, 0.32 * h);
}

/**
 * Light from a clear sky relative to the sky at a 50 degree sun, by sun
 * altitude: the cosine weighted mean radiance of this rig's Preetham sky,
 * measured at these heights (1.0 at 50 degrees). The photo sky, which cannot
 * change with the time, dims by it.
 */
const SKY_RATIO: [number, number][] = [
  [-3, 0.0011],
  [0, 0.012],
  [3, 0.057],
  [10, 0.236],
  [30, 0.604],
  [60, 1.148],
  [90, 1.708],
];

export function clearSkyRatio(altitudeDeg: number): number {
  const t = SKY_RATIO;
  if (altitudeDeg <= t[0][0]) return t[0][1];
  for (let i = 1; i < t.length; i++) {
    if (altitudeDeg <= t[i][0]) {
      const [a0, v0] = t[i - 1];
      const [a1, v1] = t[i];
      // Geometric between samples: the curve is close to exponential.
      return v0 * Math.pow(v1 / v0, (altitudeDeg - a0) / (a1 - a0));
    }
  }
  return t[t.length - 1][1];
}

export interface SkyInput {
  /** Unit vector towards the sun, world axes (x east, y up, z south). */
  sunWorld: THREE.Vector3;
  altitudeDeg: number;
  cloudy: boolean;
}

function domeMaterial(fragment: string, uniforms: Record<string, THREE.IUniform>): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    side: THREE.BackSide,
    depthWrite: false,
    depthTest: false,
    transparent: true,
    toneMapped: false,
    uniforms,
    vertexShader: /* glsl */ `
      varying vec3 vDir;
      void main() {
        vDir = normalize((modelMatrix * vec4(position, 0.0)).xyz);
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        gl_Position.z = gl_Position.w;
      }
    `,
    fragmentShader: fragment,
  });
}

/** CIE overcast sky: L = Lz (1 + 2 sin h) / 3 above the horizon, a dim ground below. */
const OVERCAST_FRAG = /* glsl */ `
  uniform vec3 zenith;
  uniform float opacity;
  varying vec3 vDir;
  void main() {
    float h = normalize(vDir).y;
    vec3 c = h >= 0.0 ? zenith * (1.0 + 2.0 * h) / 3.0 : zenith * mix(0.33, 0.12, min(-h * 6.0, 1.0));
    gl_FragColor = vec4(c, opacity);
  }
`;

/**
 * Twilight and night: a blue zenith, a paler horizon with a warm glow on the
 * side the sun went down, and under it all the city glow of a night sky.
 */
const NIGHT_FRAG = /* glsl */ `
  uniform vec3 horizon;
  uniform vec3 zenith;
  uniform vec3 glow;
  uniform vec3 sunDir;
  uniform float opacity;
  varying vec3 vDir;
  void main() {
    vec3 d = normalize(vDir);
    float h = d.y;
    vec2 dirXZ = normalize(d.xz + vec2(1e-5));
    vec2 toSun = normalize(sunDir.xz + vec2(1e-5));
    float toward = max(dot(dirXZ, toSun), 0.0);
    float band = exp(-max(h, 0.0) * 7.0);
    vec3 sky = mix(horizon, zenith, pow(max(h, 0.0), 0.45)) + glow * toward * toward * toward * band;
    vec3 c = h >= 0.0 ? sky : horizon * mix(0.55, 0.12, min(-h * 5.0, 1.0));
    gl_FragColor = vec4(c, opacity);
  }
`;

const toRaw = (cdm2: number) => cdm2 / SKY_CDM2;

export class SkyRig {
  readonly cube: THREE.WebGLCubeRenderTarget;
  private cubeCamera: THREE.CubeCamera;
  private skyScene = new THREE.Scene();
  private sky: Sky;
  private overcast: THREE.Mesh;
  private night: THREE.Mesh;
  private pmrem: THREE.PMREMGenerator | null = null;
  private envTarget: THREE.WebGLRenderTarget | null = null;
  private dirty = true;
  private last = "";

  constructor(size = 512) {
    this.cube = new THREE.WebGLCubeRenderTarget(size, { type: THREE.HalfFloatType, generateMipmaps: false });
    this.cube.texture.name = "sky";
    this.cubeCamera = new THREE.CubeCamera(0.1, 100, this.cube);

    this.sky = new Sky();
    this.sky.scale.setScalar(50);
    const u = this.sky.material.uniforms;
    u.turbidity.value = 2.6;
    u.rayleigh.value = 1.2;
    u.mieCoefficient.value = 0.004;
    u.mieDirectionalG.value = 0.8;
    // A few fair weather clouds, never moving: the sky is a still backdrop.
    u.cloudCoverage.value = 0.16;
    u.cloudDensity.value = 0.35;
    u.cloudElevation.value = 0.55;
    u.showSunDisc.value = 0;
    this.sky.renderOrder = 0;

    const dome = new THREE.SphereGeometry(40, 48, 24);
    this.overcast = new THREE.Mesh(
      dome,
      domeMaterial(OVERCAST_FRAG, { zenith: { value: new THREE.Color() }, opacity: { value: 0 } }),
    );
    this.overcast.renderOrder = 1;
    this.night = new THREE.Mesh(
      dome,
      domeMaterial(NIGHT_FRAG, {
        horizon: { value: new THREE.Color() },
        zenith: { value: new THREE.Color() },
        glow: { value: new THREE.Color() },
        sunDir: { value: new THREE.Vector3(0, -1, 0) },
        opacity: { value: 0 },
      }),
    );
    this.night.renderOrder = 2;
    this.skyScene.add(this.sky, this.overcast, this.night);
  }

  /** Sets the sky for a light. The cube is rebaked on the next `bake`. */
  set(input: SkyInput): void {
    const key = [input.sunWorld.x.toFixed(4), input.sunWorld.y.toFixed(4), input.sunWorld.z.toFixed(4), input.cloudy].join("|");
    if (key === this.last) return;
    this.last = key;
    const alt = input.altitudeDeg;
    this.sky.material.uniforms.sunPosition.value.copy(input.sunWorld);
    // Preetham falls apart a few degrees under the horizon; the night dome
    // covers it before it does.
    this.sky.visible = alt > -9;
    const overcastMat = this.overcast.material as THREE.ShaderMaterial;
    const day = smoothstep(-6, 8, alt);
    // Overcast brightness follows the sun's height like the clear sky does.
    const overcastZenith = toRaw(OVERCAST_ZENITH_CDM2) * day * (0.35 + 0.65 * smoothstep(0, 50, alt));
    overcastMat.uniforms.zenith.value.setRGB(overcastZenith * 0.97, overcastZenith, overcastZenith * 1.04);
    overcastMat.uniforms.opacity.value = input.cloudy ? 1 : 0;
    this.overcast.visible = input.cloudy && day > 0;
    // Twilight takes over from Preetham as the sun sets.
    const nightMat = this.night.material as THREE.ShaderMaterial;
    const nu = nightMat.uniforms;
    const nightK = 1 - smoothstep(-2.5, 1.5, alt);
    const tz = twilightZenith(alt) * (input.cloudy ? 0.45 : 1);
    const zen = toRaw(tz + NIGHT_ZENITH_CDM2);
    nu.zenith.value.setRGB(0.52 * zen, 0.68 * zen, 1.5 * zen);
    const hor = toRaw(tz * 2.2);
    const city = toRaw(NIGHT_HORIZON_CDM2);
    nu.horizon.value.setRGB(0.78 * hor + 1.15 * city, 0.8 * hor + 0.98 * city, 1.02 * hor + 0.78 * city);
    const warm = toRaw(tz * 3) * smoothstep(-9, -1, alt) * (input.cloudy ? 0.2 : 1);
    nu.glow.value.setRGB(1.6 * warm, 0.72 * warm, 0.26 * warm);
    nu.sunDir.value.copy(input.sunWorld);
    nu.opacity.value = nightK;
    this.night.visible = nightK > 0.001;
    this.dirty = true;
  }

  /** Rebakes the cube and its PMREM if the light changed. Returns true when it did. */
  bake(renderer: THREE.WebGLRenderer): boolean {
    if (!this.dirty) return false;
    this.dirty = false;
    const prevTarget = renderer.getRenderTarget();
    const prevXr = renderer.xr.enabled;
    const prevShadow = renderer.shadowMap.autoUpdate;
    renderer.xr.enabled = false;
    // A sky pass never touches the shadow map.
    renderer.shadowMap.autoUpdate = false;
    const needsUpdate = renderer.shadowMap.needsUpdate;
    this.cubeCamera.update(renderer, this.skyScene);
    renderer.shadowMap.needsUpdate = needsUpdate;
    renderer.shadowMap.autoUpdate = prevShadow;
    this.pmrem ??= new THREE.PMREMGenerator(renderer);
    const next = this.pmrem.fromCubemap(this.cube.texture, this.envTarget ?? undefined);
    this.envTarget = next;
    renderer.xr.enabled = prevXr;
    renderer.setRenderTarget(prevTarget);
    return true;
  }

  /** Forces a rebake, after the WebGL context came back. */
  invalidate(): void {
    this.dirty = true;
  }

  get background(): THREE.Texture {
    return this.cube.texture;
  }

  /** The PMREM for image based light. Null until the first bake. */
  get environment(): THREE.Texture | null {
    return this.envTarget?.texture ?? null;
  }

  dispose(): void {
    this.cube.dispose();
    this.envTarget?.dispose();
    this.envTarget = null;
    this.pmrem?.dispose();
    this.pmrem = null;
    this.sky.geometry.dispose();
    this.sky.material.dispose();
    this.overcast.geometry.dispose();
    (this.overcast.material as THREE.Material).dispose();
    (this.night.material as THREE.Material).dispose();
  }
}
