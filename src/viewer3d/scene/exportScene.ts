// Whole-scene export: GLB, OBJ, DAE. Serializes the CURRENT model (walls,
// openings, floors, roof, columns, stairs, assets, reference models) with no
// helpers, lights, grid, ground plane, ghosts, highlights or camera. Built
// from the same `buildScene` the live view uses, then cloned and reorganized
// so every mesh reads as "<kind>-<elementId>" grouped under a per-kind node
// (SketchUp's outliner). three.js scene units are meters; that is kept for
// every format (glTF wants meters, OBJ carries a unit comment, DAE states it
// in <asset><unit>).
//
// three ships GLTFExporter and OBJExporter (three/examples/jsm/exporters),
// but OBJExporter cannot write an MTL and there is no ColladaExporter in this
// three version, so OBJ and DAE are written here by hand: plain positions,
// normals, UVs and one material per distinct color, no external deps.

import * as THREE from "three";
import { GLTFExporter } from "three/examples/jsm/exporters/GLTFExporter.js";
import type { DocState, Element } from "../../contract/bindings";
import { buildScene } from "./buildScene";
import { MaterialLibrary } from "./materials";

const KIND_LABEL: Partial<Record<Element["kind"], string>> = {
  wall: "Walls",
  opening: "Openings",
  room: "Rooms",
  column: "Columns",
  stair: "Stairs",
  asset: "Assets",
  reference_model: "ReferenceModels",
};

function isDescendantOf(o: THREE.Object3D, ancestor: THREE.Object3D): boolean {
  for (let p: THREE.Object3D | null = o.parent; p; p = p.parent) if (p === ancestor) return true;
  return false;
}

/**
 * Builds a clean, disposable export group from the live document: no
 * helpers, camera or highlight state, every mesh grouped under a
 * "<Kind>/<kind>-<elementId>" node. `referenceModels`, when given, are the
 * engine's current reference-model mounts (loaded geometry or a placeholder
 * box), already positioned in the same world space `buildScene` uses.
 *
 * The CC0 pack is part of what is exported: furniture comes out as the GLB
 * model the viewer shows, and walls, floors and roofs carry the pack's color,
 * normal and roughness maps (GLTFExporter embeds them; OBJ and DAE carry the
 * material colors only, as before). `packModels` false exports the low-poly
 * forms instead, matching the dev harness toggle.
 */
export function buildExportGroup(
  doc: DocState,
  referenceModels?: Map<string, THREE.Object3D>,
  opts?: { packModels?: boolean },
): { group: THREE.Group; dispose: () => void } {
  const lib = new MaterialLibrary();
  const built = buildScene(doc, lib, {
    cutaway: false,
    activeLevelId: null,
    ghostsRemoved: null,
    packModels: opts?.packModels !== false,
  });
  const source = built.root.clone(true);
  source.updateMatrixWorld(true);

  const ghosts = source.getObjectByName("ghosts");
  if (ghosts) ghosts.removeFromParent();
  const roofNode = source.getObjectByName("roof");

  const group = new THREE.Group();
  group.name = "export";
  const kindGroups = new Map<string, THREE.Group>();
  const kindGroup = (label: string): THREE.Group => {
    let g = kindGroups.get(label);
    if (!g) {
      g = new THREE.Group();
      g.name = label;
      group.add(g);
      kindGroups.set(label, g);
    }
    return g;
  };

  const kindOf = new Map<string, Element["kind"]>();
  for (const e of doc.project.elements) kindOf.set(e.id, e.kind);

  const meshes: THREE.Mesh[] = [];
  source.traverse((o) => {
    if ((o as THREE.Mesh).isMesh) meshes.push(o as THREE.Mesh);
  });

  const byElement = new Map<string, THREE.Mesh[]>();
  const other: THREE.Mesh[] = [];
  for (const mesh of meshes) {
    const id = mesh.userData.elementId as string | undefined;
    if (id) {
      const list = byElement.get(id) ?? [];
      list.push(mesh);
      byElement.set(id, list);
    } else {
      other.push(mesh);
    }
  }

  for (const [id, list] of byElement) {
    const kind = kindOf.get(id);
    const label = (kind && KIND_LABEL[kind]) || "Other";
    const parent = kindGroup(label);
    const name = `${kind ?? "element"}-${id}`;
    if (list.length === 1) {
      list[0].name = name;
      parent.attach(list[0]);
    } else {
      const wrap = new THREE.Group();
      wrap.name = name;
      parent.add(wrap);
      for (const mesh of list) wrap.attach(mesh);
    }
  }

  const counters = new Map<string, number>();
  const nextName = (kind: string): string => {
    const n = counters.get(kind) ?? 0;
    counters.set(kind, n + 1);
    return `${kind}-${n}`;
  };
  for (const mesh of other) {
    const isRoof = roofNode !== undefined && isDescendantOf(mesh, roofNode);
    const label = isRoof ? "Roof" : "Floors";
    mesh.name = nextName(isRoof ? "roof" : "floor-slab");
    kindGroup(label).attach(mesh);
  }

  if (referenceModels && referenceModels.size > 0) {
    const refs = kindGroup("ReferenceModels");
    for (const [id, obj] of referenceModels) {
      const clone = obj.clone(true);
      clone.updateMatrixWorld(true);
      clone.name = `reference_model-${id}`;
      refs.add(clone);
    }
  }

  return {
    group,
    dispose: () => {
      built.kit.dispose();
      lib.dispose();
    },
  };
}

