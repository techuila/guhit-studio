// vitest runs in a plain Node environment (no jsdom in this project), and
// GLTFExporter/GLTFLoader both read and write through Blob + FileReader.
// Node has a global Blob but no FileReader, so a tiny Node-backed shim
// stands in for it here. Production code never needs this: the real app
// runs inside the Tauri webview, a real browser environment.
import { beforeAll, describe, expect, it } from "vitest";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import type { DocState } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { buildExportGroup, buildObjMtl, exportDAE, exportGLB, exportOBJ } from "./exportScene";

// No `@types/node` in this project: read Node's `Buffer` off `globalThis`.
type NodeBufferCtor = {
  from(data: ArrayBuffer | string, encoding?: string): { toString(encoding: string): string; buffer: ArrayBuffer; byteOffset: number; byteLength: number };
};
const nodeBuffer = (globalThis as unknown as { Buffer: NodeBufferCtor }).Buffer;

class NodeFileReader {
  result: ArrayBuffer | string | null = null;
  onloadend: (() => void) | null = null;
  readAsArrayBuffer(blob: Blob): void {
    void blob.arrayBuffer().then((buf) => {
      this.result = buf;
      this.onloadend?.();
    });
  }
  readAsDataURL(blob: Blob): void {
    void blob.arrayBuffer().then((buf) => {
      const base64 = nodeBuffer.from(buf).toString("base64");
      this.result = `data:${blob.type || "application/octet-stream"};base64,${base64}`;
      this.onloadend?.();
    });
  }
}

beforeAll(() => {
  if (typeof (globalThis as { FileReader?: unknown }).FileReader === "undefined") {
    (globalThis as unknown as { FileReader: unknown }).FileReader = NodeFileReader;
  }
});

const doc = fixture as unknown as DocState;

function decodeDataUrl(dataUrl: string): Uint8Array {
  const comma = dataUrl.indexOf(",");
  const base64 = dataUrl.slice(comma + 1);
  const binary = nodeBuffer.from(base64, "base64");
  return new Uint8Array(binary.buffer, binary.byteOffset, binary.byteLength);
}

function textOfDataUrl(dataUrl: string): string {
  return new TextDecoder().decode(decodeDataUrl(dataUrl));
}

/** Balanced-tag check: good enough to confirm hand-written XML is well-formed without a DOM parser. */
function isWellFormedXml(xml: string): boolean {
  const tagRe = /<\/?([a-zA-Z_][\w.:-]*)[^>]*>/g;
  const stack: string[] = [];
  let m: RegExpExecArray | null;
  let sawElement = false;
  while ((m = tagRe.exec(xml))) {
    const raw = m[0];
    const name = m[1];
    if (raw.startsWith("<?") || raw.startsWith("<!")) continue;
    if (raw.startsWith("</")) {
      if (stack.pop() !== name) return false;
    } else if (!raw.endsWith("/>")) {
      stack.push(name);
      sawElement = true;
    } else {
      sawElement = true;
    }
  }
  return sawElement && stack.length === 0;
}

function countMeshes(root: THREE.Object3D): number {
  let n = 0;
  root.traverse((o) => {
    if ((o as THREE.Mesh).isMesh) n++;
  });
  return n;
}

describe("buildExportGroup", () => {
  it("carries only groups and meshes: no helpers, lights, grid, ground or camera", () => {
    const { group, dispose } = buildExportGroup(doc);
    try {
      let stray = 0;
      group.traverse((o) => {
        if (!(o as THREE.Mesh).isMesh && !(o as THREE.Group).isGroup) stray++;
      });
      expect(stray).toBe(0);
      expect(countMeshes(group)).toBeGreaterThan(0);
    } finally {
      dispose();
    }
  });

  it("drops AI-proposal ghosts and never bakes a cutaway clip", () => {
    const { group, dispose } = buildExportGroup(doc);
    try {
      expect(group.getObjectByName("ghosts")).toBeUndefined();
      group.traverse((o) => {
        const mesh = o as THREE.Mesh;
        if (!mesh.isMesh || Array.isArray(mesh.material)) return;
        expect(mesh.material.clippingPlanes).toBeFalsy();
      });
    } finally {
      dispose();
    }
  });

  it("names and groups every renderable element as <kind>-<elementId> under a per-kind node", () => {
    const { group, dispose } = buildExportGroup(doc);
    try {
      const names = new Set<string>();
      group.traverse((o) => names.add(o.name));
      const renderable = doc.project.elements.filter((e) =>
        (["wall", "opening", "room", "column", "stair", "asset"] as const).includes(
          e.kind as "wall" | "opening" | "room" | "column" | "stair" | "asset",
        ),
      );
      for (const e of renderable) {
        expect(names.has(`${e.kind}-${e.id}`)).toBe(true);
      }
      expect(group.getObjectByName("Walls")).toBeTruthy();
      expect(group.getObjectByName("Openings")).toBeTruthy();
    } finally {
      dispose();
    }
  });
});

