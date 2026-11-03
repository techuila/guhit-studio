# Keyboard shortcuts

The full set the app implements. `MOD` is Cmd on macOS, Ctrl on Windows. Tool
letters have no modifier and are ignored while typing in a text field, and
while a canvas drawing operation is in progress (so W mid wall-chain does not
restart the wall tool). Source of truth: `src/shell/shortcuts.tsx` (the
`TOOL_KEYS` map and the `useGlobalShortcuts` handler), `src/shell/actions.ts`
(`TOOLS`, `paletteActions`), and the in-app cheat sheet (press `?`).

While the 3D view walks or flies (`useViewer` nav is not `orbit`), it owns
every key without MOD: the global handler ignores them all, including tool
letters, Delete and `?`. MOD shortcuts keep working. Escape returns to orbit,
except while following someone in a live session, where it stops following.

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
| O | Objects (opens the object library flyout: furniture, lighting, electrical, aircon, utility; search finds names, form rows and keys such as "spo" or "1.5 hp") |
| P | Services: pipes, conduit and aircon lines. The rail button's flyout groups the eight systems by trade (Plumbing, Electrical, Aircon) with size and start height |
| L | Link: click a switch (or an outlet), then the lights (or the unit) it controls. The inspector's "Link more" starts it from the selected device |
| M | Dimension |
| T | Text |
| K | Camera |
| H | Pan (a hand tool; holding Space still pans temporarily) |

### While the services tool is active

With the pointer over the plan (H is the Pan tool anywhere else). Handled by
the plan canvas (`src/editor2d/controller.ts`).

| Key | Action |
|---|---|
| PageUp, PageDown | Raise or lower the pipe by 100 mm, Shift for 10 mm. Before the first click it sets the start height; during a run it adds a riser |
| H, then a number, Enter | Type the height |
| Enter | Finish the run, or apply a typed length |
| Escape, or Backspace | Step back: the pending riser, then the last point, then the whole run |

### Editing a selected run

One pipe, conduit or line set selected with the Select tool (DECISIONS D39).
Handled by the plan canvas (`src/editor2d/controller.ts`).