// -------------------------------------------------------------------- utils

/** Node's `Buffer`, when running under vitest: no `@types/node` in this project, so it is read off `globalThis`. */
type NodeBufferCtor = {
  from(data: Uint8Array | string, encoding?: string): { toString(encoding: string): string };
};
const nodeBuffer = (globalThis as unknown as { Buffer?: NodeBufferCtor }).Buffer;

function bufferToBase64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  if (nodeBuffer) return nodeBuffer.from(bytes).toString("base64");
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  return btoa(binary);
}

function textToBase64(s: string): string {
  if (nodeBuffer) return nodeBuffer.from(s, "utf-8").toString("base64");
  return btoa(unescape(encodeURIComponent(s)));
}

function materialOf(m: THREE.Material | THREE.Material[]): THREE.MeshStandardMaterial {
  return (Array.isArray(m) ? m[0] : m) as THREE.MeshStandardMaterial;
}

/** A stable, readable name per distinct color: our materials carry no `.name`. */
function materialName(m: THREE.Material | THREE.Material[]): string {
  const mat = materialOf(m);
  const hex = mat.color ? mat.color.getHexString() : "d8d5cd";
  return `mat-${hex}`;
}

function colorOf(m: THREE.Material | THREE.Material[]): [number, number, number] {
  const mat = materialOf(m);
  return mat.color ? [mat.color.r, mat.color.g, mat.color.b] : [0.847, 0.835, 0.804];
}

const fmt = (n: number): string => (Number.isFinite(n) ? n.toFixed(5) : "0.00000");
const round5 = (n: number): number => (Number.isFinite(n) ? Math.round(n * 1e5) / 1e5 : 0);

