# MCP server

Guhit Studio speaks the Model Context Protocol. An MCP client - Claude Code,
Codex, Cursor - can open a project, draw rooms, hang doors and windows, read
areas and export a sheet, and the desktop window shows every change as it
happens.

You say this in Claude Code:

> draw a 3-bedroom bungalow 10 x 8 m

and watch the plan appear in the app.

## Why it works this way

The model runs inside your MCP client, on your own subscription. Guhit never
sees a key, never calls a model and never pays for one. The client does the
thinking and calls these tools; the Rust engine does the geometry.

This is separate from the in-app copilot, which needs a Console API key of
your own (DECISIONS D13). Both use the same engine, the same commands and the
same validation.

## Setup

The desktop app serves MCP on `http://127.0.0.1:1450/mcp` while it is running.

### Claude Code

```bash
claude mcp add --transport http guhit http://localhost:1450/mcp
```

Remove it again with `claude mcp remove guhit`. Check it with `/mcp` inside
Claude Code.

### Cursor

`~/.cursor/mcp.json` (all projects) or `.cursor/mcp.json` (one project):

```json
{ "mcpServers": { "guhit": { "url": "http://localhost:1450/mcp" } } }
```

### Codex

`~/.codex/config.toml`:

```toml
[mcp_servers.guhit]
url = "http://localhost:1450/mcp"
```

Codex has moved its MCP configuration around between versions. If this shape
is refused, check `codex --help` for the current one; the endpoint URL is the
only thing Guhit cares about.

### Changing the port

Set `mcp_port` in `settings.json` inside the app data folder and restart the
app:

```json
{ "mcp_port": 1451 }
```

The app data folder is `~/Library/Application Support/com.guhit.studio` on
macOS and `%APPDATA%\com.guhit.studio` on Windows. A port that is already in
use is reported on stderr and the app starts normally without MCP.

### Without the desktop app

The dev bridge serves the same endpoint on its own port, so the whole thing
can be driven headless:

```bash
cargo run -p guhit-devbridge -- --port 1631 --data .devdata/mcp
claude mcp add --transport http guhit-dev http://localhost:1631/mcp --scope local
```

## The tools

Every length in every argument and every result is millimeters. Areas come
back in square metres. The plan is +x east, +y north.

| Tool | What it does |
|---|---|
| `list_projects` | Every project on this machine, newest first |
| `open_project` | Open one by id; it becomes the document every tool acts on |
| `create_project` | Create and open one: `blank`, `sample-bungalow` or `plumbing-demo` (the bungalow with services) |
| `close_project` | Close it; the window goes back to the hub |
| `get_project_summary` | Totals: areas, wall length, counts |
| `list_rooms` | Rooms with areas, perimeters and bounding wall ids |
| `list_elements` | All elements of one kind, with ids |
| `describe_elements` | Full data plus derived geometry, by id |
| `find_rooms_without_exterior_window` | Rooms with no daylight |
| `list_review_items` | Design review suggestions with their status (open, or ignored with a note), located items with a `location_mm`, and the set-aside findings that are resolved |
| `get_pipe_takeoff` | Run lengths for every system by material and size, elbows, tees, sleeves, every penetration and the aircon core holes |
| `get_schedule` | Lights, outlets, switches, fixtures and aircon units per level and room, in the rows of the PH electrical inspection form, with totals per level |
| `get_plan_image` | The last plan thumbnail the window saved, as a PNG |
| `list_renders` | Saved 3D visuals |
| `add_wall`, `add_wall_chain` | Walls |
| `add_rect_room` | Four walls plus a named room |
| `add_level`, `delete_level` | Add a storey on top of the highest level, or delete a level with everything on it |
| `add_door`, `add_window` | Openings hosted on a wall |
| `resize_room`, `set_wall_length`, `move_elements` | Reshape |
| `rename_room`, `set_room_usage` | Room data |
| `set_opening_size`, `delete_elements` | Edit and remove |
| `set_material`, `set_roof` | Finishes |
| `add_asset` | Furniture, fixtures, lights, outlets, switches, panelboards, detectors and aircon units from the built-in library; wall items snap to the nearest wall face |
| `set_review_mark` | Set a review item, a whole check or a check on one element aside with a note, or reopen it |
| `undo`, `redo` | History, whoever made the change |
| `save_version` | A named version the user can restore in the app |
| `export_plan` | PDF, SVG or DXF into the exports folder: the plan or a service sheet (`sheet`), pipes included unless `show_pipes` is false, and a page of review items in a PDF (`review_page`) |
| `batch` | Several edits atomically, as one undo step |

