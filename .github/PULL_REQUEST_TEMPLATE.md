## What and why

<!-- What does this change, and why? Link the issue: "Closes #123". -->

## How you checked it

Tick what you ran and paste anything that failed. Details: [CONTRIBUTING.md](https://github.com/techuila/guhit-studio/blob/main/CONTRIBUTING.md#check-your-work).

- [ ] `cargo test --workspace --exclude guhit-studio`
- [ ] `cargo build -p guhit-studio`
- [ ] `pnpm gen:types` (after any change in `crates/guhit-model`; the regenerated bindings are in this pull request)
- [ ] `pnpm typecheck && pnpm build`
- [ ] `pnpm test`
- [ ] `node scripts/csp-check.mjs` or `node scripts/live-check.mjs`, if this touches what they cover

## Screenshots

<!-- For anything visible: before and after, or a short recording. Delete this section if there is nothing to see. -->

## Checklist

- [ ] I signed the [CLA](https://github.com/techuila/guhit-studio/blob/main/CLA.md) (the bot comments on the first pull request), and I own or may submit everything here.
- [ ] One purpose only; the change does what the issue asks and nothing more.
- [ ] The engine stays the only authority over the model: edits are typed commands, and engine changes have tests.
- [ ] Contract files I changed (`crates/guhit-model`, `src/contract`, `src/state`, tokens, motion, `docs/CONTRACT.md`) are changed on purpose, with every consumer updated.
- [ ] New interface behavior animates per [docs/MOTION.md](https://github.com/techuila/guhit-studio/blob/main/docs/MOTION.md) and respects reduced motion.
- [ ] New 3D assets are CC0 and listed in `assets/ASSETS.md`; any new dependency has a reason and an AGPL-compatible license.
- [ ] Plain language, hyphens and no em or en dashes; commits are `type(scope): subject`.
