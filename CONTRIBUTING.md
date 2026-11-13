# Contributing to Guhit Studio

Thank you for wanting to help. Guhit Studio is a desktop app for drawing floor plans, seeing them in 3D and working with an AI copilot or your own MCP agent. Bug reports, fixes, new catalog objects, review checks, translations and docs are all welcome.

| | |
|---|---|
| Found a bug or have an idea | [Open an issue](https://github.com/techuila/guhit-studio/issues/new/choose) |
| Found a security problem | Do not open an issue. Follow [SECURITY.md](SECURITY.md) |
| Want to send code | Read this page, then open a pull request |
| How we treat each other | [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) |

## Before you start

- **License.** The code is [AGPL-3.0-only](LICENSE). By contributing you agree your work can be shared under it. See [NOTICE](NOTICE) for the other licenses in the tree.
- **CLA.** Every contributor signs the [contributor license agreement](CLA.md) once (details below).
- **Talk first for anything big.** For a small fix, open the pull request. For a new feature, a change to how the model or the file format works, or a large refactor, open an issue first and describe what you want to do. Some decisions about the product are already settled, and the maintainers keep them in a private decision log. If your idea cuts across one, we will say so in the issue and explain why, before you spend your time.
- **Name and logo.** The Guhit name and logo are trademarks, and the AGPL grants no trademark rights. See [TRADEMARKS.md](TRADEMARKS.md) if you plan to fork.

## Set up

You need Rust (stable, 1.87 or newer), Node 22 or newer, pnpm 11, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your system (Xcode Command Line Tools on macOS; the Visual Studio C++ build tools and WebView2 on Windows).

```bash
git clone https://github.com/techuila/guhit-studio.git
cd guhit-studio
pnpm install
pnpm tauri dev        # the desktop app, with hot reload
```

You can also run the whole app in a normal browser, with the real Rust engine behind it. This is the quickest way to work on the interface:

```bash
pnpm bridge           # terminal 1: the Rust backend on port 1430
pnpm dev              # terminal 2: the interface on port 1420
```

If the macOS linker says "You have not agreed to the Xcode license", run `sudo xcodebuild -license` once.

## Check your work

Run these before you open a pull request. They are the same checks our CI runs on macOS and Windows.

| Command | What it proves |
|---|---|
| `cargo test --workspace --exclude guhit-studio` | The engine, exports, imports, app service and MCP server pass their tests |
| `cargo build -p guhit-studio` | The desktop shell compiles |
| `pnpm gen:types` | Run it after any change in `crates/guhit-model`. It regenerates `src/contract/bindings/`, which you never edit by hand |
| `pnpm typecheck && pnpm build` | The interface type-checks and builds |
| `pnpm test` | The interface unit tests pass |

More checks are worth running when you touch what they cover:

- `node scripts/ui-check.mjs` drives the scripted browser checks of the 2D editor and 3D viewer (the step files are under `src/editor2d/dev/checks/`).
- `node scripts/csp-check.mjs` loads the built interface under the release security policy. Start `pnpm bridge` first.
- `node scripts/live-check.mjs` runs two browser tabs in a live session through a local relay. Build `cargo build -p guhit-devbridge -p guhit-relay` first. Run it if you change live sessions or MCP.

Say in your pull request which checks you ran and what they printed.

## The rules of the code

These keep the app consistent. Reviewers will point at them.

- **The Rust `Document` is the only authority over the model.** The interface never changes it directly. It sends a typed command, and every command is deterministic, validated and one undo step. The AI uses the same commands as the interface.
- **Units.** Lengths are millimeters, plan +x is east and +y is north, angles are degrees counter-clockwise. [docs/CONTRACT.md](docs/CONTRACT.md) has the conventions.
- **Contract files change on purpose.** `crates/guhit-model`, `src/contract/ipc.ts`, `src/contract/pipes.ts`, `src/state/store.ts`, `src/state/bus.ts`, `src/styles/tokens.css`, `src/ui/motion.ts`, `docs/CONTRACT.md` and `docs/MOTION.md` change only with a stated reason, regenerated bindings and every consumer updated in the same pull request.
- **Interface style.** Use the CSS variables in `src/styles/tokens.css` and CSS modules. No new styling framework.
- **Motion.** Every interaction animates, following [docs/MOTION.md](docs/MOTION.md): motion tokens only, drags track the pointer, exits animate, `prefers-reduced-motion` is respected, no animation library.
- **3D frame loop.** There is exactly one pending `requestAnimationFrame`, scheduled only through `ViewerEngine.schedule()`.
- **Assets.** Third-party 3D assets must be CC0 and listed in [assets/ASSETS.md](assets/ASSETS.md). Poly Haven, Kenney and ambientCG are the approved sources.
- **Dependencies.** Add a dependency only with a reason in your pull request. Its license must be compatible with the AGPL-3.0 (MIT, Apache-2.0, BSD, ISC, MPL-2.0 and similar are fine; ask about anything else). Code, text or assets that you copy into the repository are stricter: they must come under MIT, BSD, Apache-2.0, ISC, zlib, CC0 or a similar permissive license, and you name the source and license in the pull request (CLA section 5).
- **Safety claims.** Review items and pipe layouts are suggestions and coordination aids. Never word anything as permit approval, structural certification, code compliance or pipe sizing.
- **Works on macOS and Windows.** No platform-specific paths or shell calls in app code.
- **Writing.** Plain, direct language in code comments, docs and interface copy. Use a hyphen, never an em dash or an en dash.
- **Commits.** `type(scope): subject`, lowercase, imperative, under 72 characters, for example `fix(editor2d): keep the door ghost on the wall`. Types: `feat`, `fix`, `refactor`, `perf`, `chore`, `docs`, `test`, `style`, `build`, `ci`. No body unless the change is not obvious.

The maintainers keep working notes for AI coding agents (`AGENTS.md`, `CLAUDE.md`, `DECISIONS.md`) on their own computers; they are not in this repository and you do not need them. This page, the [README](README.md) and the files in [docs/](docs/) are everything a contributor needs. If you use an AI tool, point it at them.

## Good first contributions

- A new catalog object: a 2D symbol, a 3D form and a CC0 model.
- A material preset.
- A review check.
- A DXF drawing the wall recognizer gets wrong, as a test fixture (only one you have the right to share).
- A translation of interface copy.
- A clearer doc.

## The CLA

Before we merge your first pull request you need to agree to the [contributor license agreement](CLA.md). A bot comments on your pull request with a link; you confirm with your GitHub account once, and that covers your later pull requests too.

In plain words:

- You keep the copyright in your work. It is a license, not an assignment.
- You allow Aliteo to distribute your work under the AGPL-3.0, and also under other terms if it chooses (for example a commercial license for companies that cannot use the AGPL).
- Aliteo promises to keep the public code available under an open source license approved by the OSI.

**The CLA text is a draft that a lawyer must review before it is used.** Until that review is done and the CLA bot is switched on, we cannot merge code from outside contributors. Issues, discussion and early drafts of a pull request are welcome in the meantime.

## How a pull request is reviewed

1. **Open it against `main`**, from a branch of your fork. Keep it to one purpose. A short pull request is reviewed faster than a long one.
2. **Fill in the template.** Link the issue, say what you changed and why, list the checks you ran, and add screenshots for anything visible.
3. **CI must pass.** Four checks run (test and desktop, each on macOS and on Windows), plus the CLA check. A pull request merges only when all are green.
4. **A maintainer reviews it.** We try to reply within a week. Expect questions and requested changes; that is normal and not a judgment of you. Push new commits to the same branch rather than opening a new pull request.
5. **What we look for:** it solves the problem the issue describes and nothing more, it follows the rules above, engine changes come with tests, and the change is small enough to understand. You are responsible for everything you submit, including code an AI tool wrote: read it, understand it, and make sure it does not copy material you have no right to submit.
6. **A maintainer merges it.** Releases are cut by the maintainers with a tag; see [docs/RELEASING.md](docs/RELEASING.md).

## Questions

Open an issue. If it is about how to use the app rather than a bug, say so in the title.
