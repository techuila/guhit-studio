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

The quickest way is the "Connect agent" button in the app, in the editor top
bar and in the project list. It shows the address and the last agent that
connected, has one-click install links for Cursor, VS Code, LM Studio and
Goose, writes a Claude Desktop extension (`.mcpb`) that runs the stdio entry,
and has an "Allow agents" switch: while it is off, every MCP request is
refused with "Agents are turned off in Guhit Studio". The rest of this
section is the manual setup.

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

Known LM Studio bug: images returned by tools (`look`) do not reach
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
- Guhit lists 27 core tools, 46 with every toolset on (see "Toolsets"
  under "The tools"). Cursor warns above about 40 tools in total and Windsurf
  (Devin Desktop) allows 100 across all servers, so add only the toolsets you
  need. Small local models do best with the core list and a 32k context.
- Running several agents at once: give each its own labelled URL,
  `http://127.0.0.1:1450/mcp/agent/<label>`, so each gets its own name,
  color and cursor in the app.
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

MCP v2 (DECISIONS D42): a core toolset of 27 workflow tools that drafts and
checks a whole house, its PH permit drawings included (D53, D54), plus seven
opt-in toolsets. Every length in every
argument and every result is millimeters. Areas come back in square metres.
The plan is +x east, +y north; angles are degrees, counter-clockwise.

### Core toolset `draft` (always listed)

