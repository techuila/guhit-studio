# Interoperability

How a Guhit Studio project moves in and out of other tools. The decision
behind this is DECISIONS D16: open formats first, DWG only through an
external converter.

## What crosses the boundary

| Direction | Format | Command | Notes |
|---|---|---|---|
| In | DXF (ASCII and binary) | `import_inspect`, `import_commit` | Walls or linework |
| In | DWG | same | Needs the ODA File Converter |
| In | glTF, GLB, OBJ | `model_store` | Reference model in the 3D view |
| In | `.guhit` bundle | `bundle_open` | A whole project |
| Out | PDF, SVG, DXF (2D plan) | `export_plan` | Sheet or model space, pipes when `show_pipes` (see "Pipes in the exports") |
| Out | DWG (2D plan) | `export_model` `dwg` | Needs the converter |
| Out | IFC4, 3D DXF | `export_model` `ifc`, `dxf3d` | Whole model with pipes, written by `guhit-export` |
| Out | GLB, OBJ, DAE | `export_bytes` | Made in the 3D view |
| Out | `.guhit` bundle | `bundle_save` | A whole project |

SketchUp `.skp` is neither read nor written: the format has no Rust binding
and its SDK is proprietary. SketchUp itself reads DAE, OBJ, glTF, DXF and
IFC, all of which this app writes.

## Importing a drawing

Import is two steps on purpose. `import_inspect` reads the file and reports
what is in it: the layers, how many entities each holds, how many walls the
recognizer would find on it, the layer colors and the bounding box. Nothing
is committed. `import_commit` then takes the options the user confirmed and
applies the whole drawing as one undo step labelled `Import <file>`.

### Units

A DXF may declare its unit in `$INSUNITS`. When it does, that wins:

| `$INSUNITS` | Unit | mm per unit |
|---|---|---|
| 1 | inches | 25.4 |
| 2 | feet | 304.8 |
| 4 | millimeters | 1.0 |
| 5 | centimeters | 10.0 |
| 6 | meters | 1000.0 |

Many drawings declare nothing (`$INSUNITS` 0 or absent). Then the app guesses
from the bounding box, because a building is a few metres to a few hundred
metres across:

- under 200 units wide, it reads as **meters** (1000 mm per unit),
- under 2000 units wide, as **centimeters** (10),
- otherwise as **millimeters** (1).

Feet and inches are never guessed. Their plausible ranges sit inside the
centimeter range, so there is no honest way to tell them apart from the box
alone. The dialog always shows the suggestion and the user confirms or
changes it; `declared_unit` is `null` when the guess is all there is.

### What is read

`LINE`, `LWPOLYLINE` (arc bulges included), `POLYLINE`, `ARC`, `CIRCLE`, and
`INSERT` of a block, expanded one level with the insert's scale, rotation and
position. Geometry drawn on layer `0` inside a block takes the insert's
layer, which is the DXF rule. Model space only: paper space is ignored.

Arcs, circles and bulges are tessellated at 5 degree steps. Everything else
(splines, hatches, text, dimensions, nested blocks, 3D solids) is counted and
reported back in `ImportResult::skipped`, never dropped in silence.

### Wall recognition

CAD plans draw a wall as two parallel lines. In `walls` mode the recognizer
pairs them up, per layer:

1. Two segments count as a wall pair when they are **parallel** (within about
   2 degrees), between **50 and 600 mm apart** after scaling, and **overlap
   over at least 300 mm**.
2. The wall centerline sits halfway between them, trimmed to the overlap.
   Thickness is the measured gap rounded to the nearest **5 mm**.
3. On each side of a line, the nearest partner claims the stretch it covers.
   A line further away that covers the same stretch is shadowed and never
   becomes a second wall on top of the first.

Recognition alone leaves corners open, because each wall stops where its own
two faces stop, half a thickness short of the corner. The engine only joins
walls whose endpoints are within 1 mm, so three healing passes run next:

1. endpoints within **20 mm** of each other are pulled onto one point,
2. two walls that meet at an angle are extended to their intersection when
   the gap is no wider than half of each thickness plus 20 mm,
3. an end that still found no partner but stops just short of another wall's
   body is dropped onto that body, which the engine then reads as a T.

Collinear walls of the same thickness that meet where nothing else arrives
are merged into one. A node where three walls meet is left alone.

Whatever no wall used becomes **linework**: one locked `Linework` element per
layer, named `<file> / <layer>`, colored from the layer's AutoCAD color
index, on the Underlays layer. In `linework` mode nothing is recognized and
everything becomes linework, which is the right choice for a survey, a site
plan or any drawing you only want to trace over.

Room detection then runs by itself, as it does after any wall edit.

