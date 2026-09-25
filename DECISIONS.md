# Decisions

Locked decisions. Read before proposing or building. Do not re-litigate.
Newest at the bottom. Format: what was chosen, what was rejected, why.

## 2026-09-22

### D1. Desktop application for macOS and Windows, built with Rust
- Chosen by: Axl.
- Rejected: web-first SaaS (the research's default recommendation).
- Consequence: cloud features from the research (share links, client comments, cloud sync, billing) are out of scope until a backend exists. Local files are the persistence layer.

### D2. Framework: Tauri 2 (Rust backend, React + TypeScript frontend)
- Chosen by: Claude, under Axl's instruction to pick the framework that fits.
- Rejected: egui, Iced, Slint (pure Rust GUI: would mean hand-writing the 2D editor UI and a wgpu 3D renderer, no Three.js, far slower to ship); Dioxus desktop (also a webview, younger tooling); Electron (not Rust).
- Why: the research's recommended stack is React + Canvas/WebGL + Three.js and it names Tauri for desktop packaging. Tauri keeps that stack, ships small native installers for macOS and Windows, and lets the model engine be real Rust.

### D3. The Rust engine is the only authority over the model
- Chosen by: Claude.
- The project model, commands, undo, room detection, review checks, queries and exports live in Rust crates with tests. The frontend holds a read-only mirror and sends typed `Command`s. The AI copilot uses the same commands.
- Rejected: model logic in TypeScript (the research's web-first layout). Geometry bugs are the top trust risk in the research, so geometry lives where it can be tested deterministically.

### D4. One IPC entry point, plus a dev HTTP bridge
- Chosen by: Claude.
- All UI calls go through `AppService::handle(cmd, args)` in `guhit-app`. Tauri exposes it as one command. `guhit-devbridge` exposes the same function over HTTP so the full app runs in a plain browser for development and automated checks.
- Rejected: one Tauri command per feature (cannot be exercised outside the desktop webview).

### D5. Units and coordinates
- Chosen by: Claude.
- All stored lengths are f64 millimeters. Plan +x east, +y north. Angles in degrees, counter-clockwise. Display unit is a view setting only.

### D6. Undo is snapshot based
- Chosen by: Claude.
- One undo restores the exact pre-command project. Rejected: inverse commands (bug surface, no benefit at this model size).

### D7. Rooms are seed based
- Chosen by: Claude.
- A `Room` stores a name and a seed point; its polygon is derived from the closed wall face containing the seed. Names survive wall edits. Closed faces without a room get one automatically inside the same undo step.

### D8. AI copilot uses Claude with strict tool calling; nothing commits without approval
- Chosen by: Claude.
- Pipeline: intent, tool calls, validate, preview (ghost diff), user accepts, commit, log. Answers to model questions come from `Query` results, never from the language model's estimate.
- The API key is stored in the OS keychain and never sent to the frontend.

### D9. AI render provider: OPEN
- Tier 1 (deterministic 3D capture, tied to revision and camera) is in scope now. Tier 2 (generative image conditioned on the view) needs Axl to pick a hosted image provider. The provider abstraction is built; no provider is wired.

### D10. Subagents run on Opus 5 or Sonnet, never Fable 5.1
- Chosen by: Axl. Opus for hard work (engine, geometry, canvas, review), Sonnet for routine work.

### D11. Every interaction has a microanimation
- Chosen by: Axl. Hover, press, drag, select, place, open, close, toggle: all animate, with smooth transitions.
- How: one motion system (tokens in `src/styles/tokens.css`, rules in `docs/MOTION.md`), guided by the `design-motion-principles` skill installed at `.agents/skills/` (reviewed by Claude before use: instructions only, no scripts or network calls).
- Guard rails (Claude): drags track the pointer 1:1, only pick-up and drop animate. Keyboard-triggered and high-frequency actions still animate but stay under 100 ms so the tool never feels slower. `prefers-reduced-motion` turns motion off. No animation library: CSS, the Web Animations API and requestAnimationFrame only.
- Rejected: Framer Motion / Motion (new dependency and bundle weight for what CSS + WAAPI cover here).

### D12. Dimensions follow the geometry they were snapped to
- Chosen by: Claude, after the final review showed a dimension reading 8000 on a wall the AI had just made 8300.
- A dimension endpoint within 1 mm of a wall joint or wall outline corner moves with it, inside the same command and undo step, even on a locked Dimensions layer. Nothing new is stored in the model.
- Rejected: leaving dimensions static until a full associative-dimension system exists. A dimension that disagrees with the drawing is worse than none.

### D13. The Claude API key lives in a private file, not the keychain, until the app is signed
- Chosen by: Claude, after Axl reported a keychain permission prompt on every save and chat.
- Cause: unsigned builds get a new code identity per build, so macOS re-asks every time. The keychain store stays in the code for a future signed build.
- Also: only Console API keys (`sk-ant-api...`) are accepted. Claude subscription and Claude Code tokens are refused with a clear message; they are for Claude apps only and using them in another product is not permitted.

### D14. Subscriptions drive Guhit through MCP, never through copied tokens
- Chosen by: Axl (the goal: use an existing Claude subscription with Guhit's floor plan context) and Claude (the mechanism).
- Guhit Studio exposes its engine as an MCP server on localhost. Claude Code, Codex, Cursor or any MCP client drives it with the user's own subscription, inside that client. The desktop app updates live.
- Confirmed by Axl on 2026-09-22: guhit-mcp is the way subscriptions are used.
- Rejected: pasting a Claude Code or subscription token into the in-app copilot. Those credentials are for the provider's own products; the API rejects them and the terms forbid it. The in-app copilot keeps using Console API keys (D13).

### D15. A dedicated copilot model is future work
- Chosen by: Axl. The in-app copilot will get its own dedicated model later (fine-tuned or specialised for floor plans and PH practice). Not scoped now.
- Until then the copilot uses Claude through a Console API key (D13), and Claude Code through guhit-mcp (D14).

### D16. Interoperability: open formats first, DWG through an external converter
- Chosen by: Axl (the project must transfer easily to and from AutoCAD and SketchUp) and Claude (the formats).
- Export: DXF 2D and 3D, IFC4, glTF/GLB, OBJ, Collada DAE, PDF/SVG, and a `.guhit` bundle. Import: DXF/DWG as recognized walls or as linework, glTF/GLB/OBJ as reference models, `.guhit` bundles.
- DWG: read and written only through the free ODA File Converter that the user installs; the app detects it. Rejected: libredwg (GPL, would force the app open source) and the Autodesk RealDWG SDK (paid licence, Windows only).
- SketchUp `.skp`: not written or read directly (proprietary SDK, no Rust binding). SketchUp reads DAE, OBJ, glTF, DXF and IFC, which the app writes.

### D17. AI visualization provider: Google Gemini image models first, behind a provider abstraction
- Chosen by: Axl (asked for a realistic render with a before/after slider, showing a Gemini-made app as the reference) and Claude (the provider).
- Gemini 3.1 Flash Image by default ($0.045 to $0.151 per image), Gemini 3 Pro Image as the high setting. The user brings a Google AI Studio API key, stored like the Claude key (D13). SynthID watermark stays.
- Every AI image is a `RenderRecord` with `source_render_id` pointing at the exact model capture it was conditioned on, and the UI always offers the side by side slider. The model is never edited from an image (research section 10).
- Rejected for now: Higgsfield API (a second account for the same models), running our own GPU (research section 9).

### D18. Landing page and its intro
- Chosen by: Axl. Public site on GitHub Pages from `site/` in this repo, no build step, no framework. Theme: clean, secure, minimalist, a visible drafting grid, scroll acts that explain the app with real numbers and real screenshots.
- Intro on first visit is the page loader (about 6.5 s, skippable, once per session, a fade under reduced motion): navy field, the grid draws one line at a time from the center outward (verticals bottom to top, horizontals left to right), the logo draws stroke by stroke with pauses so the G reads, then the field shrinks into the mark's place while the word GUHIT is outlined from G to T and filled behind the stroke. The collapse waits for fonts and above-the-fold images; while waiting it holds on the finished G with a progress line (4 s cap). Axl rejected the first 2.9 s version as too fast to absorb.
- Changed by Axl on 2026-09-25: the grid no longer draws one line at a time from the center. Every line starts in a random place at its own moment, evenly paced, and the grid is complete in 1.4 s instead of 2.4 s (directions unchanged). GUHIT is written only after the mark has landed in its place, never while the field is still moving, and the rest of the hero rises in after it; nothing in the hero shows during the collapse. Whole sequence about 6.3 s.
- Rule from a real bug: the act 02 3D house stage never clips (`overflow: visible`), its size derives from the projected model extent, and every plane must clear the stage edge at every scroll position and width (checked by script).
- Copy states only what the app does. Rejected: percentage "geometry accuracy" claims, a hip roof preset the app does not have.

## 2026-09-23

### D19. Walk mode and a plumbing layer, built now
- Chosen by: Axl ("proceed" on the plumbing walkthrough concept). Walk mode, the plumbing layer, coordination checks and the pipe take-off are built now, before the first release.
- Walk mode: walk (eye height, walls block, doors pass) and fly in the live 3D view, with a minimap. X-ray and hidden shell modes keep pipes visible.
- Plumbing: cold water, hot water, drainage and vent runs drawn in 2D, shown in 3D, one layer per system. Review items for pipes through columns and door or window openings, crossing pipes, drain slopes under the default, and penetrations that need sleeves or flashing. Take-off by system, material and size, with elbows, tees and sleeves. Pipes go into DXF, 3D DXF, IFC4 and plan sheets.
- Boundary: Guhit coordinates pipes, it does not size them or run hydraulics. Plumbing plans are signed by a registered Master Plumber (RA 1378). Pipe checks are suggestions like every review item.
- Open: electrical conduits or aircon lines next. The copilot routing pipes stays later.

### D20. Go signal delegated: research, then build the open items
- Chosen by: Axl ("proceed with what we are missing ... continue without my go signal, also implement it"), on 2026-09-23.
- Scope: the open items after D19. Rendering phases (sun study, rest mode, night lighting, a Render button, Blender as an optional renderer), electrical and aircon on the services model, and the walk and plumbing gaps (stairs and levels, IFC pipe fittings, label overlap, desktop checks).
- Method: study how existing apps do each feature and what users ask for (Reddit and forums), copy what works and improve it. Research notes live in the orchestrator's working notes; choices that change the product are recorded here as they are made.
- The D19 boundary holds for every new service: Guhit coordinates, licensed professionals design and sign.

### D21. Electrical and aircon ride on the existing models
- Chosen by: Claude, under D20, from the MEP research (Chief Architect, Revit, Cedreo, PH practice).
- Runs: storm drainage, electrical conduit, aircon refrigerant line sets and condensate are new `PipeSystem`s with their own layers (`storm`, `electrical`, `aircon`), reusing risers, fittings, penetrations, checks and take-off.
- Devices: outlets, switches, fixtures, panelboards, detectors and aircon units are `Asset`s with new categories. The catalog says how each mounts, which PH inspection-form row it counts under, the light it gives and the aircon limits. Switches link to their lights (`Asset::links`); two switches on one light make a 3-way.
- Checks and counts are suggestions: missing switches, switches behind doors, aircon without an outlet, line sets outside the manual's limits, condensate fall, unit clearances. Schedules count devices per room in the rows of the PH electrical inspection form.
- Deleting an object removes it from every `links` list in the same step, even when the linking object is on a locked layer (the same idea as D12: links follow what they point at).
- Rejected: a separate device element (it would duplicate every asset path); circuits, loads, breaker and wire sizing (the PEE's work under RA 7920); aircon sizing (the PME's); gas runs for now (most PH homes use a cylinder and hose; the LPG cylinder is a catalog object).
- The rail's Pipe tool is now Services (key P stays): one flyout grouped by trade, with the professional who sizes each (Master Plumber, PEE, PME).

### D22. Sun and light
- Chosen by: Claude, under D20, from the render UX research.
- The project gets a site (PH city presets, latitude, longitude, UTC+8), Manila by default. The live view has a Sun control with presets, U and I to scrub, a physical sky, auto exposure and a sun path overlay. A saved view keeps its time, sky, exposure and lamps, which SketchUp and Enscape users have asked for.
- Fixtures give light when placed (900 lm default, a 9 W LED bulb). Rooms with no fixture get a view-only ghost light at night so interiors are never pitch black, the top complaint in Enscape and Twinmotion forums.
- The live view refines when the camera rests (jittered frames, soft sun shadows) inside the one-rAF loop.

### D23. The Render button uses a WebGL 2 path tracer now
- Chosen by: Claude, under D20.
- Rendering uses three-gpu-pathtracer 0.0.24 (MIT, with three-mesh-bvh, MIT), lazy loaded, in an offscreen renderer: HD, QHD, 4K or square, quick or final, denoised, saved to Visuals with its camera and light, and offered to AI visualization (D17). It runs on every Mac and PC the app supports; Twinmotion and D5 path tracers skip the Mac.
- Rejected for now: the WebGPU path tracer with OIDN (unreleased; WebGPU needs macOS 26), Blender as an external renderer (needs a 350 MB install and a GPL script; later), baked bounce light and a WebGPU live view (later, after a benchmark).
- Known risk: the WebGL tracer's maintainer plans to replace it with the WebGPU one. The render module keeps the tracer behind one interface so it can be swapped.

### D24. Review items can be set aside, never approved
- Chosen by: Claude, under D20, from the coordination research (Navisworks, BIMcollab, Solibri).
- A finding, a whole check, or a check on one element can be set aside with a note (stored in the project, one undo step). Resolved is derived when the check stops finding it. The list groups by level and room. No status reads "approved" or anything like it (AGENTS.md: review items are suggestions).

### D25. Deferred after the D20 research
- Chosen by: Claude. Recorded so they are not lost: recorded walk tours and video export, a one-file offline walkthrough for clients (needs Axl's decision, D1 and D16), VR, automatic outlet placement, copilot routing of runs, facade sun hours, a perspective placed on the permit sheet, Blender rendering, baked bounce light.

### D26. Levels and ceilings
- Chosen by: Claude, under D20.
- A level is added above the top one (its elevation is the top level's elevation plus its height), renamed in place, and deleted with an inline confirmation that counts the elements that go with it. One undo step brings it back. The last level cannot be deleted.
- Ceiling fixtures hang from the level height, or from the underside of the next level's 200 mm slab when that is lower, so they never sit inside the floor above.
- The part of a level that the next level up does not cover gets a flat 200 mm concrete roof deck at its ceiling in the 3D view (it hides with the roof), the common PH roof deck. The top level keeps the project roof. Rejected for now: pitched lower roofs, which need their own roof settings.

### D27. The webview CSP allows WebAssembly and the app's own fetches
- Chosen by: Claude, under D20, after a check of the build under the release CSP.
- Found: the shipped policy refused three things the browser dev setup never showed. WebAssembly (the meshopt decoder of the asset pack), `fetch` of the app's own files (the pack manifest, the sky HDRI) and `blob:` fetches (textures inside GLB models). So installed copies drew procedural stand-ins and no photo sky.
- Chosen: `script-src` adds `'wasm-unsafe-eval'` (WebAssembly only, JavaScript eval stays refused) and `connect-src` adds `'self' blob:`. No new external origin.
- `scripts/csp-check.mjs` serves a build under the release policy and fails on any refusal; it is part of Verify.

### D28. Mac releases are signed with the Developer ID and notarized
- Chosen by: Axl (has the Apple Developer Program), after the first tester download was refused by Gatekeeper on 2026-09-25.
- The release workflow signs both Mac builds with the Developer ID Application certificate and notarizes them when the Apple secrets are set. Without them it falls back to ad-hoc signing and warns. Local builds stay ad-hoc (`signingIdentity` "-").
- The certificate's owner name and team stay out of the repo: the workflow passes `APPLE_SIGNING_IDENTITY=Developer ID Application`, which Tauri matches against the imported certificate.
- One certificate for every Aliteo Mac app, chosen by Axl on 2026-09-25: Guhit reuses TopNotch's Developer ID Application certificate, and the secrets have TopNotch's names (`MACOS_CERT_P12`, `MACOS_CERT_PASSWORD`, `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID`). Rejected: a certificate per project (nothing gained, more renewals). That certificate comes from the Previous Sub-CA and stops signing on 2027-02-01; the replacement should be made with the G2 Sub-CA and updated in every repo's two certificate secrets.
- Windows code signing stays open (no certificate yet).
