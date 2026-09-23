# Keyboard shortcuts

The full set the app implements. `MOD` is Cmd on macOS, Ctrl on Windows. Tool
letters have no modifier and are ignored while typing in a text field, and
while a canvas drawing operation is in progress (so W mid wall-chain does not
restart the wall tool). Source of truth: `src/shell/shortcuts.tsx` (the
`TOOL_KEYS` map and the `useGlobalShortcuts` handler), `src/shell/actions.ts`
(`TOOLS`, `paletteActions`), and the in-app cheat sheet (press `?`).

While the 3D view walks or flies (`useViewer` nav is not `orbit`), it owns
every key without MOD: the global handler ignores them all, including tool
letters, Delete and `?`. MOD shortcuts keep working. Escape returns to orbit.

## Draw (tools)

| Key | Tool |
|---|---|
| V | Select |
| W | Wall |
| R | Room rectangle |
| D | Door |
| N | Window |
| C | Column |
| S | Stair |
| O | Objects (opens the object library flyout) |
| P | Pipe. System, size and start height are in the rail button's flyout |
| M | Dimension |
| T | Text |
| K | Camera |
| H | Pan (a hand tool; holding Space still pans temporarily) |

### While the pipe tool is active

With the pointer over the plan (H is the Pan tool anywhere else). Handled by
the plan canvas (`src/editor2d/controller.ts`).

| Key | Action |
|---|---|
| PageUp, PageDown | Raise or lower the pipe by 100 mm, Shift for 10 mm. Before the first click it sets the start height; during a run it adds a riser |
| H, then a number, Enter | Type the height |
| Enter | Finish the run, or apply a typed length |
| Escape, or Backspace | Step back: the pending riser, then the last point, then the whole run |

## Toggles

| Key | Action |
|---|---|
| G, or F7 | Toggle the grid |
| Shift+S, or F9 | Toggle snapping |
| Shift+O, or F8 | Toggle ortho lock |

## View

| Key | Action |
|---|---|
| 1 | Plan only |
| 2 | Plan and 3D, split |
| 3 | 3D only |
| F, or MOD+0 | Zoom to fit |
| MOD+= | Zoom in |
| MOD+- | Zoom out |
| Z | Zoom to the selection (zoom to fit when nothing is selected) |

## Walk and X-ray (3D view)

| Key | Action |
|---|---|
| Shift+W | Walk through the building. From the plan-only view the view switches to split first |
| X | Building shell: solid, then X-ray, then hidden, then solid. Works from the plan view too; the status bar shows the mode while it is not solid |

In walk or fly mode the 3D view handles the keys below
(`src/viewer3d/engine/walker.ts`). Drag in the 3D view to look around, or use
its Lock mouse button.

| Key | Action |
|---|---|
| W A S D, or arrow keys | Move forward, back and sideways |
| Shift | Run |
| F | Switch between walk and fly |
| E or Space, Q or C | Up and down, in fly mode |
| X | Building solid, X-ray, hidden |
| Escape | Back to orbit. With the mouse locked, the first Escape only frees the mouse |

Switching to the plan-only view (the 2D button, or the palette) ends a walk,
so the plan gets its keys back.

The palette also has Walk through the building, Fly through the building,
X-ray the building, Hide the building, Show the building solid, and Show the
pipe take-off.

## Edit

| Key | Action |
|---|---|
| MOD+Z | Undo |
| Shift+MOD+Z, or MOD+Y | Redo |
| Delete, or Backspace | Delete the selection |
| Escape | Tool back to Select, then clear the selection |
| MOD+A | Select all on the active level (visible, unlocked layers only) |
| MOD+D | Duplicate the selection, offset by one grid step. The copies become the selection |
| Shift+R | Rotate the selection 90 degrees counter-clockwise about its center. Openings turn with their wall, not on their own |
| Arrow keys | Nudge the selection by the project's grid step |
| Shift+Arrow | Nudge by 10x the grid step |
| Alt+Arrow | Nudge by 1 mm |

A held arrow key is one undo step: the move commits on keyup or after 250 ms
of no further repeats, whichever comes first. While it is held, the move is
shown as a live preview ghost (the same mechanism the AI proposal preview
uses) built from `doc_preview`, without committing anything.

## Project

| Key | Action |
|---|---|
| MOD+K | Command palette |
| MOD+E | Export |
| MOD+S | Save a quick version, labeled "Version N" |
| Shift+MOD+S | Open Export with PDF selected |
| ? | This list |
| MOD+, | Open the copilot panel |

MOD+, only switches the side dock to the copilot tab. The copilot's own
settings popover is local state inside `src/ai/AiDock.tsx` with no store hook
to open it from outside, so the shortcut cannot reach into it without
changing AI-owned code.
