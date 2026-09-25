// A full screen pass that can be compiled ahead, off the main thread
// (three's compileAsync with KHR_parallel_shader_compile). A shader compiled
// on first draw would stall the page, and with it the live view.

import * as THREE from "three";

const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);

export class ScreenPass {
  readonly mesh: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>;

  constructor(material: THREE.ShaderMaterial) {
    this.mesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), material);
    this.mesh.frustumCulled = false;
  }

  get material(): THREE.ShaderMaterial {
    return this.mesh.material;
  }

  /**
   * Compiles the pass for drawing into `target` (null: the canvas), off the
   * main thread where the browser can. The program depends on the target
   * (tone mapping and color space), so it must be the one drawn into.
   */
  compile(r: THREE.WebGLRenderer, target: THREE.WebGLRenderTarget | null): Promise<unknown> {
    const prev = r.getRenderTarget();
    r.setRenderTarget(target);
    const done = r.compileAsync(this.mesh, camera);
    r.setRenderTarget(prev);
    return done;
  }

  render(r: THREE.WebGLRenderer): void {
    r.render(this.mesh, camera);
  }

  dispose(): void {
    this.mesh.geometry.dispose();
    this.mesh.material.dispose();
  }
}

/**
 * Compiles every material of a scene for drawing into `target`, off the main
 * thread where the browser can.
 */
export function compileScene(r: THREE.WebGLRenderer, scene: THREE.Scene, cam: THREE.Camera, target: THREE.WebGLRenderTarget | null): Promise<unknown> {
  const prev = r.getRenderTarget();
  r.setRenderTarget(target);
  const done = r.compileAsync(scene, cam);
  r.setRenderTarget(prev);
  return done;
}