Pipes and service runs (cold and hot water, drainage, vent, storm drains,
electrical conduit, aircon line sets and condensate) are drawn in the app, not
through these tools. An MCP client can read them (`list_elements` with kind
`pipe`, `describe_elements`), move or delete them like any element, and answer
quantity questions with `get_pipe_takeoff`, which covers every system.

Lights, outlets, switches, the panelboard, detectors and aircon units are
library objects: `add_asset` places them, and a wall item lands with its back
on the nearest wall face within 1000 mm of the point given. `describe_elements`
shows an object's device kind, mount, light, circuit tag and links (what a
switch controls, what feeds an aircon unit). Linking is done in the app with
the link tool (L). `get_schedule` counts the objects per room in the rows of
the PH electrical inspection form.

Guhit coordinates services and never sizes them, plans circuits or calculates
loads: plumbing plans are signed by a registered Master Plumber, electrical
plans by a Professional Electrical Engineer, aircon by a Professional
Mechanical Engineer (DECISIONS D19, D21).

The `plumbing-demo` template is the bungalow with services: the 16 plumbing
runs of the concept, two storm downspouts, a light in every room, a pendant
and an outdoor light, switches by the doors (a 3-way in the bedroom), outlets,
the range and washer outlets, a panelboard, a smoke detector, and a split
aircon for the bedroom with its outlet, line set and condensate drain. Its
review items: the four plumbing findings (the penetration summary names the
aircon core hole), the T&B light with no switch, and the meters of line set
beyond a standard installation.

### Levels

