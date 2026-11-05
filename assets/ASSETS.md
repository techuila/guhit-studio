# Asset Pack

CC0 asset pack for the 3D viewer: HDRI sky, PBR materials, catalog models.
Built by `scripts/assets-build.mjs` from three approved CC0 sources:
[Poly Haven](https://polyhaven.com), [ambientCG](https://ambientcg.com), and the
[Kenney Furniture Kit](https://kenney.nl/assets/furniture-kit).

Rebuild: `node scripts/assets-build.mjs [--raw <dir>]` (idempotent, skips cached downloads in the raw cache: `assets/raw/` by default, or `--raw` / `GUHIT_ASSETS_RAW`). A clean cache downloads about 440 MB, 214 MB of it the jacaranda tree.

The Kenney Furniture Kit zip has to be placed by hand on a clean cache: kenney.nl now hands it out only through a signed link made for a browser session, and its old media URL redirects to the asset page. Download it (v1.0, CC0) from https://kenney.nl/assets/furniture-kit into `<raw>/kenney/kenney_furniture-kit.zip`.

Check: `node scripts/assets-build.mjs --check` (no downloads, under a second; a full build runs it too). It fails when a file under `public/assets/pack/` is not in `manifest.json` or a listed file is missing, when an entry that ships a file is not CC0 1.0 or its source is not on polyhaven.com, ambientcg.com or kenney.nl, when a model key is not a catalog key in `crates/guhit-model/src/catalog.rs`, or when the pack is over 40 MB.

Pack size: 27085684 bytes (25.83 MB) of the 40 MB budget.

## License

Every asset in this pack is CC0 1.0 (public domain dedication): no attribution required, free for commercial use, modification and redistribution. Full text: https://creativecommons.org/publicdomain/zero/1.0/

## HDRI

| Field | Value |
|---|---|
| Asset | Pretoria Gardens (`pretoria_gardens`) |
| Source | https://polyhaven.com/a/pretoria_gardens |
| License | CC0 1.0 |
| File | `hdri/sky.hdr` (6.46 MB) |
| Background | `hdri/sky.jpg` (1.71 MB, 4096x2048 JPG) |
| Sun direction | found from the pixels at load time (scanHdri in src/viewer3d/engine/environment.ts); midday, high sun |

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
| `mat-ground-grass` | [Grass004](https://ambientcg.com/a/Grass004) | 1400x1400 | color, normal, roughness | 1.01 MB |
| `mat-paving-stone` | [PavingStones099](https://ambientcg.com/a/PavingStones099) | 2200x2200 | color, normal, roughness | 381.0 KB |
| `mat-paint-warm-white` | none (flat color) | - | - | 0 |
| `mat-paint-sage` | none (flat color) | - | - | 0 |
| `mat-aluminum-frame` | none (flat color) | - | - | 0 |
| `mat-steel` | none (flat color) | - | - | 0 |
| `mat-wood-door` | none (flat color) | - | - | 0 |
| `mat-roof-concrete-deck` | none (flat color) | - | - | 0 |
| `mat-glass-clear` | none (flat color) | - | - | 0 |

Presets with no texture (flat `MeshStandardMaterial` color, matching `pattern: none`
in `crates/guhit-model/src/defaults.rs`): `mat-paint-warm-white`, `mat-paint-sage`, `mat-aluminum-frame`, `mat-steel`, `mat-wood-door`, `mat-roof-concrete-deck`, `mat-glass-clear`.

`mat-paving-stone`, "Paving stones (site paving)". For deck slabs used as site paving (SlabKind::Deck with this material): driveways, carports, garden walks. Grey I-shaped interlocking concrete pavers, 200 x 157 mm at this tile size.

## Models

Every model's origin is the footprint center, underside at y=0, and its back sits at local -Z
(`docs/CONTRACT.md`: local +y in plan is the back; `src/viewer3d/scene/assets.ts` maps that to
three.js -z). How that was checked:

- Wave 1 (Kenney kit and the three Poly Haven heroes): already facing that way, no rotation. Checked on Kenney's isometric preview renders for `bedDouble` and `loungeSofa` (headboard and backrest on -Z) and with a geometric heuristic across the rest: the centroid of each mesh's upper 40% of vertices, which sits over the tall or detailed side of an asymmetric object, falls on -Z.
- Catalog v2 (Poly Haven scans): the same centroid, plus the tallest point on each half of the depth, the surface area in the front and back 15% of the depth, and flat-shaded side, top, front and back renders of each built GLB, compared with the Poly Haven thumbnail where those were unsure (the covered car, the picture frame, the mirror, the drawer cabinet, the cube shelves). Seat backs, the bench back, the stove's knobs and oven door, the sideboard's slatted doors, the drawer fronts, the car's hood and the frame and mirror faces all sit on +Z, so none of those turn. The one exception is `shelf-cube`: `wooden_display_shelves_01` is modeled with its open cubes on +X (its 1078 mm width runs along Z), so the build turns it -90 degrees about +Y. Turns are baked into the GLB: every v2 entry has `back_axis` -z. The wall items (`wall-art`, `mirror-wall`) keep their flat back board on -Z, the wall side.

### Wave 1 models

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

### Catalog v2 models

Photoscanned CC0 models from Poly Haven, one per new catalog key, normalized to the catalog size and optimized with meshopt and WebP textures. Textures are 1024 px for the large pieces and 512 px where noted (the small pieces, the chairs, the wall items and the decor): in the GPU a 1024 px map takes about 5.6 MB (RGBA plus mipmaps), a 512 px one 1.4 MB. No v2 model keeps an occlusion map of its own; the viewer draws its own ambient occlusion. The sets count their table and one chair, since every seat shares the chair mesh.

| Catalog key | Source | Bbox (mm, w x d x h) | Triangles | Bytes | Build notes |
|---|---|---|---|---|---|
| `sofa-leather` | [sofa_02](https://polyhaven.com/a/sofa_02) | 1810x820x710 | 2690 | 165.0 KB | - |
| `sofa-wood` | [chinese_sofa](https://polyhaven.com/a/chinese_sofa) | 2290x970x860 | 698 | 165.5 KB | - |
| `lounge-chair` | [mid_century_lounge_chair](https://polyhaven.com/a/mid_century_lounge_chair) | 1010x1190x1170 | 5800 | 119.0 KB | 512 px textures |
| `armchair-classic` | [ArmChair_01](https://polyhaven.com/a/ArmChair_01) | 850x770x1065 | 5616 | 139.2 KB | 512 px textures |
| `ottoman` | [Ottoman_01](https://polyhaven.com/a/Ottoman_01) | 885x620x625 | 4010 | 94.8 KB | 512 px textures |
| `coffee-table-round` | [coffee_table_round_01](https://polyhaven.com/a/coffee_table_round_01) | 1200x1200x490 | 3732 | 187.9 KB | - |
| `coffee-table-square` | [modern_coffee_table_02](https://polyhaven.com/a/modern_coffee_table_02) | 1100x1100x370 | 12749 | 466.5 KB | - |
| `side-table` | [side_table_01](https://polyhaven.com/a/side_table_01) | 550x450x550 | 2542 | 82.1 KB | 512 px textures |
| `side-table-round` | [side_table_tall_01](https://polyhaven.com/a/side_table_tall_01) | 385x385x760 | 5538 | 86.2 KB | 512 px textures |
| `tea-table` | [chinese_tea_table](https://polyhaven.com/a/chinese_tea_table) | 840x840x500 | 2436 | 65.8 KB | 512 px textures |
| `sideboard` | [modern_wooden_cabinet](https://polyhaven.com/a/modern_wooden_cabinet) | 2000x500x680 | 15687 | 386.9 KB | simplify `--simplify-ratio 0.5 --simplify-error 0.003` |
| `ceiling-fan` | [ceiling_fan](https://polyhaven.com/a/ceiling_fan) | 1460x1460x520 | 11457 | 148.8 KB | 512 px textures |
| `plant-leafy` | [potted_plant_02](https://polyhaven.com/a/potted_plant_02) | 700x660x840 | 24426 | 321.7 KB | simplify `--simplify-ratio 0.35 --simplify-error 0.01`; 512 px textures |
| `plant-succulent` | [potted_plant_04](https://polyhaven.com/a/potted_plant_04) | 170x185x270 | 8697 | 175.5 KB | 512 px textures |
| `vase-ceramic` | [ceramic_vase_01](https://polyhaven.com/a/ceramic_vase_01) | 260x260x500 | 9784 | 79.1 KB | 512 px textures |
| `wall-art` | [fancy_picture_frame_01](https://polyhaven.com/a/fancy_picture_frame_01) | 600x20x465 | 335 | 97.3 KB | 512 px textures |
| `mirror-wall` | [ornate_mirror_01](https://polyhaven.com/a/ornate_mirror_01) | 490x26x745 | 10621 | 119.8 KB | 512 px textures |
| `shelf-cube` | [wooden_display_shelves_01](https://polyhaven.com/a/wooden_display_shelves_01) | 1080x370x1555 | 2854 | 165.3 KB | turned -90 deg |
| `shelf-steel` | [steel_frame_shelves_01](https://polyhaven.com/a/steel_frame_shelves_01) | 1100x500x2140 | 3664 | 174.8 KB | - |
| `basket-rattan` | [wicker_basket_02](https://polyhaven.com/a/wicker_basket_02) | 374x270x220 | 17608 | 265.0 KB | 512 px textures |
| `rocking-chair` | [Rockingchair_01](https://polyhaven.com/a/Rockingchair_01) | 710x830x995 | 11014 | 161.3 KB | 512 px textures |
| `dining-round-2` | [round_wooden_table_02](https://polyhaven.com/a/round_wooden_table_02) | 800x800x745 | 3662 | 100.5 KB | 512 px textures |
| `chair-dining` | [dining_chair_02](https://polyhaven.com/a/dining_chair_02) | 435x575x975 | 17011 | 163.9 KB | 512 px textures |
| `chair-wood-white` | [painted_wooden_chair_01](https://polyhaven.com/a/painted_wooden_chair_01) | 430x540x955 | 672 | 77.8 KB | 512 px textures |
| `chair-monobloc` | [plastic_monobloc_chair_01](https://polyhaven.com/a/plastic_monobloc_chair_01) | 640x630x880 | 3226 | 127.6 KB | 512 px textures |
| `stool-wood` | [bar_chair_round_01](https://polyhaven.com/a/bar_chair_round_01) | 480x480x750 | 14373 | 370.0 KB | 512 px textures |
| `table-wood` | [wooden_table_02](https://polyhaven.com/a/wooden_table_02) | 1200x700x760 | 196 | 204.4 KB | - |
| `dining-set-4` | [wooden_table_02](https://polyhaven.com/a/wooden_table_02) + [painted_wooden_chair_01](https://polyhaven.com/a/painted_wooden_chair_01) x4 | 1200x1600x955 | 868 | 400.3 KB | table 1200x800x750, chairs 430x540x955, seats 140 mm under the table edge |
| `dining-set-6` | [wooden_table_02](https://polyhaven.com/a/wooden_table_02) + [painted_wooden_chair_01](https://polyhaven.com/a/painted_wooden_chair_01) x6 | 1800x1700x955 | 868 | 400.6 KB | table 1800x900x750, chairs 430x540x955, seats 140 mm under the table edge |
| `nightstand` | [painted_wooden_nightstand](https://polyhaven.com/a/painted_wooden_nightstand) | 505x510x615 | 468 | 80.2 KB | 512 px textures |
| `dresser` | [GothicCommode_01](https://polyhaven.com/a/GothicCommode_01) | 1200x580x1210 | 3871 | 212.9 KB | - |
| `cabinet-drawers` | [drawer_cabinet](https://polyhaven.com/a/drawer_cabinet) | 1140x490x1880 | 13543 | 241.9 KB | - |
| `desk-metal` | [metal_office_desk](https://polyhaven.com/a/metal_office_desk) | 1600x760x760 | 4856 | 174.3 KB | - |
| `range-electric` | [electric_stove](https://polyhaven.com/a/electric_stove) | 550x650x860 | 12532 | 278.6 KB | - |
| `microwave` | [vintage_microwave](https://polyhaven.com/a/vintage_microwave) | 500x370x300 | 7191 | 105.2 KB | 512 px textures |
| `cabinet-white` | [painted_wooden_cabinet](https://polyhaven.com/a/painted_wooden_cabinet) | 1195x615x1180 | 2135 | 225.6 KB | - |
| `outdoor-set` | [outdoor_table_chair_set_01](https://polyhaven.com/a/outdoor_table_chair_set_01) | 783x1828x860 | 9602 | 402.6 KB | - |
| `bench-wood` | [painted_wooden_bench](https://polyhaven.com/a/painted_wooden_bench) | 1165x500x890 | 624 | 252.6 KB | - |
| `planter-box` | [planter_box_01](https://polyhaven.com/a/planter_box_01) | 915x415x425 | 6898 | 148.5 KB | 512 px textures |
| `car-covered` | [covered_car](https://polyhaven.com/a/covered_car) | 1790x4380x1410 | 12118 | 320.1 KB | - |
| `gate-iron` | [large_iron_gate](https://polyhaven.com/a/large_iron_gate) | 2950x100x2930 | 19070 | 437.4 KB | simplify `--simplify-ratio 0.5 --simplify-error 0.002` |
| `lamp-post-classic` | [street_lamp_01](https://polyhaven.com/a/street_lamp_01) | 509x280x2800 | 20052 | 275.9 KB | scaled uniformly by height |
| `tree-flowering` | [jacaranda_tree](https://polyhaven.com/a/jacaranda_tree) | 6758x5326x5460 | 50192 | 1.38 MB | scaled uniformly by height; simplify `--simplify-ratio 0.01 --simplify-error 0.05`; 512 px textures; BLEND leaves set to OPAQUE (they carry no alpha) |

Stretched more than 15% away from the scan's own proportions to fill the catalog size: `sideboard` (2440 mm wide scan made 2000 mm at the same 680 mm height), `desk-metal` (2000 x 947 mm made 1600 x 760, height kept), `microwave` (the scan is 1074 mm wide; at 500 x 370 x 300 it is about 20% taller than its own proportions) and the tables of `dining-set-4` and `dining-set-6` (the 1134 x 706 mm scan made 1200 x 800 and 1800 x 900). `tree-flowering` is scaled uniformly instead: `jacaranda_tree` is 24 x 18 m in plan and 19.8 m tall, so at 5.5 m tall it is about 6.8 x 5.3 m, the catalog footprint.

Not covered by any CC0 source (procedural fallback in `src/viewer3d/scene/assets.ts` stays in use):

- `tree`: Kenney Furniture Kit has no tree; Poly Haven's tree models (e.g. island_tree_01) carry a ~58 MB shared .bin far outside budget.
- `car-sedan`: None of the three approved sources publish a CC0 vehicle model.

Kenney stays for the beds, the bath fixtures, the kitchen appliances and the other Wave 1 keys (`sofa-2`, `sofa-3`, `wardrobe`, `tv-console`, `desk`, `dining-4`, `dining-6`). Poly Haven has no usable beds (only bare or antique frames: `GothicBed_01`, `old_bed_frame`, `vintage_day_bed`) and no toilets, sinks, showers, fridges or washers, so those stay Kenney or procedural. Its classic sofas come in as keys of their own (`sofa-leather` from `sofa_02`, `sofa-wood` from `chinese_sofa`) next to the plain modern Kenney ones.

The v2 keys were chosen for homes in the Philippines: a narra-like wooden sala sofa (`sofa-wood`), the white monobloc chair (`chair-monobloc`), a rattan basket (`basket-rattan`) and the ceiling fan (`ceiling-fan`), with a low wooden table, a rocking chair, a bench, a planter box, a covered car, an iron gate, a lantern lamp post and a flowering tree for the porch and the site.

## Kenney palette recolor

The Kenney Furniture Kit's stock palette includes one saturated color, a salmon
(`carpet` material, raw `baseColorFactor` ~`[0.943, 0.367, 0.343]`, `#f15e57` read as sRGB
hex) reused as-is for both the sofa upholstery and the bed mattress/bedding top. Every other
Kenney material this pack uses (wood, the metal greys, the near-white fixture/appliance
tones, the pale glass) was already muted enough to keep unchanged. Applied in
`scripts/assets-build.mjs` (`recolorKenneyMaterials`) via `@gltf-transform/core`'s `NodeIO`:
each Kenney model's materials are read, matched against the salmon value, and their
`baseColorFactor` replaced, before the optimize pass. Poly Haven models are never recolored.

| Catalog keys | Original | New | Role |
|---|---|---|---|
| `sofa-2`, `sofa-3` | `#f15e57` salmon (`carpet`) | `#8f8a82` warm grey | upholstery |
| `bed-single`, `bed-double`, `bed-queen` | `#f15e57` salmon (`carpet`) | `#eeeae2` off-white | mattress / bedding |

Unchanged (already muted, kept as-is): wood tones (`wood` `#e59964`, `woodDark` `#ad744c`),
appliance/fixture whites and metal greys (`carpetWhite` `#f8ffff`, `metalLight` `#effaf4`,
`metal` `#bdd2d6`, `metalMedium` `#5e7777`, `metalDark` `#4e6363`, `_defaultMat` `#ffffff`), and
the pale glass (`glass` `#b2d3c4`).

