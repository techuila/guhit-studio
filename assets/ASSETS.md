# Asset Pack

CC0 asset pack for the 3D viewer: HDRI sky, PBR materials, catalog models.
Built by `scripts/assets-build.mjs` from three approved CC0 sources:
[Poly Haven](https://polyhaven.com), [ambientCG](https://ambientcg.com), and the
[Kenney Furniture Kit](https://kenney.nl/assets/furniture-kit).

Rebuild: `node scripts/assets-build.mjs` (idempotent, skips cached downloads in `assets/raw/`).

## License

Every asset in this pack is CC0 1.0 (public domain dedication): no attribution required, free for commercial use, modification and redistribution. Full text: https://creativecommons.org/publicdomain/zero/1.0/

## HDRI

| Field | Value |
|---|---|
| Asset | Alps Field (`alps_field`) |
| Source | https://polyhaven.com/a/alps_field |
| License | CC0 1.0 |
| File | `hdri/sky.hdr` (1.54 MB) |
| Preview | `hdri/sky-preview.jpg` (25.5 KB, 512x256 JPG) |
| Sun direction | unknown (Poly Haven's API does not publish sun azimuth/altitude; tags: sun, high-contrast, partly-cloudy) |

## Materials

Physical tile sizes match the UV-repeat convention already used by the built-in
procedural patterns in `src/viewer3d/scene/materials.ts` (`repeat = 1 / size`), so a
later agent can swap in these textures without re-deriving tiling scale.

| Preset | Source | Size (mm) | Maps | Bytes |
|---|---|---|---|---|
| `mat-chb-painted` | [PaintedPlaster017](https://ambientcg.com/a/PaintedPlaster017) | 1000x1000 | color, normal, roughness | 222.1 KB |
| `mat-chb-bare` | [Bricks032](https://ambientcg.com/a/Bricks032) | 800x400 | color, normal, roughness | 620.9 KB |
| `mat-concrete-fairface` | [Concrete030](https://ambientcg.com/a/Concrete030) | 1200x600 | color, normal, roughness | 425.4 KB |
| `mat-floor-concrete` | [Concrete048](https://ambientcg.com/a/Concrete048) | 1200x600 | color, normal, roughness | 480.7 KB |
| `mat-tile-ceramic` | [Tiles107](https://ambientcg.com/a/Tiles107) | 4800x4800 | color, normal, roughness | 177.4 KB |
| `mat-tile-granite` | [Granite001A](https://ambientcg.com/a/Granite001A) | 600x600 | color, normal, roughness | 592.0 KB |
| `mat-floor-laminate` | [WoodFloor051](https://ambientcg.com/a/WoodFloor051) | 1800x600 | color, normal, roughness | 299.8 KB |
| `mat-wood-cladding` | [Wood092](https://ambientcg.com/a/Wood092) | 1800x600 | color, normal, roughness | 201.1 KB |
| `mat-roof-longspan` | [CorrugatedSteel009](https://ambientcg.com/a/CorrugatedSteel009) | 15000x7500 | color, normal, roughness | 230.0 KB |
| `mat-roof-gi` | [CorrugatedSteel005](https://ambientcg.com/a/CorrugatedSteel005) | 760x760 | color, normal, roughness | 396.0 KB |
| `mat-roof-clay-tile` | [RoofingTiles006](https://ambientcg.com/a/RoofingTiles006) | 600x660 | color, normal, roughness | 386.2 KB |
| `mat-paint-warm-white` | none (flat color) | - | - | 0 |
| `mat-paint-sage` | none (flat color) | - | - | 0 |
| `mat-aluminum-frame` | none (flat color) | - | - | 0 |
| `mat-steel` | none (flat color) | - | - | 0 |
| `mat-wood-door` | none (flat color) | - | - | 0 |
| `mat-roof-concrete-deck` | none (flat color) | - | - | 0 |
| `mat-glass-clear` | none (flat color) | - | - | 0 |

Presets with no texture (flat `MeshStandardMaterial` color, matching `pattern: none`
in `crates/guhit-model/src/defaults.rs`): `mat-paint-warm-white`, `mat-paint-sage`, `mat-aluminum-frame`, `mat-steel`, `mat-wood-door`, `mat-roof-concrete-deck`, `mat-glass-clear`.

## Models

Orientation: every source checked (the full Kenney kit sample plus all three Poly Haven
hero swaps) already places its "back" at local -Z, matching this app's convention
(`docs/CONTRACT.md`: local +y in plan is the back; `src/viewer3d/scene/assets.ts` maps
that to three.js -z). Verified by: (1) visual check of Kenney's isometric preview renders
for `bedDouble` and `loungeSofa` (headboard / backrest sit on the -Z side in both), and
(2) a geometric heuristic across the rest of the kit and the Poly Haven models (the
centroid of each mesh's upper 40% of vertices, which tends to sit over the tall/detailed
side of asymmetric objects, consistently falls on the -Z side). No rotation was applied to
any model as a result. Every model's origin is the footprint center, underside at y=0.

| Catalog key | Source | Bbox (mm, w x d x h) | Triangles | Bytes |
|---|---|---|---|---|
| `bed-single` | Kenney: bedSingle.glb | 920x1900x500 | 214 | 7.6 KB |
| `bed-double` | Kenney: bedDouble.glb | 1370x1900x500 | 264 | 8.2 KB |
| `bed-queen` | Kenney: bedDouble.glb | 1520x2030x500 | 264 | 8.2 KB |
| `wardrobe` | Kenney: bookcaseClosedWide.glb | 1200x600x2100 | 372 | 5.1 KB |
| `sofa-3` | Kenney: loungeSofaLong.glb | 2100x900x800 | 186 | 4.8 KB |
| `sofa-2` | Kenney: loungeSofa.glb | 1500x900x800 | 128 | 4.2 KB |
| `armchair` | Poly Haven: modern_arm_chair_01 | 850x850x800 | 7029 | 705.2 KB |
| `coffee-table` | Poly Haven: modern_coffee_table_01 | 1100x600x420 | 3832 | 285.4 KB |
| `tv-console` | Kenney: cabinetTelevision.glb | 1600x450x500 | 154 | 3.2 KB |
| `dining-4` | Kenney: table.glb + 4x chair.glb (composed) | 1200x920x880 | 800 | 8.6 KB |
| `dining-6` | Kenney: table.glb + 6x chair.glb (composed) | 1800x1020x880 | 1140 | 11.5 KB |
| `desk` | Kenney: desk.glb | 1200x600x750 | 198 | 5.0 KB |
| `wc` | Kenney: toilet.glb | 400x700x780 | 226 | 7.9 KB |
| `lavatory` | Kenney: bathroomSink.glb | 500x420x200 | 316 | 6.9 KB |
| `shower` | Kenney: shower.glb | 900x900x50 | 806 | 12.5 KB |
| `bathtub` | Kenney: bathtub.glb | 1500x750x550 | 602 | 9.6 KB |
| `kitchen-counter` | Kenney: kitchenCabinet.glb | 1800x600x900 | 114 | 5.3 KB |
| `kitchen-sink` | Kenney: kitchenSink.glb | 1200x600x900 | 318 | 5.7 KB |
| `range` | Kenney: kitchenStove.glb | 600x600x900 | 830 | 12.1 KB |
| `refrigerator` | Kenney: kitchenFridge.glb | 700x700x1750 | 228 | 7.6 KB |
| `washing-machine` | Kenney: washer.glb | 600x600x850 | 720 | 12.3 KB |
| `plant-pot` | Poly Haven: potted_plant_01 | 500x500x1187 | 44055 | 1.34 MB |
| `tree` | none (procedural fallback in the viewer) | - | - | - |
| `car-sedan` | none (procedural fallback in the viewer) | - | - | - |

Not covered by any CC0 source (procedural fallback in `src/viewer3d/scene/assets.ts` stays in use):

- `tree`: Kenney Furniture Kit has no tree; Poly Haven's tree models (e.g. island_tree_01) carry a ~58 MB shared .bin far outside budget.
- `car-sedan`: None of the three approved sources publish a CC0 vehicle model.

Kept as Kenney instead of a Poly Haven hero swap: `sofa-3`, `sofa-2`, `bed-single`, `bed-double`,
`bed-queen`. Poly Haven's only sofa models (`sofa_02`, `sofa_03`, `Sofa_01`) and bed models
(`GothicBed_01`, `old_bed_frame`, `vintage_day_bed`) are vintage/Victorian pieces or a bare frame
with no mattress; none fit the app's tropical-modern / modern-minimal render styles
(`crates/guhit-model/src/defaults.rs` `render_styles()`) better than Kenney's plain kit pieces.

## Kenney palette recolor

The Kenney Furniture Kit's stock palette includes one saturated color, a salmon
(`carpet` material, raw `baseColorFactor` ~`[0.943, 0.367, 0.343]`, `#f15e57` read as sRGB
hex) reused as-is for both the sofa upholstery and the bed mattress/bedding top. Every other
Kenney material this pack uses (wood, the metal greys, the near-white fixture/appliance
tones, the pale glass) was already muted enough to keep unchanged. Applied in
`scripts/assets-build.mjs` (`recolorKenneyMaterials`) via `@gltf-transform/core`'s `NodeIO`:
each Kenney model's materials are read, matched against the salmon value, and their
`baseColorFactor` replaced, before the optimize pass. Poly Haven models are never touched.

| Catalog keys | Original | New | Role |
|---|---|---|---|
| `sofa-2`, `sofa-3` | `#f15e57` salmon (`carpet`) | `#8f8a82` warm grey | upholstery |
| `bed-single`, `bed-double`, `bed-queen` | `#f15e57` salmon (`carpet`) | `#eeeae2` off-white | mattress / bedding |

Unchanged (already muted, kept as-is): wood tones (`wood` `#e59964`, `woodDark` `#ad744c`),
appliance/fixture whites and metal greys (`carpetWhite` `#f8ffff`, `metalLight` `#effaf4`,
`metal` `#bdd2d6`, `metalMedium` `#5e7777`, `metalDark` `#4e6363`, `_defaultMat` `#ffffff`), and
the pale glass (`glass` `#b2d3c4`).

