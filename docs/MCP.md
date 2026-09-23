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
| `create_project` | Create and open one: `blank`, `sample-bungalow` or `plumbing-demo` |
| `close_project` | Close it; the window goes back to the hub |
| `get_project_summary` | Totals: areas, wall length, counts |
| `list_rooms` | Rooms with areas, perimeters and bounding wall ids |
| `list_elements` | All elements of one kind, with ids |
| `describe_elements` | Full data plus derived geometry, by id |
| `find_rooms_without_exterior_window` | Rooms with no daylight |
| `list_review_items` | Design review suggestions, pipe items with a `location_mm` |
| `get_pipe_takeoff` | Pipe lengths by system, material and size, elbows, tees, sleeves and every penetration |
| `get_plan_image` | The last plan thumbnail the window saved, as a PNG |
| `list_renders` | Saved 3D visuals |
| `add_wall`, `add_wall_chain` | Walls |
| `add_rect_room` | Four walls plus a named room |
| `add_door`, `add_window` | Openings hosted on a wall |
| `resize_room`, `set_wall_length`, `move_elements` | Reshape |
| `rename_room`, `set_room_usage` | Room data |
| `set_opening_size`, `delete_elements` | Edit and remove |
| `set_material`, `set_roof` | Finishes |
| `add_asset` | Furniture and fixtures from the built-in library |
| `undo`, `redo` | History, whoever made the change |
| `save_version` | A named version the user can restore in the app |
| `export_plan` | PDF, SVG or DXF into the exports folder, pipes included unless `show_pipes` is false |
| `batch` | Several edits atomically, as one undo step |

Pipes are drawn in the app, not through these tools. An MCP client can read
them (`list_elements` with kind `pipe`, `describe_elements`), move or delete
them like any element, and answer quantity questions with `get_pipe_takeoff`.
Guhit coordinates pipes and never sizes them; plumbing plans are signed by a
registered Master Plumber (DECISIONS D19). The `plumbing-demo` template is a
bungalow with all four pipe systems and the pipe review items to look at.

Resources:

- `guhit://project/current` - compact JSON of the open project.
- `guhit://docs/conventions` - units, coordinates, joins, flip conventions,
  pipe heights.
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
