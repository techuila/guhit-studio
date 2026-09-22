# Keyboard shortcuts

The full set the app implements. `MOD` is Cmd on macOS, Ctrl on Windows. Tool
letters have no modifier and are ignored while typing in a text field, and
while a canvas drawing operation is in progress (so W mid wall-chain does not
restart the wall tool). Source of truth: `src/shell/shortcuts.tsx` (the
`TOOL_KEYS` map and the `useGlobalShortcuts` handler), `src/shell/actions.ts`
(`TOOLS`, `paletteActions`), and the in-app cheat sheet (press `?`).

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
| M | Dimension |
| T | Text |
| K | Camera |
| H | Pan (a hand tool; holding Space still pans temporarily) |

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
