import * as THREE from "three";
import { describe, expect, it } from "vitest";
import type { DocState } from "../../contract/bindings";
import fixture from "../../../fixtures/sample-bungalow.docstate.json";
import { buildScene } from "../scene/buildScene";
import { MaterialLibrary } from "../scene/materials";
import { DOOR_SWING_OPEN_RAD, type DoorLeafTag } from "../scene/openings";
import { DOOR_OPEN_MM, DoorSwing, doorsOf, doorWantsOpen, DOORWAY_MM, type DoorInfo } from "./doors";

const doc = fixture as unknown as DocState;
const NORTH = Math.PI / 2;
const SOUTH = -Math.PI / 2;

// The front door: 900 wide, centered at (1500, 0) on the south wall.
const front = (): DoorInfo => {
  const d = doorsOf(doc).find((x) => Math.abs(x.center.y) < 1);
  if (!d) throw new Error("fixture changed");
  return d;
};

describe("doorWantsOpen", () => {
  const door = front();

  it("opens within 1.2 m when the walker faces the doorway", () => {
    expect(doorWantsOpen(door, { x: 1500, y: -1100, yaw: NORTH }, false)).toBe(true);
    expect(doorWantsOpen(door, { x: 1500, y: -(DOOR_OPEN_MM + 50), yaw: NORTH }, false)).toBe(false);
    // Roughly toward it: 60 degrees off still counts, 80 does not.
    expect(doorWantsOpen(door, { x: 1500, y: -1000, yaw: NORTH + (60 * Math.PI) / 180 }, false)).toBe(true);
    expect(doorWantsOpen(door, { x: 1500, y: -1000, yaw: NORTH + (80 * Math.PI) / 180 }, false)).toBe(false);
  });

  it("stays shut for a walker nearby looking away", () => {
    expect(doorWantsOpen(door, { x: 1500, y: -900, yaw: SOUTH }, false)).toBe(false);
  });

  it("opens in the doorway whichever way the walker looks", () => {
    expect(doorWantsOpen(door, { x: 1500, y: 120, yaw: 0 }, false)).toBe(true);
    expect(doorWantsOpen(door, { x: 1300, y: -DOORWAY_MM + 10, yaw: SOUTH }, false)).toBe(true);
  });

  it("stays open a little further out, and closes behind the walker or past 1.4 m", () => {
    expect(doorWantsOpen(door, { x: 1500, y: -1300, yaw: NORTH }, true)).toBe(true);
    expect(doorWantsOpen(door, { x: 1500, y: -1500, yaw: NORTH }, true)).toBe(false);
    // Walked through and on: the door is behind.
    expect(doorWantsOpen(door, { x: 1500, y: 700, yaw: NORTH }, true)).toBe(false);
  });
});

describe("DoorSwing", () => {
  const leavesOf = (root: THREE.Object3D) => {
    const out: { obj: THREE.Object3D; tag: DoorLeafTag; id: string }[] = [];
    root.traverse((o) => {
      if (o.userData.doorLeaf) out.push({ obj: o, tag: o.userData.doorLeaf as DoorLeafTag, id: o.userData.elementId as string });
    });
    return out;
  };

  it("keeps each door leaf apart from its frame, ajar outside walk mode", () => {
    const built = buildScene(doc, new MaterialLibrary(), { cutaway: false, activeLevelId: null });
    const leaves = leavesOf(built.root);
    expect(leaves).toHaveLength(2);
    for (const l of leaves) {
      expect(l.tag.kind).toBe("swing");
      expect(Math.abs(l.obj.rotation.y)).toBeCloseTo((32 * Math.PI) / 180, 6);
      expect(l.obj.children.some((c) => (c as THREE.Mesh).isMesh)).toBe(true);
    }
    built.kit.dispose();
  });

  it("closes every leaf while walking, opens the one the walker comes up to and closes it behind", () => {
    const built = buildScene(doc, new MaterialLibrary(), { cutaway: false, activeLevelId: null });
    const doors = new DoorSwing();
    doors.attach(built.root, doc);
    const level = doc.project.levels[0].id;
    const door = front();
    const leaf = leavesOf(built.root).find((l) => l.id === door.id)!;
    let t = 1000;
    doors.setWalking(true, t);
    expect(doors.sample(t + 1).animating).toBe(true);
    t += 1000;
    let s = doors.sample(t);
    expect(s.moved).toBe(true);
    expect(s.animating).toBe(false);
    expect(leaf.obj.rotation.y).toBeCloseTo(0, 6);

    doors.update({ x: 1500, y: -1000, yaw: NORTH }, level, t);
    expect(doors.stats().open).toEqual([door.id]);
    t += 1000;
    doors.sample(t);
    expect(Math.abs(leaf.obj.rotation.y)).toBeCloseTo(DOOR_SWING_OPEN_RAD, 6);

    // A rebuild while walking keeps the open door open, with no animation.
    const again = buildScene(doc, new MaterialLibrary(), { cutaway: false, activeLevelId: null });
    doors.attach(again.root, doc);
    const leaf2 = leavesOf(again.root).find((l) => l.id === door.id)!;
    expect(Math.abs(leaf2.obj.rotation.y)).toBeCloseTo(DOOR_SWING_OPEN_RAD, 6);
    expect(doors.sample(t + 1).animating).toBe(false);

    // Through and on: it closes behind.
    doors.update({ x: 1500, y: 800, yaw: NORTH }, level, t);
    expect(doors.stats().open).toEqual([]);
    t += 1000;
    doors.sample(t);
    expect(leaf2.obj.rotation.y).toBeCloseTo(0, 6);

    // Leaving walk mode puts the leaves back ajar.
    doors.setWalking(false, t);
    t += 1000;
    doors.sample(t);
    expect(Math.abs(leaf2.obj.rotation.y)).toBeCloseTo((32 * Math.PI) / 180, 6);
    // And nothing is left moving: no frames wanted.
    expect(doors.sample(t + 1000)).toEqual({ moved: false, animating: false });
    built.kit.dispose();
    again.kit.dispose();
  });

  it("ignores doors on another level than the walker's", () => {
    const built = buildScene(doc, new MaterialLibrary(), { cutaway: false, activeLevelId: null });
    const doors = new DoorSwing();
    doors.attach(built.root, doc);
    doors.setWalking(true, 0);
    doors.update({ x: 1500, y: -1000, yaw: NORTH }, "another-level", 10);
    expect(doors.stats().open).toEqual([]);
    built.kit.dispose();
  });
});