| Key or action | Action |
|---|---|
| Click the selected run | Pick the segment under the pointer. Click a point handle to pick that point |
| Escape | Step out: a picked segment or point back to the whole run, then clear the selection. Following someone, an open dialog or a focused field takes Escape first |
| PageUp, PageDown | With the pointer over the plan: move the picked part, or the whole run, up or down 100 mm, Shift for 10 mm. Joined runs follow |
| H, then a number, Enter | With the pointer over the plan: the picked part moves so its height reads the number (H is the Pan tool otherwise) |
| Click a height, fall or length tag | Type the value, Enter applies it, Escape cancels. Something that is not a number keeps the box open and says why; a value the engine refuses (a fall past 100%, a segment under 10 mm) shows its reason |
| Drag a point or the picked segment | Move it; joined runs follow. The picked segment's middle diamond moves it sideways only |
| A number, then Enter, while dragging | Move exactly that far along the drag (SketchUp's way); `600<90` also sets the direction |
| Alt while dragging | Break the joins: joined runs stay where they are |
| Click the + past an open end | Continue the run from there with the services tool; Enter finishes and returns to Select |

### While the link tool is active

Handled by the plan canvas. Click a switch (or an outlet), then each light
(or unit) it controls.

| Key | Action |
|---|---|
| Escape, or Enter | End linking |
| Tool letters | Still switch tools, as everywhere else |

## Live session

With the pointer on the plan, in a live session (DECISIONS D29). Handled by
the live session layer over the plan (`src/live/LiveLayer.tsx`).

| Key | Action |
|---|---|
| / | Cursor chat: a bubble opens at your pointer, in your color, and follows it. The others see what you type as you type it |
| Enter | Send the message (160 characters at most). It stays in the bubble a few seconds, and it is in the Chat tab with "Show on plan". An empty Enter closes the bubble |
| Escape | Close the bubble without sending. Clicking elsewhere closes it too |

"/" does nothing while a text field has focus, while a drawing, drag or
typed entry is in progress on the plan, while walking or flying in 3D, or
under a dialog. Outside a live session it shows a short hint at the pointer
instead.

In the Chat tab of the side dock, Enter sends and Shift+Enter starts a new
line. The palette has Start live session, Show the live session, Copy invite
(while hosting), Join a live session, Open chat, and End or Leave the live
session.

Following someone (DECISIONS D37: click their avatar in the top bar):

| Key | Action |
|---|---|
| Escape | Stop following. It comes first, before walk mode and the plan: a capture listener in `src/shell/shortcuts.tsx`. A dialog or a text field with the focus keeps its own Escape |

Any pan, zoom, fit, orbit, walk key or view mode key (1, 2, 3) of your own
also stops it.

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

## Sun and render

The live light is `useViewer().light`: view state, never saved in the
project and never an undo step (docs/CONTRACT.md, "Sun and light"). These
keys work from the plan or the 3D view; the status bar shows the new time for
a moment. Not while typing, not while walking or flying.

| Key | Action |
|---|---|
| U, I | Sun 15 minutes earlier or later, on the quarter hour. Holding the key repeats, so it scrubs |
| Shift+U, Shift+I | Previous or next preset: Morning 8:00 AM, Noon, Afternoon 3:00 PM, Dusk (sunset at the site plus 20 minutes, lamps on), Night 8:00 PM. Wraps around |
| Shift+N | Lamps on, or back to auto (on after sunset) |
| MOD+Alt+R | Render this view with the path tracer (works while walking, like every MOD key). Esc cancels the render |

WebView2 keeps its browser keys on (reload, DevTools, zoom), so none of these
use Ctrl+R, F5 or F12.

The palette also has Render this view, Render all saved views, Shadow study,
each sun preset, the sun 15 minutes earlier or later, the lamps, Show or hide
the sun path, Refine the 3D view when it rests (on or off) and Set the site for
the sun.

## Review list

With the review list focused (click an item, or Tab to it) in the project
inspector:

| Key | Action |
|---|---|
| Up, Down | Move between items, set-aside rows included. Home and End jump |
| S | Set the item aside: the note field takes focus. Enter sets it aside, Escape cancels |
| O | Reopen a set-aside row |
| Enter | Show it: select its objects and zoom to them |

MOD keys pass through (MOD+S still saves a version).

## Walk and X-ray (3D view)

| Key | Action |
|---|---|
| Shift+W | Walk through the building. From the plan-only view the view switches to split first |
| X | Building shell: solid, then X-ray, then hidden, then solid. Works from the plan view too; the status bar shows the mode while it is not solid |

### The pipe height handle (3D view)

A selected run, or the segment or point picked on it, shows a double arrow in
the 3D view while orbiting (DECISIONS D39, SketchUp's Move). Handled by
`src/viewer3d/engine/pipeHandle.ts`; while a move lasts it takes every key
without MOD, so a digit is a distance and not a view.

| Key or action | Action |
|---|---|
| Drag the arrow | Move the run, segment or point up or down, 1:1 with the pointer. Snaps: 10 mm steps, the level floor, the level height, the heights of runs nearby |
| Click the arrow | Pick it up: it follows the pointer until the next click puts it down |
| A number, then Enter | Move exactly that far, in the direction of the drag (a minus turns it round) |
| Up or Down arrow | Lock the move to the vertical (the default) |
| Right arrow | Lock the move to plan x; again goes back to vertical |
| Left arrow | Lock the move to plan y; again goes back to vertical |
| Alt | Break the joins: joined runs stay where they are |
| Escape | Cancel: the handle eases back and nothing changes |

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
X-ray the building, Hide the building, Show the building solid, Show the
pipe take-off, Show the device schedules, Show the review list, Add level
above (stacks a level on the top floor and works on it) and Delete this level
(asks inline in the inspector's Levels section, with the number of elements
that go with it; never for the last level). Clicking the level name in the
status bar opens the Levels section.

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
