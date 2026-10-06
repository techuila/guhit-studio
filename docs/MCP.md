# MCP server

Guhit Studio speaks the Model Context Protocol. An MCP client (Claude Code,
Codex, Cursor, Copilot, Gemini, a local-model app, any agent that speaks MCP)
can open a project, draw rooms, hang doors and windows, read areas and export
a sheet, and the desktop window shows every change as it happens.

You say this in your agent:

> draw a 3-bedroom bungalow 10 x 8 m

and watch the plan appear in the app.

## Why it works this way

The model runs inside your MCP client, on your own subscription, API key or
local model. Guhit never sees a key, never calls a model and never pays for
one. The client does the thinking and calls these tools; the Rust engine does
the geometry.

This is separate from the in-app copilot, which needs a Console API key of
your own (DECISIONS D13). Both use the same engine, the same commands and the
same validation.

## Setup

Two ways in:

| Way | Use it when | Entry |
|---|---|---|
| HTTP URL | The agent takes a Streamable HTTP URL (most do) | `http://127.0.0.1:1450/mcp` |
| stdio command | The agent only launches a command | the app binary with `--mcp-stdio` |

Always write `127.0.0.1`, never `localhost`: some clients resolve `localhost`
to IPv6 first. The app listens on `127.0.0.1` and also `[::1]` when the
machine has IPv6, with no auth, loopback only.

The HTTP URL needs the desktop app to be running. The stdio entry forwards to
the running app. If the app is not open it still answers the tool list, and
the first tool call opens the app. It needs no Node.

| Platform | App binary for stdio (`<app binary>` below) |
|---|---|
| macOS | `/Applications/Guhit Studio.app/Contents/MacOS/guhit-studio` |
| Windows | `C:\Users\<you>\AppData\Local\Guhit Studio\guhit-studio.exe` (default per-user install; check your install folder, this path is not verified yet) |

In JSON the Windows path needs escaped backslashes
(`C:\\Users\\<you>\\AppData\\Local\\Guhit Studio\\guhit-studio.exe`), and most
clients do not expand `%LOCALAPPDATA%`, so write the full path. `--port N`
points the stdio entry at a different port.

### Status

Tested against Guhit on 2026-10-07 (connection and tool list, 47 tools):
Claude Code over HTTP and over the stdio entry, the Cursor CLI over HTTP, and
the stdio entry inside a built macOS app (`/Applications/...` path layout
confirmed; with the app closed it lists the tools, and the first tool call
opened the app and answered in about a second).
The transport tests in `crates/guhit-mcp/tests/transport.rs` also cover both
protocol generations (with and without `initialize`, the 2026-07-28 spec). Every
other snippet follows that agent's official docs as of October 2026 and has not
been run against Guhit. If a shape is refused, check that agent's current MCP docs; the
URL (or the stdio command) is the only thing Guhit cares about.

### Supported agents