New elements go on the first level. `add_wall`, `add_wall_chain`,
`add_rect_room` and `add_asset` take an optional `level`: a level id, or its
name (case does not matter). `add_level` adds a storey: "Level N", its floor on
top of the highest level (that level's elevation plus its height) and 3000 mm
floor to floor unless you say otherwise. Names are 1 to 60 characters, heights
2000 to 10000 mm, and no two levels share a floor elevation. The result lists
it under `levels_added` with its id, so one `batch` can add the level and draw
on it by name:

```json
{"steps": [
  {"tool": "add_level", "args": {"name": "Second Floor"}},
  {"tool": "add_rect_room", "args": {"origin": {"x": 0, "y": 0}, "width_mm": 4000,
    "depth_mm": 3000, "name": "Bedroom", "level": "Second Floor"}}
]}
```

`delete_level` removes a level with its walls, doors, windows, rooms, columns,
stairs, objects, notes, dimensions and pipes, and removes links from other
objects to the deleted ones, as one undo step. The last level cannot be
deleted, and elements on a locked layer keep their level until the layer is
unlocked. The roof sits on the top level.

### `export_plan` sheets

`sheet` picks the drawing, `plan` when it is left out:

| `sheet` | Drawing |
|---|---|
| `plan` | The architectural plan |
| `lighting` | Lights, switches and their links, a legend and counts |
| `power` | Outlets, special purpose outlets, the panelboard and conduit, a legend, counts and a schedule of loads with blank ratings for the Professional Electrical Engineer |
| `plumbing` | Water, drainage, vent and storm runs, a legend and a fixture table |
| `plumbing_isometric` | Water and sanitary isometric diagrams, not to scale, with a legend and a blank Master Plumber block |
| `aircon` | Aircon units, line sets, condensate drains and core holes, with a legend |

Every sheet works in PDF, SVG and DXF. `review_page: true` adds a page of
review items, open and set aside, with their notes; it is PDF only, and the
tool refuses it with another format. An unknown sheet name is refused before
anything is drawn. The signing professional's fields stay blank on every
sheet.

### `set_review_mark`

`{"action": "set_aside" | "reopen", "issue_id"?, "code"?, "element_id"?, "note"?}`

The target is one finding (`issue_id` from `list_review_items`), a whole check
(`code`, for example `light_no_switch`) or a check on one element (`code` and
`element_id`). `set_aside` needs a note. A set-aside item stays in
`list_review_items` with status `ignored` and its note; it is never approved
(DECISIONS D24). A set-aside finding that the checks stop producing is listed
under `resolved`. One call is one undo step, like every edit.

Resources:

- `guhit://project/current` - compact JSON of the open project, with review
  items and their status.
- `guhit://docs/conventions` - units, coordinates, joins, flip conventions,
  pipe and service run systems, devices and links, review marks.
- `guhit://docs/ph-defaults` - 150 mm CHB walls, 900 x 2100 doors,
  1200 x 1200 windows with a 900 sill, 3000 mm levels, material ids.

### `batch` and element ids

`batch` takes a list of `{"tool": ..., "args": ...}` steps, applies them in
order and commits them as one `Command::Batch`: either all of them land or
none do, and one undo reverts the lot.

Each step sees the result of the steps before it, so a later step can name an
element an earlier step created. Ids are stable: the core seeds new ids per
leaf command from the project state right before that command runs, so an id
does not move when more steps are appended to the batch. The result lists what
every step created under `steps`, which makes the normal recipe two calls:

1. `batch` the rooms, read the wall ids it reports per step,
2. `batch` the doors and windows on those walls.

## Safety

- **The engine validates everything.** A tool call is translated into the same
  typed `Command` the app's own tools and the in-app copilot produce, then run
  through `Document::preview` before it is committed. A refused edit changes
  nothing and the engine's exact message goes back to the model, so it can
  correct itself: `The door (900 mm wide, centered 9000 mm from the wall
  start) does not fit on its wall, which is 4000 mm long.`
- **One tool call is one undo step**, labelled `MCP: ...` in the app, with
  `Origin::Ai`. Anything an MCP client does can be undone in the app, and
  anything done in the app can be undone from the client.
- **The commit is revision guarded.** If the window or the copilot changes the
  plan between the tool reading it and committing, nothing is applied and the
  client is told the plan moved on.
- **Approval is the MCP client's job.** Claude Code asks before it runs a tool
  and remembers what you allowed. Guhit does not add a second prompt, because
  an MCP client is a program you already trust with your machine. The in-app
  copilot is different: it never commits without you pressing Apply.
- **Loopback only.** The listener binds 127.0.0.1 and the transport refuses a
  request whose `Host` header is not local, which blocks DNS rebinding from a
  web page. There is no authentication, so anything that can already run code
  on your machine can drive the app; that is the same trust level as the dev
  bridge.
- **Review items are suggestions.** Nothing here is permit approval,
  structural certification or code compliance, and the server instructions
  tell the model to say so.
- **Exports** go to the app's exports folder. Through the dev bridge a caller
  cannot name a path outside the data dir at all.

## How the window keeps up

`AppService` fires a change notification after every commit, undo, redo, open,
close and restore. The desktop shell forwards it as the Tauri event
`doc_changed` with `{revision}`; `onDocChanged` in `src/contract/ipc.ts`
subscribes, and `EditorShell` refetches `doc_state`. In a plain browser
against the dev bridge, `onDocChanged` polls `doc_revision` once a second
instead.

Known gap: `EditorShell` is not mounted while the project hub is showing, so
if an MCP client creates or opens a project while the window is on the hub,
the window does not switch to the editor by itself. Opening the project in the
hub picks up the MCP client's work correctly. Moving the effect up to
`src/App.tsx` would close this; that file belongs to the shell owner.

## One document

There is one open document per app, shared by the window, the copilot and
every MCP client. `open_project` switches it, saving the previous one first.
An MCP client and the window are always looking at the same plan.