| Tool | What it does |
|---|---|
| `project` | `action` list, open (`id`), create (`name`, `template` blank, sample-bungalow or plumbing-demo), close, save_version (`label`). In a live session open, create and close need `force` after the user confirms |
| `get_guide` | `topic` conventions, workflows (drafting and the verify loop), ph_defaults, catalog, the same text as the doc resources, or swarm (several agents at once, see "Swarms") |
| `query` | `what` summary, levels, rooms, elements (`kind`, `level`, `ids`; kinds include roof, grid, view_mark, lot, tour), describe (`ids`), selection, schedules (the door and window schedule by mark, room and finish rows, areas per level, from the engine), schedule (device counts), site (the lot: area, bearings, setbacks, coverage), takeoff, catalog (`search`, `usage`), placements (free spots for a `catalog_key` in a `room`), tour (the camera at every frame, paged), materials, history. Compact rows by default; `format` full or `fields` for more; `page`, `page_size` |
| `look` | `mode` plan: the level drawn by the engine with no window, `show` dims (wall lengths), ids (short ids mapped to full ids in the result), grid, scale_bar, labels, furniture; `bbox` crops; `overlay` underlay; the result maps pixels to mm. `source` window uses the app's canvas. `mode` elevation and section: the drawing of a `mark` (id or label), hidden lines removed, `scale`. `mode` sheet: a permit `sheet` (A-1 or id) with the license numbers masked. `mode` schedule: doors_windows, rooms, finishes or areas, drawn and as rows. `mode` 3d: the 3D view or a saved `view` from that view's own camera, through the window (D31); the result's `camera` says whether the picture came from it. `mode` tour: a `tour` frame at `at_s`, through the window. Drawings are rasterized here with resvg, no window needed |
| `measure` | `kind` distance (points or ids), clear_width (two parallel walls face to face, a room, a door), height (levels, walls, openings, stairs with riser, tread and the treads of each flight), area (rooms: net and floor area; slabs, a polygon), clearance (the gap between two outlines; a stair's are its flights and landings) |
| `check` | The review items with code, element_ids, location_mm and status; `since_revision` lists what was added and resolved since then; `ids`, `codes`, `level` filter; rooms with no outside window; storeys that overlap |
| `build` | A declarative spec of `levels`, `walls`, `rooms`, `columns`, `slabs`, `stairs`, `openings`, `assets` and `cameras`, applied in that order as ONE undo step, each entry with the arguments of its tool and an optional `ref`; `dry_run`; `replace_level`. Returns every ref's id |
| `batch` | Steps `{tool, args, ref}` of any editing tool, v2 or v1, as one undo step; `"$ref"` names an earlier step's element (`"$ref.2"` its third); `dry_run` |
| `add_walls` | Walls through `points` (`closed`), `thickness_mm`, `height_mm` (a double-height room), `role` building or site, `material_id`, `level` |
| `add_room` | A named room from `rect` (south-west corner, centerlines) or `polygon`, `usage`, `floor_material_id`; draws its walls unless `walls` is false, sharing any side an existing wall covers |
| `add_opening` | A door or window by `at` {x, y} (the engine finds the wall and offset) or `wall_id` and `offset_mm`; `style` swing_single, swing_double, sliding, fixed, casement, jalousie; `swing_into` a room |
| `add_stair` | A flight from `start` up `direction_deg`, rising its level's height to `to_level`: `shape` straight, or l or u with a landing (`turn` left or right, `first_flight_risers`, `landing_mm`, u: `well_mm`; at least 4 risers, n - 2 treads); risers about 175 mm and treads 250 mm by default; a 900 mm handrail unless `railing` is false. The result gives riser height, tread depth and the flights |
| `add_column` | Columns at `at` or `points`, rect or round, `to_level` repeats them up the storeys |
| `add_slab` | `slab_kind` floor, balcony (1000 mm bars by default), deck or void (the space below is double height) from `polygon` or `rect`, with `railing` (D50) |
| `place_asset` | A library item by `catalog_key` at `at`; an unknown key answers with the nearest keys; wall items snap to a wall face unless `snap` is false |
| `add_grid` | Grid lines on every level: a system (`x_spacings_mm` for lines 1, 2, 3 going east, `y_spacings_mm` for A, B, C going north, from `origin`, `rotation_deg`) or single `lines` {label, start, end} |
| `annotate` | `kind` string: a dimension string through `points`, or through the jambs of `walls`' openings (`face`; without `offset_mm` the line goes on that face's side, outside the building for exterior), or between `grids`, `offset_mm`; the result's `parts_mm` and `overall_mm` are what was stored; strings on walls and grids follow the model. auto: the engine's exterior strings (overall, walls and grids, openings) or rooms' inside strings (`kinds`), rewritten after every edit. note: `text` at `at` |
| `add_roof` | The roof over the top floor (`outline` top_footprint: changes that roof), over a level's walls (footprint) or a drawn area (`points`: a carport, a lanai); `shape` flat, shed, gable or hip, `pitch_deg`, `ridge_axis`, `gable_sides`, overhang, fascia, gutter; `posts` puts columns under open eaves. A shed rises along `ridge_axis` toward east (+x) or north (+y); `flip` true makes it rise toward west or south, so a lanai north or east of the house slopes away from it. Pitched roofs need square corners |
| `types` | Door and window types with marks (D1, W1, D53): `action` list, set (a new type, or `type` to change one: every opening of it follows in the same step), delete, assign (every untyped opening gets a type), apply (`type` to `openings`, null untypes them). `add_opening` and `modify` take `type` too |
| `modify` | `move` {dx_mm, dy_mm} with `stretch_connected`, `rotate_deg` about `pivot`, `set` fields per kind (room name, usage, ceiling height and finishes; opening size, style and type; wall height and thickness; stair shape and turn; roof shape, pitch, `ridge_axis`, a shed's `flip` and trims; grid and mark labels; slab and object fields; materials) |
| `reshape` | `op` resize_room, set_wall_length, set_wall_endpoints, split_wall |
| `delete` | Elements by id; a wall takes its openings with it |
| `levels` | `action` list, add, update (the ground floor's height too), delete; warns when a storey's height runs past the floor above |
| `set_project` | `roof`, `site` (latitude, longitude, time zone), `north_angle_deg`, `settings` (client, designer, location, default wall thickness, scale, paper, grid) |
| `history` | `action` list (who made each step, when), undo, redo (`steps`), undo_to (`index`); someone else's step needs `force` after the user confirms |
| `export` | `format` pdf, svg or dxf of a `level` or `level` "all" (empty levels are skipped and named), dxf3d and ifc of the whole model, guhit (the bundle); glb answers `unavailable` (the window's exporter) |
| `swarm` | Several agents drawing one building at once into a draft a person accepts (D56, D57, see "Swarms"): `action` start (`brief`, `summary`, `setup`, `parts`; the caller leads and gets a handle per part), status, milestone (`name`), wait (`for`, `timeout_s` up to 60), finish (`summary`), discard (the lead, with `force`); accept is always refused `people_only` |

Every editing tool takes `scope` (see "The selection and `scope`"); every
tool takes `agent` (see "Agents").

### Toolsets

The extended toolsets are opt in:

| Toolset | Tools |
|---|---|
| `services` | `edit_pipe` (`op` move_points, segment_length, slope, insert_point, remove_point), `add_pipe` (coordination only, D19), `link_devices` (a switch to its lights) |
| `visuals` | `render` (`action` start, status, image, list; each saved view from its own camera; status without a job says what the window's render queue is doing; a window still rendering answers `window_busy` with what it is doing and when to try again), `visualize_render` (AI visualization, costs the user money: the result gives the price, agents get 20 images per project per day by default, `style_reference_id`, `use_maps`, the drift check), `set_look` (a saved visual becomes the project look AI renders follow) |
| `live` | `get_session` (the agents and the project chat, with or without a live session), `send_chat_message` (as the agent, at most 6 a minute), `set_status` (a status line, `claim` {levels, ids} and `release`) |
| `review` | `set_review_mark` (set a review item aside with a note, never approve it) |
| `interop` | `import_underlay` (the user's plan image under a level at a scale from a known dimension), `import_dxf` (`action` inspect, commit) |
| `documents` | `title_block` (`action` get: the fields, owner and location from the settings, the signing architect masked; set: project title, lot, block, TCT, firm. Only people set the architect's name and license numbers, D53), `sheets` (`action` make the permit set, list, preview, revise, export one PDF, delete, vicinity: the vicinity map image on the sheets that frame one, from `image`, a PNG or JPEG data URL stored in the project's files, or `file_name`, with `radius_m`), `view_marks` (section and elevation marks: add, list, delete), `set_lot` (from the TCT technical description as written, or `legs`; the summary states the misclosure; `sides` with roles and drawn setbacks) |
| `tours` | `make_tour` (fly in, orbit, the front door, the rooms in door order, up the stairs, day to dusk), `edit_tour` (name, fps, each shot's seconds, ease and transition, remove, order), `export_tour` (the video into Visuals, rendered in the window: a job id when it takes longer than `wait_seconds`; call again with `job_id`) |

With every toolset on the list is 45 tools, above Cursor's warning at 40:
turn on only the toolsets you need. The core list is under 44 KB of
definitions; every description is under 1 KB.

How a client turns them on, the first that applies:

1. The URL: `http://127.0.0.1:1450/mcp?toolsets=services,visuals` (or
   `?toolsets=all`).
2. The stdio proxy: `--toolsets services,visuals` (the dev binary
   `guhit-mcp-stdio` now; the desktop's `--mcp-stdio` once src-tauri passes it
   through).
3. The setting `mcp_toolsets` in settings.json, which the Connect agent dialog
   sets through `mcp_set_toolsets` (`mcp_toolsets` reads it).

Toolsets only decide what `tools/list` shows. A call to a tool that is not
listed still works, so a guide that names `render` never strands an agent.

### Results and errors

Every edit result has `ok`, `revision`, `base_revision`, `summary`,
`undo_label`, `created`, `changed` and `deleted` (id, kind, label; lists past
100 are counted), `issues_added` and `issues_resolved` since the base
revision, what it did besides elements (`types_added`, `types_changed`,
`types_removed`, `sheets_added`, `sheets_changed`, `sheets_removed`,
`revisions`, `title_block_changed`, never the signatory), `also_moved` (walls it reshaped that the call did not name),
`totals`, and where they apply `id` (what one call made), `refs`, `steps`,
`details` (a stair's riser and tread, an opening's wall and offset),
`warnings` (storeys that overlap), `scope` and `agent`.

Every failure is a tool error (`isError`) whose structured content is
`{code, message, element_ids, location_mm, step, ref, hint}`. `code` is the
engine's own (`opening_overlap`, `stretch_turns_wall`, `roof_outline`) or an
IPC code (`not_found`, `stale`, `out_of_scope`, `live_session`,
`agent_paused`, `forbidden`, `not_built`); `hint` names the tool and argument
that fixes it, and every failure has one. A failing `build` or
`batch` step names its step, its list entry (`openings[2]`) and its ref, and
nothing is applied. An unknown tool answers with the nearest name.

### The verify loop

What the workflows guide and the `verify_model` prompt teach, per level:
`build` (with `dry_run` first), read `issues_added`, `look` the plan with
`show` dims and ids, `measure` the key dimensions against the brief, `look`
mode 3d from a saved view, then `check` with `since_revision` set to the
first `base_revision`. Numbers come from `query` and `measure`, never the
model's eye (D8).

### Agents

Every MCP client working on the project is an agent with its own name,
color, cursor and status line (D42, D47, `guhit_app::agents`). The server
tells agents apart by, most specific first:

1. the `agent` argument every tool takes, a short handle such as "kitchen",
   the only way to tell subagents of one connection apart;
2. a labelled URL, `http://127.0.0.1:1450/mcp/agent/<label>` (the stdio
   proxy's `--agent <label>` posts there);
3. the stdio proxy's instance header `x-guhit-agent-instance`, one per proxy;
4. `clientInfo`. `initialize` answers with an `Mcp-Session-Id` that names
   the client; the client sends it back on every call, so its calls stay its
   own even when it opens new sockets or the app restarts. A call that names
   no client and carries no session, label or instance is an "Unknown
   agent", never the last client seen.

The registry keeps pause, stop, the limit and the rate per connection (the
session, else the label, else the stdio instance), for every handle on it.

Working together: claim your level first (`set_status` with `claim`); other
agents' edits there are refused `claimed`, and a `dry_run` of `build` or
`batch` runs the same pause, limit and claim checks, so it says so before
the commit. Write "@Name" (or "@name-with-hyphens") in `send_chat_message`. Every
result an agent gets carries `activity` (working, looking, idle, paused or
stopped), an `inbox` of at most three chat messages naming it and `nearby`,
at most five steps others made inside its claim, each told once
(`AppService::agent_notes`); `get_session` reads the whole recent chat with
or without a live session.

Pictures: `look` mode 3d and `render` take `visibility` {roof, cutaway}
(a field left out keeps roof on, no cutaway) and pass it to the window
(`WindowTask` `visibility`). Without it the window draws the model as drawn.
Each result says what the picture shows.

Each call sets the agent's status line from the tool ("Drawing walls",
"Placing doors", "Looking at the plan", "Checking the model") unless the agent
wrote its own with `set_status` in the last minute. The user pauses, stops or
limits agents in the Agents panel: a paused agent's reads still work, and its
edits and the calls that spend money or run long (`visualize_render`,
`render` start, `export_tour`) answer `agent_paused`; a stopped agent's every
call answers `agent_stopped`. An agent undoes only its own steps (`history`),
never a step that changed the signing architect, even with `force`
(`forbidden`), and its edits are checked against its limit and other agents'
claims. In-process
callers (tests) pass no hint and commit as before.

### Resources and prompts

| Resource | What |
|---|---|
| `guhit://project/current` | The open project in brief: levels, totals, rooms, review items |
| `guhit://docs/conventions`, `guhit://docs/workflows`, `guhit://docs/ph-defaults` | The guides |
| `guhit://catalog`, `guhit://materials`, `guhit://review`, `guhit://history` | The library, the project's materials, the review items, the undo history |
| `guhit://project/levels/{level}/plan.svg` (template) | A level drawn by the engine as SVG, with wall lengths, short ids, a grid and a scale bar |

Prompts: `draft_from_brief` (`brief`, `lot`), `model_from_plan_image`
(`image_path`, `known_dimension`, `level`), `verify_model` (`level`, `brief`),
`client_render_pack` (`views`) and `swarm_from_brief` (`brief`, `lot`: the
same house drawn by a swarm of sub-agents). Each spells out the verify loop.
While a swarm draft is open, an agent drawing a part reads the draft in
`guhit://project/current`, `guhit://history` and the plan template too.

### v1 tool names

The v1 names keep working for one release as hidden aliases: never listed,
answered as before, with the v2 result fields added. Each maps to its v2
tool:

| v1 | v2 |
|---|---|
| `list_projects`, `open_project`, `create_project`, `close_project`, `save_version` | `project` |
| `get_project_summary`, `list_rooms`, `list_elements`, `describe_elements`, `get_selection`, `get_schedule`, `get_pipe_takeoff` | `query` |
| `find_rooms_without_exterior_window`, `list_review_items` | `check` |
| `get_plan_image`, `capture_view` | `look` |
| `add_wall`, `add_wall_chain` | `add_walls` |
| `add_rect_room` | `add_room` |
| `add_door`, `add_window` | `add_opening` |
| `resize_room`, `set_wall_length` | `reshape` |
| `move_elements`, `rename_room`, `set_room_usage`, `set_opening_size`, `set_material` | `modify` |
| `delete_elements` | `delete` |
| `set_roof` | `set_project` |
| `add_asset` | `place_asset` |
| `add_level`, `delete_level` | `levels` |
| `undo`, `redo` | `history` |
| `export_plan` | `export` |
| `add_camera` | `build` cameras |
| `move_pipe_points`, `set_pipe_segment_length`, `set_pipe_slope`, `insert_pipe_point`, `remove_pipe_point` | `edit_pipe` |
| `render_view`, `get_render_job`, `get_render_image`, `list_renders` | `render` |

### Levels

New elements go on the first level unless a call passes `level`: an id or a
name (case does not matter), or a `build` ref to a level ("$upper"). `levels`
action add stacks a storey on the highest level ("Level N", 3000 mm floor to
floor unless given); update changes a level's name, elevation or height, the
ground floor included; delete removes a level with everything on it, only
when the user names it. A stair rises its level's height, so set that first.

### The selection and `scope`

The user can select parts of the plan in the window and ask the agent to
change only those. `query` what "selection" shows what is selected; every
editing tool, `build` and `batch` take `scope`: "selection" or element ids.
The engine refuses anything outside it with `out_of_scope`, naming the
element. When the user switches on "Only the selection" in the app, their
selection binds every edit whatever the call says (D30). Project-wide tools
(`levels`, `set_project`) are outside every scope.

### Views and renders

Views and renders run in the window (D31). Save views as `build` cameras
(name, position, target {x, y, z}, z above the level floor). `look` mode 3d
captures one into Visuals, handing the window that view's camera as the
engine holds it, and the result's `camera` says whether the picture came from
it (`matches_view`, `off_by_mm`, G4 problem 7). The window reports its render
queue as it goes (`window_status`: which view of how many, how far, how long
is left), so `render` action status and a busy refusal say what it is doing
(G4 problem 8); `render` action start path traces views and hands
back a job id when it takes longer than `wait_seconds`; `visualize_render`
makes an AI visualization from a saved view with the user's own key, only
when they ask, labelled "AI visualization", never changing the model.

### Live sessions

In a live session every edit shows up for everyone, and undo is one shared
history: `history` refuses a step someone else made (`other_author`) until
the user confirms and the agent passes `force`. `project` open, create and
close end a session this computer hosts, for everyone, and refuse with
`live_session` until `force`. `send_chat_message` posts as the agent, marked
as sent by AI.

### Swarms

One brief, several agents drawing it at once (DECISIONS D56, D57, the
`swarm` core tool, `crates/guhit-mcp/src/swarm.rs`). The lead splits the
brief into parts, its sub-agents draw the parts into a draft, and a person
accepts all of it, or part by part, or discards it in the window. Nothing
reaches the model before a person accepts; no agent can (`accept` answers
`people_only`). `get_guide` topic "swarm" and the `swarm_from_brief` prompt
teach it to the model; `docs/skills/guhit-swarm/SKILL.md` teaches Claude Code
when to use it.

**Starting.** `swarm` action start with `brief`, `summary`, `setup` {levels:
[{name, height_mm}]} (the storeys to add, as the lead's first draft step)
and 1 to 6 `parts`, each {key, name, role, instructions, scope {levels,
kinds}, after [{part, milestone}], milestones}. Levels are named by id or
name, setup levels included; an empty list means all. The app checks the
plan (`bad_args` says what to change): two parts never draw the same kind on
the same level, every wait names a declared milestone or "done", no circle
of waits, one part can start at once. The caller becomes the lead and gets a
`handle` and a `url` per part, plus `how_to` for its sub-agents. Refused
`draft_open` while a draft is open and `live_session` in a live session.

**Sub-agents.** A handle is `sw-<16 hex digits>-<key>`, 64 random bits,
never issued twice. A sub-agent on the lead's connection (Claude Code's
Task agents) passes `agent: "<handle>"` on every call; an agent in its own
process (Codex, another terminal) connects to the part's `url`,
`/mcp/agent/<handle>`. Either way the app binds it to the part on its first
call and shows it with the part's name and color. A call of the lead's
connection that names no handle is the lead's. A handle of a swarm that has
ended is refused `part_stopped`.

**Reading and writing.** Every read of a bound agent goes through one gate
(`crates/guhit-mcp/src/view.rs`): `query`, `look` (plan, elevation,
section, sheet, schedule), `measure`, `check`, `levels` list, `types` list,
the title block, sheets and marks lists, dry runs, the v1 read aliases and
the resources read the draft, which holds every part's work, so siblings
read each other. A test reads the sources and fails if a tool reads the
model around the gate. Edits land in the draft, inside the part's scope
(`out_of_scope` names the part, its levels and kinds). What the window draws
or what is written from the model shows the model, so a bound agent is
refused it as `draft_view`: `look` mode 3d, mode tour and source window,
`get_plan_image`, `capture_view`, `render` start, `visualize_render`,
`export_tour`, `export`, `sheets` export and saved versions. `history` list
shows the draft's steps; undo, redo and undo_to take back only the agent's
own draft steps (`other_author` for another part's), never the model's.

**Order and the end.** `milestone` (`name`, one the part declares) tells the
parts that wait for it; `wait` long-polls, at most 60 s a call, until `for`
{part, milestone} is reached (milestone "done" when that part finishes), or
without `for` until what the caller's part comes after is reached, or for
the lead until every part is done. It answers `reached` false with
`timed_out` when time runs out, and `ended` when the swarm is over.
`finish` (`summary`) marks the part done; the lead's finish marks the draft
ready for review. `status` lists each part (state, steps, milestones, what
it waits for, what it drew by kind) and the draft's counts. `discard` drops
the draft: only the lead, only with `force` after the user agrees
(`needs_force` without it).

**The lock.** While a draft is open the model is locked for everyone else:
another agent's edit, undo and redo are refused `swarm_running`, and so are
`project` open (of another project), create and close. Every swarm refusal
has its own hint (`swarm_running`, `draft_open`, `part_stopped`,
`draft_view`, `people_only`), never the live session's advice to pass
`force`.

## Safety

- **The engine validates everything.** A tool call is translated into the same
  typed `Command` the app's own tools and the in-app copilot produce, then run
  through `Document::preview` before it is committed. A refused edit changes
  nothing and the engine's exact message goes back to the model, so it can
  correct itself: `The door (900 mm wide, centered 9000 mm from the wall
  start) does not fit on its wall, which is 4000 mm long.`
- **One tool call is one undo step**, labelled `MCP: ...` in the app, with
  `Origin::Ai` and the agent as its author. A `build` of a whole house is one
  step too. Anything an MCP client does can be undone in the app, and
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
- **Three tools reach beyond this machine**, and say so with `openWorldHint`:
  `visualize_render` (the image provider, at the user's cost),
  `send_chat_message` and `set_status` (the other people in a live session).
  Everything else stays on this computer. `get_session` never returns the session's invite.
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
every MCP client. `project` action open switches it, saving the previous one first.
An MCP client and the window are always looking at the same plan.