| Agent | Paid or free | Free or local models | Connects by | Notes |
|---|---|---|---|---|
| [Claude Code](#claude-code) | Paid Claude plan or API key | Ollama (Anthropic-compatible API) | HTTP | Tested |
| [Claude Desktop](#claude-desktop) | Paid Claude plan | No | stdio | Config file is stdio only |
| [ChatGPT desktop app](#chatgpt-desktop-app) | Paid ChatGPT plan | No | HTTP | Shares Codex config |
| [Codex CLI and IDE](#codex-cli-and-ide-extension) | ChatGPT plan or API key | `codex --oss` (Ollama, LM Studio) | HTTP | |
| [Cursor](#cursor) | Free tier and paid | BYOK | HTTP | Warns above about 40 tools |
| [VS Code with GitHub Copilot](#vs-code-with-github-copilot) | Free tier, paid, or BYOK | BYOK and Ollama, no Copilot plan needed since June 2026 | HTTP | Root key is `servers` |
| [GitHub Copilot CLI](#github-copilot-cli) | Copilot plan | No | HTTP | |
| [Windsurf (Devin Desktop)](#windsurf-devin-desktop) | Free tier and paid | No | HTTP | 100 tools total |
| [JetBrains AI Assistant, Junie](#jetbrains-ai-assistant-and-junie) | Paid | No | HTTP | |
| [Zed](#zed) | Free editor, bring a model | Ollama | HTTP | No MCP resources, tools work |
| [Google Antigravity](#google-antigravity) | Free tier and paid | No | HTTP | Must be `serverUrl` |
| [Gemini CLI](#gemini-cli) | Paid API key or enterprise only | No | HTTP | Must be `httpUrl` |
| [Kiro](#kiro) | Free tier and paid | No | HTTP | |
| [Augment](#augment) | Paid | No | HTTP | |
| [Cline](#cline) | Free extension, bring a model | Ollama, LM Studio | HTTP | |
| [Kilo Code](#kilo-code) | Free, bring a model | BYOK, Ollama | HTTP | |
| [Continue](#continue) | Free | Ollama | HTTP | Agent mode only |
| [Goose](#goose) | Free | Yes | HTTP | Key is `uri` |
| [OpenCode](#opencode) | Free | Ollama | HTTP | |
| [Amp](#amp) | Paid | No | HTTP | |
| [Qwen Code](#qwen-code) | Free CLI | Yes | HTTP | Must be `httpUrl` |
| [Warp](#warp) | Free tier and paid | No | HTTP | |
| [Crush](#crush) | Free, bring a model | Ollama, LM Studio | HTTP | |
| [LM Studio](#lm-studio) | Free | Yes, local | HTTP | Tool images do not reach the model |
| [Jan](#jan) | Free | Yes, local | HTTP | |
| [Msty Studio](#msty-studio) | Free tier and paid | Yes | HTTP | |
| [AnythingLLM](#anythingllm) | Free | Yes | HTTP | Needs `type` |
| [LibreChat](#librechat) | Free, self-hosted | Yes | HTTP | Needs `allowedAddresses` |
| [Trae](#trae) | Free tier and paid | No | HTTP | |
| [Any other agent](#any-other-agent) | | | HTTP or stdio | |

### Claude Code

```bash
claude mcp add --transport http guhit http://127.0.0.1:1450/mcp --scope user
```

Or in `~/.claude.json` (all projects) or `.mcp.json` (one project):

```json
{ "mcpServers": { "guhit": { "type": "http", "url": "http://127.0.0.1:1450/mcp" } } }
```

Remove it with `claude mcp remove guhit`. Check it with `/mcp`.

### Claude Desktop

The config file takes a command, not a URL. Edit `claude_desktop_config.json`:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{ "mcpServers": { "guhit": {
  "command": "/Applications/Guhit Studio.app/Contents/MacOS/guhit-studio",
  "args": ["--mcp-stdio"]
} } }
```

On Windows set `command` to `C:\\Users\\<you>\\AppData\\Local\\Guhit Studio\\guhit-studio.exe`.

Fallback if you have Node:

```json
{ "mcpServers": { "guhit": {
  "command": "npx",
  "args": ["-y", "mcp-remote", "http://127.0.0.1:1450/mcp", "--transport", "http-only"]
} } }
```

Restart Claude Desktop after editing.

### ChatGPT desktop app

Settings, MCP servers, Add server, Streamable HTTP, URL
`http://127.0.0.1:1450/mcp`, then restart. It shares its config with Codex.

### Codex CLI and IDE extension

```bash
codex mcp add guhit --url http://127.0.0.1:1450/mcp
```

Or in `~/.codex/config.toml`:

```toml
[mcp_servers.guhit]
url = "http://127.0.0.1:1450/mcp"
```

Free or local: `codex --oss` runs Codex on Ollama or LM Studio.

### Cursor

`~/.cursor/mcp.json` (all projects) or `.cursor/mcp.json` (one project):

```json
{ "mcpServers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

Cursor warns when the enabled servers pass about 40 tools in total.

### VS Code with GitHub Copilot

Agent mode. `.vscode/mcp.json` (one project) or the user `mcp.json` (command
palette, "MCP: Open User Configuration"). The root key is `servers`:

```json
{ "servers": { "guhit": { "type": "http", "url": "http://127.0.0.1:1450/mcp" } } }
```

Since June 2026, bring-your-own-key models and Ollama work without a Copilot
plan.

### GitHub Copilot CLI

`~/.copilot/mcp-config.json`, or `/mcp add` inside the CLI:

```json
{ "mcpServers": { "guhit": { "type": "http", "url": "http://127.0.0.1:1450/mcp", "tools": ["*"] } } }
```

### Windsurf (Devin Desktop)

Cascade agent, `~/.config/devin/mcp_config.json` (Windows
`%APPDATA%\devin\mcp_config.json`):

```json
{ "mcpServers": { "guhit": { "serverUrl": "http://127.0.0.1:1450/mcp" } } }
```

Devin Local agent:

```bash
devin mcp add guhit http://127.0.0.1:1450/mcp
```

Windsurf allows 100 tools in total across servers.

### JetBrains AI Assistant and Junie

AI Assistant: Settings, Tools, AI Assistant, Model Context Protocol (MCP),
Add, then paste:

```json
{ "mcpServers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

Junie: the same JSON in `~/.junie/mcp/mcp.json` (all projects) or
`.junie/mcp/mcp.json` (one project).

### Zed

`settings.json`:

```json
{ "context_servers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

Zed has no MCP resources. The tools still work.

### Google Antigravity

IDE and CLI. `~/.gemini/config/mcp_config.json` or `.agents/mcp_config.json`.
The key must be `serverUrl`:

```json
{ "mcpServers": { "guhit": { "serverUrl": "http://127.0.0.1:1450/mcp" } } }
```

### Gemini CLI

`~/.gemini/settings.json`:

```json
{ "mcpServers": { "guhit": { "httpUrl": "http://127.0.0.1:1450/mcp" } } }
```

Or `gemini mcp add --transport http guhit http://127.0.0.1:1450/mcp`. A plain
`url` key means the old SSE transport, which Guhit does not serve. Since
2026-06-18 Gemini CLI serves enterprise and paid API keys only; Google moved
individuals to Antigravity CLI.

### Kiro

`~/.kiro/settings/mcp.json`:

```json
{ "mcpServers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

### Augment

```bash
auggie mcp add guhit --transport http --url http://127.0.0.1:1450/mcp
```

### Cline

`cline_mcp_settings.json` (Cline, MCP Servers, Configure):

```json
{ "mcpServers": { "guhit": { "type": "streamableHttp", "url": "http://127.0.0.1:1450/mcp" } } }
```

### Kilo Code

`~/.config/kilo/kilo.jsonc`:

```json
{ "mcp": { "guhit": { "type": "remote", "url": "http://127.0.0.1:1450/mcp" } } }
```

### Continue

`.continue/mcpServers/guhit.yaml`. MCP works in agent mode only.

```yaml
name: Guhit Studio
version: 0.0.1
schema: v1
mcpServers:
  - name: guhit
    type: streamable-http
    url: http://127.0.0.1:1450/mcp
```

### Goose

`~/.config/goose/config.yaml`, or add it in Goose, Extensions. The key is
`uri`, not `url`:

```yaml
extensions:
  guhit:
    name: Guhit Studio
    type: streamable_http
    uri: http://127.0.0.1:1450/mcp
    enabled: true
    timeout: 300
```

### OpenCode

`~/.config/opencode/opencode.json`:

```json
{ "mcp": { "guhit": { "type": "remote", "url": "http://127.0.0.1:1450/mcp", "oauth": false } } }
```

### Amp

`~/.config/amp/settings.json`:

```json
{ "amp.mcpServers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

### Qwen Code

`~/.qwen/settings.json`. A plain `url` means SSE, so use `httpUrl`:

```json
{ "mcpServers": { "guhit": { "httpUrl": "http://127.0.0.1:1450/mcp" } } }
```

### Warp

Settings, Agents, MCP servers, then paste:

```json
{ "mcpServers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

### Crush

`~/.config/crush/crush.json`:

```json
{ "mcp": { "guhit": { "type": "http", "url": "http://127.0.0.1:1450/mcp" } } }
```

### LM Studio

`~/.lmstudio/mcp.json` (Program, Install, Edit mcp.json):

```json
{ "mcpServers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

Known LM Studio bug: images returned by tools (`get_plan_image`) do not reach
the model. The tool list fills a small model's context, so prefer a model with
a 32k or larger context.

### Jan

Settings, MCP Servers, add an HTTP server with the URL
`http://127.0.0.1:1450/mcp`.

### Msty Studio

Toolbox, Add, HTTP, with the URL `http://127.0.0.1:1450/mcp`.

### AnythingLLM

`anythingllm_mcp_servers.json`. Without `type` it assumes SSE:

```json
{ "mcpServers": { "guhit": { "type": "streamable", "url": "http://127.0.0.1:1450/mcp" } } }
```

### LibreChat

Self-hosted. In `librechat.yaml`:

```yaml
mcpServers:
  guhit:
    type: streamable-http
    url: http://127.0.0.1:1450/mcp
    serverInstructions: true
```

LibreChat blocks private addresses by default, so add `127.0.0.1` to
`allowedAddresses` in its MCP settings. If LibreChat runs in Docker it cannot
reach the app on the host's loopback (see Not supported).
`serverInstructions: true` passes Guhit's instructions to the model.

### Trae

`.trae/mcp.json`:

```json
{ "mcpServers": { "guhit": { "url": "http://127.0.0.1:1450/mcp" } } }
```

### Any other agent

If it takes a Streamable HTTP URL, use `http://127.0.0.1:1450/mcp`. If it only
launches a command, use the stdio entry:
`<app binary> --mcp-stdio`.

### Not supported

| Client | Why |
|---|---|
| claude.ai on the web, Claude custom connectors | Connect from the vendor's cloud |
| ChatGPT on the web (developer mode connectors) | Connect from the vendor's cloud |
| Open WebUI in Docker | Connects from a container |

None of these can reach an app on `127.0.0.1`. Guhit does not expose itself
to the internet: that would need authentication, and leaving it off is a
deliberate choice, not a missing setting.

### Tips for every agent

- Approvals: each agent asks before a tool runs. Most have an auto-approve
  setting: Claude Code permissions, Codex `default_tools_approval_mode`,
  Gemini CLI trust, Kiro `autoApprove`, Cline `autoApprove`, Copilot CLI
  `--allow-tool`.
- Guhit has 47 tools. Cursor warns above about 40 tools in total and
  Windsurf (Devin Desktop) allows 100 across all servers, so turn off servers
  you are not using. Small local models get confused by that many, so use a
  capable model.
- Restart the agent after editing its config.

### Changing the port

Set `mcp_port` in `settings.json` inside the app data folder and restart the
app:

```json
{ "mcp_port": 1451 }
```

The app data folder is `~/Library/Application Support/ph.guhit.studio` on
macOS and `%APPDATA%\ph.guhit.studio` on Windows. A port that is already in
use is reported on stderr and the app starts normally without MCP. Change the
port in the agent's URL too, or pass `--port` to the stdio entry.

### Without the desktop app

The dev bridge serves the same endpoint on its own port, so the whole thing
can be driven headless:

```bash
cargo run -p guhit-devbridge -- --port 1631 --data .devdata/mcp
claude mcp add --transport http guhit-dev http://127.0.0.1:1631/mcp --scope local
```

For an agent that only launches a command, the dev stdio entry proxies to the
dev bridge:

```bash
cargo run -p guhit-mcp --bin guhit-mcp-stdio -- --port 1631
```

## The tools

Every length in every argument and every result is millimeters. Areas come
back in square metres. The plan is +x east, +y north.

| Tool | What it does |
|---|---|
| `list_projects` | Every project on this machine, newest first |
| `open_project` | Open one by id; it becomes the document every tool acts on. In a live session, `force` after the user confirms |
| `create_project` | Create and open one: `blank`, `sample-bungalow` or `plumbing-demo` (the bungalow with services). In a live session, `force` after the user confirms |
| `close_project` | Close it; the window goes back to the hub. In a live session, `force` after the user confirms |
| `get_project_summary` | Totals: areas, wall length, counts |
| `list_rooms` | Rooms with areas, perimeters and bounding wall ids |
| `list_elements` | All elements of one kind, with ids |
| `describe_elements` | Full data plus derived geometry, by id |
| `find_rooms_without_exterior_window` | Rooms with no daylight |
| `list_review_items` | Design review suggestions with their status (open, or ignored with a note), located items with a `location_mm`, and the set-aside findings that are resolved |
| `get_pipe_takeoff` | Run lengths for every system by material and size, elbows, tees, sleeves, every penetration and the aircon core holes |
| `get_schedule` | Lights, outlets, switches, fixtures and aircon units per level and room, in the rows of the PH electrical inspection form, with totals per level |
| `get_selection` | What the user has selected in the window, with full data, and whether "Only the selection" is on |
| `get_plan_image` | A picture of the plan: drawn fresh by the open window (any level), else the last thumbnail it saved |
| `list_renders` | Saved 3D visuals |
| `get_render_image` | A saved visual as a picture, fitted to 1568 px, with its record |
| `get_session` | The live session: who is in, what they have selected, the latest chat |
| `get_guide` | The conventions or the PH defaults (`topic`: `conventions` or `ph_defaults`), the same text as the two doc resources, for agents that do not read resources |
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
| `add_camera` | Save a view (position, target, field of view) to render later |
| `capture_view` | Capture the 3D view in the window, or a saved view, into Visuals |
| `render_view` | Path trace the view on screen or saved views in the window, into Visuals; a job id when it takes longer than it waits |
| `get_render_job` | Wait for a render `render_view` handed back as a job |
| `visualize_render` | An AI visualization of a saved model view, with the user's own image provider key |
| `send_chat_message` | Post to the live session chat as the user, marked as sent by AI |
| `undo`, `redo` | History, whoever made the change. In a live session someone else's step needs `force` |
| `save_version` | A named version the user can restore in the app |
| `export_plan` | PDF, SVG or DXF into the exports folder: the plan or a service sheet (`sheet`), pipes included unless `show_pipes` is false, and a page of review items in a PDF (`review_page`) |
| `batch` | Several edits atomically, as one undo step |

Every editing tool (the ones above that change the plan, `batch`,
`set_review_mark` and `add_camera`) takes an optional `scope`; see "The
selection and `scope`" below.

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

### The selection and `scope`

DECISIONS D30. The user selects parts of the plan in the window and asks
Claude Code to change only those:

> make the selected room 600 wider to the east and put a window on its new wall

`get_selection` returns what is selected: ids, kinds, readable labels, the
level on screen and the full data of each element. An editing tool called
with `"scope": "selection"` (or a list of element ids) is held to it by the
engine, not by the model: a command that reaches outside is refused with
`out_of_scope`, naming the element, and nothing is applied.

- A selected room reaches its bounding walls, their doors and windows, and
  what stands inside it. A selected wall reaches its doors and windows.
  Anything else reaches itself.
- New elements must land inside the selection's area on its level (a room's
  centerline outline, or 500 mm around other elements). In a `batch`, what an
  earlier step created joins the scope, so a later step can build on it.
- Roof, levels, layers and settings are outside every scope. What the engine
  changes as a consequence (connected walls stretching, dimensions following)
  is allowed.
- When the user switches on **Only the selection** (the copilot dock, the
  status bar, or the palette), every MCP edit is limited to their selection,
  whatever the call says; `get_selection` reports it as
  `limited_to_selection`. With nothing selected there is no limit.

### Views and renders

DECISIONS D31. The path tracer and the plan canvas run in the desktop window,
so these tools ask the window to do the work and wait for it; with no window
open (the dev bridge with no browser) they answer `no_window`.

- `add_camera` saves a view: `position` and `target` in mm, heights above the
  floor of `level`. The view shows in the app and renders by name.
- `capture_view` saves a capture of the 3D view as it is on screen, or from a
  saved view, in a second or two.
- `render_view` renders like the Render button: the view on screen or saved
  views, `quick` (about a minute at HD) or `final`, at `hd`, `qhd`, `4k` or
  `square`. It waits `wait_seconds` (45 by default) and hands back the records
  and a preview, or a `job_id` for `get_render_job`.
- `get_render_image` returns any saved visual, fitted to 1568 px.
- `visualize_render` sends a saved model view to the image provider the user
  set up in the Visuals panel (Gemini, DECISIONS D17), with the user's own key
  and at the user's cost. The result is labelled "AI visualization", keeps the
  view it came from, and never changes the model.
- `get_plan_image` asks the open window to draw the plan (any `level`), and
  falls back to the last saved thumbnail when no window answers.

### Live sessions

DECISIONS D29 and D32. When the window is in a live session, several people
edit the same plan from their own computers, on the same network or VPN or
over the internet through the relay, and every MCP edit shows up for all of
them. An MCP client on a guest's computer works too: its edits go to the host
like the guest's own.

- `get_session` lists the participants, what each has selected, the level they
  are on, and the latest chat messages.
- `send_chat_message` posts to the session chat as the user of this computer,
  marked as sent by AI.
- Undo is one shared history. `undo` and `redo` refuse a step someone else made
  (`other_author`, naming them); pass `force` only after the user confirms.
- Opening, creating or closing a project ends a session this computer hosts,
  for everyone, and closing leaves a session it joined. The window asks first;
  `open_project`, `create_project` and `close_project` refuse with
  `live_session` (naming who is in) until the call passes `force`, which a
  client does only after the user confirms. Opening the shared project itself
  changes nothing and is not refused.

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
- **Approval is the MCP client's job.** Your agent asks before it runs a tool
  and remembers what you allowed. Guhit does not add a second prompt, because
  an MCP client is a program you already trust with your machine. The in-app
  copilot is different: it never commits without you pressing Apply.
- **Loopback only.** The listener binds 127.0.0.1 and the transport refuses a
  request whose `Host` header is not local, which blocks DNS rebinding from a
  web page. There is no authentication, so anything that can already run code
  on your machine can drive the app; that is the same trust level as the dev
  bridge.
- **Two tools reach beyond this machine**, and say so with `openWorldHint`:
  `visualize_render` (the image provider, at the user's cost) and
  `send_chat_message` (the other people in a live session). Everything else
  stays on this computer. `get_session` never returns the session's invite.
- **"Only the selection" is enforced by the engine.** The model cannot talk its
  way past it; the user switches it off in the app.
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
