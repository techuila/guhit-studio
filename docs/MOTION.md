# Motion

Every interaction in Guhit Studio has a microanimation (DECISIONS D11). This file is the rulebook. Tokens live in `src/styles/tokens.css`. Craft guidance: the `design-motion-principles` skill in `.agents/skills/` (weight it as a productivity tool: restraint and speed first, polish second, play only in rare moments).

## Rules

1. Motion explains, it never delays. Input is never blocked while something animates. Every animation is interruptible and retargets from its current value.
2. Drags track the pointer 1:1, with zero smoothing or lag. Only the pick-up (lift) and the drop (settle) animate.
3. Duration follows frequency. Use the tokens, never raw numbers.

| Token | Value | Use |
|---|---|---|
| `--dur-press` | 80 ms | pointer-down feedback, keyboard-triggered changes |
| `--dur-hover` | 120 ms | hover, focus ring, small state changes |
| `--dur-base` | 180 ms | popovers, flyouts, tabs, toggles, sliding indicators |
| `--dur-panel` | 240 ms | dialogs, dock, inspector sections, view mode change |
| `--dur-scene` | 360 ms | rare: open project, AI proposal arrives, fit view |

4. Easing: `--ease-out` for anything entering or answering input, `--ease-in` for leaving (and leave faster than you enter: about 70% of the enter duration), `--ease-in-out` for moving between two places, `--ease-spring` only for things that land (placed object, toggle thumb, check mark).
5. Animate `transform` and `opacity` (and `clip-path`, `filter` sparingly). Never animate layout properties in a loop. Height reveals use grid-template-rows 0fr -> 1fr or a measured WAAPI animation.
6. Exits are animated too. Nothing pops out of existence: popovers, dialogs, toasts, flyouts, list rows, canvas elements.
7. Origin-aware: a flyout grows from its button, a dialog rises from slightly below, a toast slides from its edge, a tooltip fades from its anchor side.
8. One thing moves at a time per region. Stagger lists only on first appearance, at most 30 ms per item, at most 8 items.
9. `prefers-reduced-motion: reduce` sets all duration tokens to ~0. Canvas and three.js code must check `motionOK()` and jump to the end state.
10. No animation library. CSS transitions/keyframes, the Web Animations API, and requestAnimationFrame. Shared helpers live in `src/ui`: `motion.ts` (tokens in JS, easing, tween, usePresence), `motionDom.tsx` (Presence, sliding indicator, flash), `motionWaapi.ts` (grow in, collapse out, FLIP, origin transforms). Canvas and three.js keep their own small animators (`src/editor2d/anim.ts`, `src/viewer3d/engine/animator.ts`).
11. Purple (`--ai`) motion is reserved for AI: the proposal ghost breathes slowly, nothing else pulses.

## Inventory

| Surface | Interaction | Motion |
|---|---|---|
| All buttons, rows, chips | hover / press / focus | background and border fade (`--dur-hover`), press scales to `--press-scale` (`--dur-press`), focus ring fades and expands 2 px |
| Tool rail | tool change | one active pill slides between tools (`--dur-base`, `--ease-in-out`), icon nudges 1 px on hover |
| Segmented controls, tabs | change | thumb or underline slides and resizes to the new item |
| Toggles, checkboxes | change | thumb slides with `--ease-spring`, check mark draws (stroke-dashoffset) |
| Flyouts, menus, popovers, tooltips | open / close | scale 0.96 -> 1 + fade from the anchor origin, exit faster |
| Dialogs, command palette | open / close | backdrop fades, panel rises 8 px + fades (`--dur-panel`); palette rows highlight slides between rows |
| Toasts | enter / leave / stack | slide in from the edge, siblings move up smoothly, leave with fade + slide |
| Inspector | selection change / section collapse | content cross-fades with a 4 px slide, sections animate height, a committed number field flashes teal briefly, an invalid entry shakes 4 px once |
| Dock, split divider | collapse / drag | dock height animates, divider thickens and tints on hover, panes never lag the drag |
| Top bar | save status, undo/redo | "Saving" spinner cross-fades to a drawn check mark; undo/redo icon rotates 20 degrees on press |
| Hub | cards, create, open | cards rise on first load (stagger), lift 2 px with a larger shadow on hover while the thumbnail scales 1.03, press sinks; opening a project cross-fades hub to editor (`--dur-scene`) |
| 2D canvas | hover / select | hover tint fades in, selection outline fades in, grips scale in from 0 with a stagger |
| 2D canvas | snapping | snap glyph pops (scale 0.6 -> 1), guide lines fade in and out |
| 2D canvas | place / delete / undo | a placed element settles (brief scale or ink flash from the click point), a deleted element fades out, elements changed by undo/redo flash once |
| 2D canvas | drag | 1:1 tracking; on pick-up the element lifts (soft shadow, slight tint), on drop it settles; a rejected drop eases back to the origin |
| 2D canvas | view | zoom to fit and focus elements ease the view (`--dur-scene`); repeated keyboard zoom steps ease at `--dur-press` and retarget; wheel zoom and pan stay immediate |
| 2D canvas | marquee, type-to-precise box | marquee fades out on release; the input box scales in at the cursor |
| 3D view | hover / select | emissive tint fades in and out, outline fades in |
| 3D view | roof, cutaway, shadows | roof lifts and fades, cutaway height animates down from full height, shadow strength fades |
| 3D view | camera | presets and fit already ease; keep them interruptible |
| 3D view | model change | new meshes rise from the floor a few centimeters with opacity, removed meshes sink and fade |
| 3D view | walk and fly | entering and leaving blends the camera (`--dur-scene`); movement tracks the keys 1:1 with a short velocity ease; the minimap, crosshair and key hint fade in (`--dur-base`), the hint fades back after a few seconds |
| 3D view | solid, X-ray, hidden | each part of the building fades to its own opacity (`--dur-panel`), pipes stay solid, the shadow fades with the shell |
| 3D view | pipe layer on or off | the runs of that system fade out or in like any model change |
| 3D view | walking | glides (minimap click, double-click) ease over `--dur-scene`; door leaves swing or slide open and closed over `--dur-scene`; the walk settings popover opens like a flyout; the speed readout and the level label fade in and out; the minimap click leaves a ring that expands and fades; touch arrows press like buttons |
| 2D canvas | pipe tool | a placed point rings once, a finished run settles, point handles grow on hover and settle on drop, a refused drop eases back |
| 2D canvas | links | link arcs draw in along their curve and fade out (`--dur-base`); flipping a bow sweeps it to the other side; the flip handle grows on hover |
| 2D canvas | placement refused | the reason bumps once beside the cursor (under 100 ms, `--ease-spring`) |
| 2D canvas | fall and height tags | tags fade in and out as the layout drops or restores them |
| View mode | 2D / Split / 3D | panes resize with `--dur-panel`, the entering pane fades in |
| Copilot | messages, busy, proposal | messages rise and fade in, a three-dot busy indicator, the proposal card expands in, Apply morphs into a check mark, the canvas ghost breathes (opacity 0.55 to 0.85, 1.6 s) |
| Visuals | capture | a quick white flash over the 3D view (120 ms), the new card grows into the gallery |