### Limits, stated plainly

- Straight walls only. A curved wall stays linework.
- A wall thinner than 50 mm or thicker than 600 mm is not recognized.
- Layers are paired one at a time. A drawing whose two wall faces live on
  different layers will not pair across them; import it as linework.
- Doors, windows, stairs, fixtures and text are not recognized. They come in
  as linework when their layer is picked, and you place the real elements.
- Three parallel lines within 600 mm of each other (a wall plus a nearby
  line) can produce a second, wider wall. Check the result before building
  on it.
- The file must be 10 MB or smaller.

Import is a starting point, never a finished model. Always look at the plan
after importing.

## DWG, through the ODA File Converter

DWG is a closed format. Guhit Studio does not read or write it directly:
the only free library is GPL, which would force this app open source, and the
Autodesk SDK is paid and Windows only. Instead the app drives the **free ODA
File Converter** that you install yourself.

1. Download it from
   <https://www.opendesign.com/guestfiles/oda_file_converter>. It is free,
   with builds for macOS (Intel and Apple Silicon) and Windows. You give an
   email address and get a download link.
2. Install it. On macOS you get `ODAFileConverter.app` in `/Applications`; on
   Windows you get `ODAFileConverter.exe` under `Program Files`.
3. In Guhit Studio, point the app at it. `dwg_set_path` takes the absolute
   path. On macOS you may hand it the `.app` bundle: the app finds the binary
   inside. The path is checked and the program is started once to see that it
   answers, then stored in `settings.json` under `dwg_converter_path`.
4. `dwg_status` reports the state at any time: whether a path is set, whether
   the program runs, and what to do when it does not.

After that, `.dwg` files import like `.dxf` and `export_model` with format
`dwg` writes DWG. Under the hood each job gets its own pair of folders inside
the app data dir, the converter is called as

```
ODAFileConverter <in folder> <out folder> ACAD2018 DXF 0 1 *.dwg
```

with `std::process::Command` and an absolute path, never through a shell, and
the folders are removed afterwards. A job that takes more than two minutes is
stopped.

Without the converter, DWG commands fail with a message that says exactly
this and points at the download page. Nothing else is affected: DXF, PDF,
SVG, glTF, OBJ and bundles all work without it.

## The `.guhit` bundle

A `.guhit` file is a plain zip of one project folder. It exists so a project
moves between machines, or to a client, as a single file.

```
project.json          the model
thumbnail.png
snapshots/<id>.json   named and automatic versions
underlays/<file>      traced images
renders/<id>.png      saved visuals, plus renders/index.json
models/<file>         glTF, GLB and OBJ reference models
```

`ai-log.jsonl` is left out. It is a private record of what was asked of the
copilot and does not belong in a file you hand to someone else.

`bundle_save` writes to the exports folder, or to a path the desktop save
dialog supplied. `bundle_open` reads one back:

- Every entry is checked before anything is written: no absolute path, no
  `..`, no drive letter, no backslash, and each path element must already be
  a safe file name. A bundle over 200 MB expanded, or with more than 5000
  entries, is refused.
- The bundle may carry one wrapping folder; the `project.json` nearest the
  root marks where the project folder starts.
- The project keeps its own id when that id is free. When a different project
  already holds it, the copy gets a new id and is renamed to
  `<name> (imported)`, so the original is never overwritten. Re-opening the
  same bundle over the same unchanged project just opens it.

## Reference models

`model_store` copies a `.glb`, `.gltf` or `.obj` of up to 50 MB into
`models/` inside the project. The name is made safe and never overwrites an
existing file: a second `massing.glb` is stored as `massing-2.glb`, because a
`ReferenceModel` element may already point at the first. Any other extension
is refused. `model_data` returns the bytes as a data URL with the right media
type (`model/gltf-binary`, `model/gltf+json`, `model/obj`).

A reference model is context for the 3D view: a site, a neighbour, a massing
from SketchUp. It is never part of the model the engine measures.

## Whole-model exports

`export_model` takes one `format` and an optional `path`, and writes to the
exports folder when no path is given:

- `ifc` writes an IFC4 STEP file in millimeters, from `guhit_export::model_ifc`.
- `dxf3d` writes a 3D DXF, from `guhit_export::model_dxf3d`.
- `dwg` writes the 2D plan DXF and hands it to the ODA File Converter.

GLB, OBJ and DAE are produced in the 3D view and saved with `export_bytes`,
because the geometry for them is built by the three.js scene.

## Pipes in the exports

Pipes are a coordination layer (DECISIONS D19). Every export writes them as
they were drawn: Guhit does not size pipes, and no export claims a plumbing
design or code compliance. Plumbing plans are signed by a registered Master
Plumber (RA 1378). A project without pipes exports exactly as it did before
pipes existed, byte for byte.