function xmlEscape(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function sanitizeId(s: string): string {
  const cleaned = s.replace(/[^A-Za-z0-9_.-]/g, "_");
  return /^[A-Za-z_]/.test(cleaned) ? cleaned : `_${cleaned}`;
}

// --------------------------------------------------------------------- GLB

/** glTF is Y-up like the three.js scene, so no rotation: meters, binary, textures embedded. */
export async function exportGLB(group: THREE.Group): Promise<{ data: string; extension: string }> {
  const exporter = new GLTFExporter();
  const result = await exporter.parseAsync(group, { binary: true, onlyVisible: true, embedImages: true });
  const buf = result as ArrayBuffer;
  return { data: `data:model/gltf-binary;base64,${bufferToBase64(buf)}`, extension: "glb" };
}

// --------------------------------------------------------------------- OBJ

/**
 * OBJExporter (three/examples/jsm) cannot write material colors, so this
 * walks the geometry itself and writes a companion MTL alongside it. `obj`
 * is what `exportScene("obj")` returns (with a `mtllib model.mtl` line);
 * `mtl` is exposed for a caller able to write the second file (the dev
 * harness, or a future multi-file writer) since the store's `ExportScene`
 * contract returns a single `{ data, extension }` pair.
 */
export function buildObjMtl(group: THREE.Group): { obj: string; mtl: string } {
  group.updateMatrixWorld(true);
  const meshes: THREE.Mesh[] = [];
  group.traverse((o) => {
    if ((o as THREE.Mesh).isMesh) meshes.push(o as THREE.Mesh);
  });

  const objLines = ["# Guhit Studio scene export", "# units: meters", "mtllib model.mtl"];
  const mtlLines = ["# Guhit Studio materials"];
  const seen = new Set<string>();
  let vOff = 0;
  let vtOff = 0;
  let vnOff = 0;
  const normalMat = new THREE.Matrix3();
  const v = new THREE.Vector3();
  const n = new THREE.Vector3();
  const uvv = new THREE.Vector2();

  for (const mesh of meshes) {
    const geo = mesh.geometry;
    const pos = geo.getAttribute("position");
    if (!pos) continue;
    const nrm = geo.getAttribute("normal");
    const uv = geo.getAttribute("uv");
    const idx = geo.getIndex();
    const matName = materialName(mesh.material);
    if (!seen.has(matName)) {
      seen.add(matName);
      const [r, g, b] = colorOf(mesh.material);
      mtlLines.push(`newmtl ${matName}`, `Kd ${fmt(r)} ${fmt(g)} ${fmt(b)}`, `Ka ${fmt(r * 0.2)} ${fmt(g * 0.2)} ${fmt(b * 0.2)}`, "illum 1", "");
    }
    objLines.push(`o ${mesh.name || "mesh"}`, `usemtl ${matName}`);
    for (let i = 0; i < pos.count; i++) {
      v.fromBufferAttribute(pos, i).applyMatrix4(mesh.matrixWorld);
      objLines.push(`v ${fmt(v.x)} ${fmt(v.y)} ${fmt(v.z)}`);
    }
    if (uv) {
      for (let i = 0; i < uv.count; i++) {
        uvv.fromBufferAttribute(uv as THREE.BufferAttribute, i);
        objLines.push(`vt ${fmt(uvv.x)} ${fmt(uvv.y)}`);
      }
    }
    if (nrm) {
      normalMat.getNormalMatrix(mesh.matrixWorld);
      for (let i = 0; i < nrm.count; i++) {
        n.fromBufferAttribute(nrm, i).applyMatrix3(normalMat).normalize();
        objLines.push(`vn ${fmt(n.x)} ${fmt(n.y)} ${fmt(n.z)}`);
      }
    }
    const faceRef = (localIdx: number): string => {
      const j = localIdx + 1;
      const vi = vOff + j;
      if (uv && nrm) return `${vi}/${vtOff + j}/${vnOff + j}`;
      if (nrm) return `${vi}//${vnOff + j}`;
      if (uv) return `${vi}/${vtOff + j}`;
      return `${vi}`;
    };
    if (idx) {
      for (let i = 0; i < idx.count; i += 3) {
        objLines.push(`f ${faceRef(idx.getX(i))} ${faceRef(idx.getX(i + 1))} ${faceRef(idx.getX(i + 2))}`);
      }
    } else {
      for (let i = 0; i < pos.count; i += 3) objLines.push(`f ${faceRef(i)} ${faceRef(i + 1)} ${faceRef(i + 2)}`);
    }
    vOff += pos.count;
    if (uv) vtOff += uv.count;
    if (nrm) vnOff += nrm.count;
  }

  return { obj: `${objLines.join("\n")}\n`, mtl: `${mtlLines.join("\n")}\n` };
}

export function exportOBJ(group: THREE.Group): { data: string; extension: string } {
  const { obj } = buildObjMtl(group);
  return { data: `data:text/plain;base64,${textToBase64(obj)}`, extension: "obj" };
}

// --------------------------------------------------------------------- DAE

/**
 * SketchUp and DAE are Z-up; the three.js scene is Y-up. Instead of trusting
 * a reader to honor <up_axis>, the export group is rotated +90 deg about X
 * before its vertices are baked (matrixWorld), so the written geometry is
 * genuinely in the Z-up frame the <up_axis>Z_UP</up_axis> element declares.
 */
export function exportDAE(group: THREE.Group): { data: string; extension: string } {
  const zUp = new THREE.Group();
  zUp.rotation.x = Math.PI / 2;
  zUp.add(group.clone(true));
  zUp.updateMatrixWorld(true);

  const meshes: THREE.Mesh[] = [];
  zUp.traverse((o) => {
    if ((o as THREE.Mesh).isMesh) meshes.push(o as THREE.Mesh);
  });

  interface Mat {
    id: string;
    color: [number, number, number];
  }
  const materials: Mat[] = [];
  const matIndex = new Map<string, number>();
  const geomXml: string[] = [];
  const nodeXml: string[] = [];
  const v = new THREE.Vector3();

  meshes.forEach((mesh, i) => {
    const geo = mesh.geometry;
    const pos = geo.getAttribute("position");
    if (!pos) return;
    const idx = geo.getIndex();
    const matName = materialName(mesh.material);
    if (!matIndex.has(matName)) {
      matIndex.set(matName, materials.length);
      materials.push({ id: sanitizeId(matName), color: colorOf(mesh.material) });
    }
    const gid = `geom-${i}`;
    const floats: number[] = [];
    for (let k = 0; k < pos.count; k++) {
      v.fromBufferAttribute(pos, k).applyMatrix4(mesh.matrixWorld);
      floats.push(round5(v.x), round5(v.y), round5(v.z));
    }
    const tris: number[] = [];
    if (idx) for (let k = 0; k < idx.count; k++) tris.push(idx.getX(k));
    else for (let k = 0; k < pos.count; k++) tris.push(k);
    const triCount = tris.length / 3;
    const meshName = xmlEscape(mesh.name || gid);
    const matSym = `${xmlEscape(matName)}-sym`;
    geomXml.push(
      `<geometry id="${gid}" name="${meshName}"><mesh>` +
        `<source id="${gid}-pos"><float_array id="${gid}-pos-array" count="${floats.length}">${floats.join(" ")}</float_array>` +
        `<technique_common><accessor source="#${gid}-pos-array" count="${pos.count}" stride="3">` +
        `<param name="X" type="float"/><param name="Y" type="float"/><param name="Z" type="float"/>` +
        `</accessor></technique_common></source>` +
        `<vertices id="${gid}-vtx"><input semantic="POSITION" source="#${gid}-pos"/></vertices>` +
        `<triangles material="${matSym}" count="${triCount}">` +
        `<input semantic="VERTEX" source="#${gid}-vtx" offset="0"/><p>${tris.join(" ")}</p></triangles>` +
        `</mesh></geometry>`,
    );
    nodeXml.push(
      `<node id="node-${i}" name="${meshName}"><instance_geometry url="#${gid}">` +
        `<bind_material><technique_common><instance_material symbol="${matSym}" target="#${sanitizeId(matName)}"/></technique_common></bind_material>` +
        `</instance_geometry></node>`,
    );
  });

  const effectsXml = materials
    .map(
      (m) =>
        `<effect id="${m.id}-fx"><profile_COMMON><technique sid="common"><lambert>` +
        `<diffuse><color>${m.color.map((c) => c.toFixed(4)).join(" ")} 1</color></diffuse>` +
        `</lambert></technique></profile_COMMON></effect>`,
    )
    .join("");
  const materialsXml = materials.map((m) => `<material id="${m.id}" name="${m.id}"><instance_effect url="#${m.id}-fx"/></material>`).join("");

  const now = new Date().toISOString();
  const xml =
    `<?xml version="1.0" encoding="UTF-8"?>\n` +
    `<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">` +
    `<asset><created>${now}</created><modified>${now}</modified><unit name="meter" meter="1"/><up_axis>Z_UP</up_axis></asset>` +
    `<library_effects>${effectsXml}</library_effects>` +
    `<library_materials>${materialsXml}</library_materials>` +
    `<library_geometries>${geomXml.join("")}</library_geometries>` +
    `<library_visual_scenes><visual_scene id="Scene" name="Guhit Studio">${nodeXml.join("")}</visual_scene></library_visual_scenes>` +
    `<scene><instance_visual_scene url="#Scene"/></scene>` +
    `</COLLADA>\n`;

  return { data: `data:model/vnd.collada+xml;base64,${textToBase64(xml)}`, extension: "dae" };
}
