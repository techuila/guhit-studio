// Keeps the built form of elements that did not change between two rebuilds.
//
// `buildScene` still runs top to bottom on every revision, because walls,
// rooms, slabs and the roof all depend on derived data that the Rust engine
// recomputes as a whole. Assets do not: a chair is a function of its own
// fields plus the floor height it stands on. Rebuilding three hundred of them
// because one wall moved is the single most expensive thing the viewer does,
// so their groups are cached by a signature and handed back untouched.
//
// Cached geometries belong to the cache, never to the per-build `Kit`, so a
// rebuild that disposes its kit cannot pull them out from under a reused
// group. Eviction is deferred: a group that is still on screen (it is playing
// its exit fade) is only released once nothing holds it any more.

import type * as THREE from "three";

interface Entry {
  group: THREE.Group;
  geometries: THREE.BufferGeometry[];
  /** Not used by the last build. Released by `sweep` once it is unparented. */
  stale: boolean;
}

export class BuildCache {
  private entries = new Map<string, Entry>();
  private used = new Set<string>();

  /** Call before a build. */
  begin(): void {
    this.used.clear();
  }

  /** The cached group for `key`, marked as used, or null. */
  take(key: string): THREE.Group | null {
    const entry = this.entries.get(key);
    if (!entry) return null;
    entry.stale = false;
    this.used.add(key);
    return entry.group;
  }

  put(key: string, group: THREE.Group, geometries: THREE.BufferGeometry[]): void {
    this.entries.get(key)?.group.removeFromParent();
    this.release(this.entries.get(key));
    this.entries.set(key, { group, geometries, stale: false });
    this.used.add(key);
  }

  /** Call after a build: anything not used this time is up for release. */
  end(): void {
    for (const [key, entry] of this.entries) if (!this.used.has(key)) entry.stale = true;
  }

  /**
   * Releases every stale entry whose group is no longer in a scene. An element
   * that was removed keeps its meshes while it sinks and fades out, so this is
   * called again when that animation lands.
   */
  sweep(): void {
    for (const [key, entry] of this.entries) {
      if (!entry.stale || entry.group.parent) continue;
      this.release(entry);
      this.entries.delete(key);
    }
  }

  private release(entry: Entry | undefined): void {
    if (!entry) return;
    for (const g of entry.geometries) g.dispose();
    entry.geometries.length = 0;
  }

  size(): number {
    return this.entries.size;
  }

  dispose(): void {
    for (const entry of this.entries.values()) {
      entry.group.removeFromParent();
      this.release(entry);
    }
    this.entries.clear();
    this.used.clear();
  }
}