Which pipes go where:

| Export | Pipes drawn | Hidden pipe layer |
|---|---|---|
| PDF, SVG sheet | the exported level, when `show_pipes` is on | not drawn |
| DXF 2D (and DWG) | the exported level, when `show_pipes` is on | not drawn |
| DXF 3D | tubes for every pipe; plan lines at z = 0 like the rest of the linework | tubes stay, plan lines go |
| IFC4 | every pipe | written |

The whole-model exports carry every element whatever its layer, and pipes
follow that rule. A level holding only pipes is still a drawing, not an
empty export.

### On the plan sheet

- Colors are the tokens `--pipe-cold` `#2b7bd0`, `--pipe-hot` `#e0563a`,
  `--pipe-drain` `#9b6a35` and `--pipe-vent` `#3a9a5c`.
- The line weight is the pipe size at the sheet scale, never under 0.35 mm:
  a 100 mm drain is 2 mm wide at 1:50, a 20 mm supply line 0.35 mm at 1:100.
- Cold and hot water are solid, drainage is dashed, vent is dash-dot. Wider
  lines get proportionally longer dashes.
- A segment that runs vertically, a riser or a drop, is a white circle in
  the system color at its plan position, always wider than its line.
  Vertical means under 1 mm apart in plan, or at most 50 mm apart and
  rising at least ten times that.
- Pipes draw over the whole plan, dimensions included, and over the white
  masks behind room names, so a label never cuts a run; the names stay on
  top. Wide drainage goes first so supply lines stay readable on top of it.
- The sheet extent includes the pipes, so a service line to the meter or an
  outlet to the septic tank is not cut off.
- A legend in the band between the drawing title and the scale bar lists
  only the systems on the sheet. It never reaches the title block.

### DXF

One layer per system, with the AutoCAD color index nearest the system color
(R12 has no true color):

| Layer | System | Color | Linetype |
|---|---|---|---|
| `P-DOMW-CPIP` | cold water | 150 | `CONTINUOUS` |
| `P-DOMW-HPIP` | hot water | 20 | `CONTINUOUS` |
| `P-SANR-PIPE` | drainage | 33 | `GUHIT_DASHED` |
| `P-SANR-VENT` | vent | 103 | `GUHIT_DASHDOT` |

- The file defines the two linetypes itself, sized to plot at 1:N (the
  export scale, else the project scale): `GUHIT_DASHED` is a 2.2 mm dash and
  a 1.1 mm gap on paper, `GUHIT_DASHDOT` 3.0 mm dash, 0.9 mm gap, dot, 0.9 mm
  gap. Their own names keep another drawing's `DASHED` from replacing them
  when the file is inserted as a block.
- Runs are open polylines at z = 0, risers are circles sized for plotting
  and kept `CONTINUOUS` on the dashed layers.
- Pipe layers and linetypes are only in the file when a system is present.
- In the 3D DXF each pipe is a closed 8-sided tube of 3DFACEs on its system
  layer, at world height (level elevation plus the point's z). Bends up to
  120 degrees are mitred into one surface; sharper bends and very short
  stubs are cut square and capped. Tube faces carry a `CONTINUOUS` linetype
  so only the 2D lines are dashed.

### IFC4

- One `IfcPipeSegment` (`.RIGIDSEGMENT.`) per straight segment. The body is
  an `IfcExtrudedAreaSolid` of an `IfcCircleProfileDef` (radius = size / 2)
  swept from the segment start along its direction, representation `Body`,
  `SweptSolid`. It is contained in the `IfcBuildingStorey` of its level.
- Name: the pipe's name, or the system and size ("Cold water 20 mm") when it
  has none. Description: "Segment 2 of 4". The GlobalId comes from the pipe
  id and the segment index, so it survives edits and re-exports.
- One `IfcDistributionSystem` per system present, `.DOMESTICCOLDWATER.`,
  `.DOMESTICHOTWATER.`, `.DRAINAGE.` or `.VENT.`, holding its segments
  through `IfcRelAssignsToGroup` and serving the building through
  `IfcRelServicesBuildings`.
- Material by `IfcRelAssociatesMaterial`: `PPR`, `uPVC`, `GI`, `PE` or
  `Copper`.
- Properties: `Pset_PipeSegmentTypeCommon.NominalDiameter`, and
  `Guhit_Pset_Pipe` with the system, material, pipe id and segment count.
- Not written: elbows and tees as `IfcPipeFitting` (segments meet at their
  end points, so a bend shows a small notch on its outside in a viewer),
  sleeves, and colors (the exporter styles no element).
