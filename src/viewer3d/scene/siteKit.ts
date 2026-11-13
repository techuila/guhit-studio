// The site kit in the 3D view (lane L5). Gates, hedges and lamp posts are
// catalog objects (`AssetCategory::Site` and `Plant`), drawn by assets.ts
// like any object so they are picked, cached and cut away the same way;
// paving is a deck slab with the paving material (slabs.ts). This scene
// hook, called by buildScene after the placed objects, is for site parts
// that are not one object; there are none yet. In its own file because
// scene/kit.ts is the mesh construction kit.

import type { SceneHost } from "./buildScene";

/** Scene hook for site parts that are not objects. Builds nothing yet. */
export function buildSiteKit(_host: SceneHost): void {}