describe("exportGLB", () => {
  it("writes a valid GLB header (magic glTF, version 2) with a JSON chunk listing every mesh", async () => {
    const { group, dispose } = buildExportGroup(doc);
    let result: { data: string; extension: string };
    let meshCount: number;
    let uniqueGeomMat: number;
    try {
      meshCount = countMeshes(group);
      // Several elements share one unit BoxGeometry/CylinderGeometry (`Kit`, src/viewer3d/scene/kit.ts)
      // scaled per instance, and GLTFExporter reuses one glTF "mesh" per distinct
      // (geometry, material) pair across nodes, same as it would for any glTF scene.
      const seen = new Set<string>();
      group.traverse((o) => {
        const mesh = o as THREE.Mesh;
        if (!mesh.isMesh) return;
        const mat = Array.isArray(mesh.material) ? mesh.material[0] : mesh.material;
        seen.add(`${mesh.geometry.uuid}|${mat.uuid}`);
      });
      uniqueGeomMat = seen.size;
      result = await exportGLB(group);
    } finally {
      dispose();
    }
    expect(result.extension).toBe("glb");
    const bytes = decodeDataUrl(result.data);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);

    expect(String.fromCharCode(bytes[0], bytes[1], bytes[2], bytes[3])).toBe("glTF");
    expect(view.getUint32(4, true)).toBe(2); // version

    const jsonChunkLength = view.getUint32(12, true);
    const jsonChunkType = view.getUint32(16, true);
    expect(jsonChunkType).toBe(0x4e4f534a); // "JSON"
    const json = JSON.parse(new TextDecoder().decode(bytes.slice(20, 20 + jsonChunkLength))) as {
      meshes?: unknown[];
      nodes?: { mesh?: number }[];
    };
    expect(Array.isArray(json.meshes)).toBe(true);
    expect(json.meshes?.length).toBe(uniqueGeomMat);
    // Every element instance still gets its own node, even when it reuses a mesh.
    const nodesWithMesh = (json.nodes ?? []).filter((n) => typeof n.mesh === "number").length;
    expect(nodesWithMesh).toBe(meshCount);
  });

  it("round trips through GLTFLoader with a bounding box matching the live scene within 1 mm", async () => {
    const { group, dispose } = buildExportGroup(doc);
    const liveBox = new THREE.Box3().setFromObject(group);
    let result: { data: string; extension: string };
    try {
      result = await exportGLB(group);
    } finally {
      dispose();
    }
    const bytes = decodeDataUrl(result.data);
    const loader = new GLTFLoader();
    const scene = await new Promise<THREE.Group>((resolve, reject) => {
      const buf = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
      loader.parse(buf, "", (gltf) => resolve(gltf.scene), reject);
    });
    const loadedBox = new THREE.Box3().setFromObject(scene);
    const TOL_M = 0.001; // 1 mm
    expect(Math.abs(loadedBox.min.x - liveBox.min.x)).toBeLessThan(TOL_M);
    expect(Math.abs(loadedBox.min.y - liveBox.min.y)).toBeLessThan(TOL_M);
    expect(Math.abs(loadedBox.min.z - liveBox.min.z)).toBeLessThan(TOL_M);
    expect(Math.abs(loadedBox.max.x - liveBox.max.x)).toBeLessThan(TOL_M);
    expect(Math.abs(loadedBox.max.y - liveBox.max.y)).toBeLessThan(TOL_M);
    expect(Math.abs(loadedBox.max.z - liveBox.max.z)).toBeLessThan(TOL_M);
  });
});

describe("exportOBJ", () => {
  it("writes v/f lines, a meter unit comment, and no NaN", () => {
    const { group, dispose } = buildExportGroup(doc);
    let result: { data: string; extension: string };
    try {
      result = exportOBJ(group);
    } finally {
      dispose();
    }
    expect(result.extension).toBe("obj");
    const text = textOfDataUrl(result.data);
    expect(text).toContain("# units: meters");
    expect(text).toMatch(/^v -?\d/m);
    expect(text).toMatch(/^f \d/m);
    expect(text).not.toMatch(/NaN/);
  });

  it("writes a companion MTL with a color per distinct material", () => {
    const { group, dispose } = buildExportGroup(doc);
    let mtl: string;
    let obj: string;
    try {
      ({ obj, mtl } = buildObjMtl(group));
    } finally {
      dispose();
    }
    expect(obj).toContain("mtllib model.mtl");
    expect(mtl).toMatch(/newmtl mat-[0-9a-f]{6}/);
    expect(mtl).toMatch(/^Kd [\d.]+ [\d.]+ [\d.]+$/m);
    expect(mtl).not.toMatch(/NaN/);
  });
});

describe("exportDAE", () => {
  it("writes well-formed XML with a Z_UP up axis and meter unit", () => {
    const { group, dispose } = buildExportGroup(doc);
    let result: { data: string; extension: string };
    try {
      result = exportDAE(group);
    } finally {
      dispose();
    }
    expect(result.extension).toBe("dae");
    const xml = textOfDataUrl(result.data);
    expect(xml.startsWith("<?xml")).toBe(true);
    expect(xml).toContain("<up_axis>Z_UP</up_axis>");
    expect(xml).toContain('<unit name="meter" meter="1"/>');
    expect(isWellFormedXml(xml)).toBe(true);
  });

  it("bakes the Y-up to Z-up rotation into the vertex data, not just the up_axis label", () => {
    // An asymmetric box (2 x 4 x 6, x/y/z) makes each axis unambiguous: in the
    // live (Y-up) scene x spans 2, y (up) spans 4, z spans 6. Y-up -> Z-up is
    // x unchanged, new y = -old z, new z = old y, so the Z-up DAE must show
    // x spanning 2, y spanning 6, z (now up) spanning 4.
    const probe = new THREE.Group();
    const geo = new THREE.BoxGeometry(2, 4, 6);
    probe.add(new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color: "#ffffff" })));
    const { data } = exportDAE(probe);
    const xml = textOfDataUrl(data);
    const floats = /float_array[^>]*>([^<]+)</.exec(xml)?.[1].trim().split(/\s+/).map(Number) ?? [];
    const axis = (i: number) => floats.filter((_, k) => k % 3 === i);
    const span = (vals: number[]) => Math.max(...vals) - Math.min(...vals);
    expect(span(axis(0))).toBeCloseTo(2, 3); // x unchanged
    expect(span(axis(1))).toBeCloseTo(6, 3); // new y = -old z
    expect(span(axis(2))).toBeCloseTo(4, 3); // new z = old y (now up)
  });
});
