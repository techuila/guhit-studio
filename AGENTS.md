# Guhit Studio

PH-first, AI-native architectural design desktop app. Draw a real floor plan
in minutes, see it in 3D instantly, ask AI to make revisions, produce a
client-ready visual, in one workspace. Source of product truth: `docs/product-context.md`, a summary of the
confidential market research PDF, which stays out of the repo.

## Session protocol

Start:
1. Read `DECISIONS.md`. Locked decisions are not up for debate.
2. Read `docs/CONTRACT.md` before touching anything that crosses a crate or the IPC boundary.

Stop:
1. Run the checks in "Verify" and report real output. Do not claim done on unverified work.
2. Append any decision the user made to `DECISIONS.md`.
3. Write durable project knowledge here or in `docs/`, not in tool-specific memory.

## Repo map

| Path | What | Notes |
|---|---|---|
| `crates/guhit-model` | Contract types, built-in materials, asset catalog | Types only. Source of the TS bindings. |
| `crates/guhit-core` | `Document` engine: commands, undo, joins, rooms, checks, queries | No I/O. Heavily tested. |
| `crates/guhit-export` | Plan to SVG, PDF, DXF | Depends on `guhit-model` only. |
| `crates/guhit-import` | DXF to walls or linework | Pure, no file I/O. Rules and limits: `docs/INTEROP.md`. |
| `crates/guhit-app` | App service: project store, session, exports, AI (`src/ai/`) | No Tauri dependency. Single entry `AppService::handle`. |
| `crates/guhit-mcp` | MCP server over `guhit-app`, for Claude Code and other MCP clients | `docs/MCP.md`. Reuses the copilot's tool translation. |
| `crates/guhit-devbridge` | Dev-only HTTP transport over `guhit-app` | Port 1430. Also serves `/mcp`. |
| `src-tauri` | Desktop shell | One `ipc` command. No logic. Hosts `/mcp` on 127.0.0.1:1450. |
| `src/contract` | `ipc.ts` client and generated `bindings/` | Never hand-edit `bindings/`. |
| `src/state` | Zustand store and event bus | Frontend join point. |
| `src/shell`, `src/hub` | App frame, inspector, palette, project hub | Keyboard shortcuts: `docs/SHORTCUTS.md` |
| `src/editor2d` | 2D plan canvas and tools | |
| `src/viewer3d` | Three.js live 3D | |
| `src/ai` | Copilot dock | |
| `fixtures/` | Golden sample project | Regenerate: `cargo run -p guhit-core --example gen_fixture` |

## Standing constraints

- The Rust `Document` is the only authority over the model. The frontend never mutates `doc`; it sends a `Command` through `useApp().dispatch`.
- Every mutation is a typed `Command`: deterministic, validated, one undo step. The AI uses the same commands as the UI and never commits without user approval.
- All lengths are millimeters (f64). Plan +x east, +y north. Degrees, counter-clockwise.
- Geometry is authoritative, AI imagery is derivative. AI output is labelled "AI visualization" and never writes back into the model.
- Review items are suggestions. Never present anything as permit approval, structural certification or code compliance.
- Contract files (`crates/guhit-model/**`, `src/contract/ipc.ts`, `src/state/store.ts`, `src/state/bus.ts`, `src/styles/tokens.css`, `src/ui/motion.ts`, `docs/CONTRACT.md`, `docs/MOTION.md`) change only deliberately, with bindings regenerated and every consumer updated in the same change.
- The webview runs under the CSP in `src-tauri/tauri.conf.json` (`csp` for release, `devCsp` for Vite). New external origins, inline scripts or eval are refused by it; extend the policy deliberately instead of loosening it.
- Must build and run on macOS and Windows. No platform-specific paths or shell calls in app code.

## House rules

- No em dashes or en dashes anywhere. Use a hyphen.
- Plain, direct language in code comments, docs and UI copy.
- Commits: `type(scope): subject`, lowercase, imperative, under 72 chars, no body unless non-obvious, no AI attribution. Commit only when asked.
- Every interaction has a microanimation. Follow `docs/MOTION.md`: motion tokens only, drags track 1:1, exits animate, `prefers-reduced-motion` respected, no animation library. Helpers: `src/ui/motion.ts`.
- 3D frame loop invariant: exactly one pending requestAnimationFrame, ever, scheduled only through `ViewerEngine.schedule()`. A second scheduling path once doubled renders per frame and starved input. Shadow maps redraw only on explicit invalidation. Measure with `node scripts/perf-3d.mjs` (real GPU, headed Chromium).
- 3D assets: only CC0 files from Poly Haven, Kenney and ambientCG, listed in `assets/ASSETS.md` and `public/assets/pack/manifest.json`. Rebuild with `node scripts/assets-build.mjs`. Nothing from Sketchfab or unlisted sources.
- UI styling uses the CSS variables in `src/styles/tokens.css` and CSS modules. No new styling framework.
- No new dependency without a reason stated in the change.

## Verify

```bash
cargo test --workspace --exclude guhit-studio   # engine, export, app service
cargo build -p guhit-studio                     # desktop shell compiles
pnpm gen:types                                  # after any guhit-model change
pnpm typecheck && pnpm build                    # frontend
```

Run the full app in a browser (real Rust engine, no desktop shell):

```bash
pnpm bridge        # terminal 1: HTTP bridge on :1430
pnpm dev           # terminal 2: UI on :1420
```

Run the desktop app: `pnpm tauri dev` (dev builds print one `ipc <cmd> -> ok|error` line per call). Build installers: `pnpm tauri build` (macOS: `--bundles app,dmg`, output under `target/release/bundle/`; unsigned until a Developer ID is configured). Unsigned builds get a new ad-hoc identity on every build, so macOS shows a keychain permission prompt the first time each new build reads the stored Claude API key: click Always Allow. A Developer ID signed build has a stable identity and asks once.
(macOS builds on macOS, Windows builds on Windows; CI does both). CI runs on macOS and Windows only: the keychain dependency needs extra system packages on Linux.

If the linker fails with "You have not agreed to the Xcode license", either
run `sudo xcodebuild -license` once, or prefix commands with
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`.
