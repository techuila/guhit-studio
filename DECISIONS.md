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
