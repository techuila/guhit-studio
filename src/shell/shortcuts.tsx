// Global keyboard shortcuts and the cheat sheet.
//
// Listeners are bubble-phase on window: the 2D editor's own listener runs in
// the capture phase and calls preventDefault/stopPropagation on anything it
// wants for itself - Escape, Enter, Backspace, typed digits, and (while a
// drawing operation is in progress) every other key too, so none of that
// ever reaches the handler below. Precedence is: a text input first (the
// isTyping check), then an in-progress canvas operation (already gone by
// the time we get here), then this global handler.
//
// While the 3D view walks or flies (useViewer nav is not "orbit") it owns
// every key without MOD: WASD, arrows, F and Escape. This handler then only
// answers MOD shortcuts.
//
// Sun and render keys (docs/CONTRACT.md, "Sun and light"): U and I move the
// sun 15 minutes and repeat while held, so holding one scrubs; Shift+U and
// Shift+I step the presets; Shift+N switches the lamps; MOD+Alt+R renders the
// view. None of them is a WebView2 browser key (Ctrl+R, F5 and F12 are).
import { useEffect, useRef } from "react";
import { ipc } from "../contract/ipc";
import { getActiveController } from "../editor2d/controller";
import { bus } from "../state/bus";
import { useApp, type Tool } from "../state/store";
import { Dialog, isModalOpen } from "../ui/Dialog";
import { useViewer } from "../viewer3d/viewerStore";
import type { PresenceStage } from "../ui/motion";
import {
  ALT,
  MOD,
  SHIFT,
  activateTool,
  deleteSelection,
  duplicateSelection,
  enterNav,
  escapeToSelect,
  openAssetTool,
  quickSaveVersion,
  renderViews,
  rotateSelectionCCW,
  selectAllOnLevel,
  stepSunPreset,
  stepSunTime,
  toggleLampsNow,
  zoomToSelection,
} from "./actions";
import { useShell } from "./shellStore";
import s from "./overlays.module.css";

const TOOL_KEYS: Record<string, Tool> = {
  v: "select",
  w: "wall",
  r: "rect_room",
  d: "door",
  n: "window",
  c: "column",
  s: "stair",
  o: "asset",
  m: "dimension",
  t: "text",
  k: "camera",
  h: "pan",
  p: "pipe",
  l: "link",
};

const ARROW_DELTA: Record<string, [number, number]> = {
  ArrowUp: [0, 1],
  ArrowDown: [0, -1],
  ArrowLeft: [-1, 0],
  ArrowRight: [1, 0],
};

/** How long a burst of held-arrow-key repeats waits before it is committed as one undo step. */
const NUDGE_SILENCE_MS = 250;

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || !el.tagName) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable;
}

interface NudgeBurst {
  ids: string[];
  dx: number;
  dy: number;
  timer: number | null;
}

export function useGlobalShortcuts() {
  // A held arrow key accumulates one delta and is committed as a single
  // `move_elements` command (one undo step) on keyup or after a short
  // silence, whichever comes first. Meanwhile a preview (doc_preview, no
  // commit) shows the accumulated move as a live ghost, reusing the same
  // ghost machinery the AI proposal preview draws with.
  const burst = useRef<NudgeBurst | null>(null);

  useEffect(() => {
    const commitBurst = () => {
      const b = burst.current;
      burst.current = null;
      if (!b) return;
      if (b.timer !== null) window.clearTimeout(b.timer);
      const app = useApp.getState();
      app.setPreview(null);
      if (b.dx === 0 && b.dy === 0) return;
      void app.dispatch({ type: "move_elements", ids: b.ids, delta: { x: b.dx, y: b.dy }, stretch_connected: true });
    };

    const nudge = (e: KeyboardEvent) => {
      const app = useApp.getState();
      if (app.selection.length === 0) return;
      const [ux, uy] = ARROW_DELTA[e.key];
      const grid = app.doc?.project.settings.grid_mm || 10;
      const step = e.altKey ? 1 : e.shiftKey ? grid * 10 : grid;

      let b = burst.current;
      if (b && b.ids.join(",") !== app.selection.join(",")) {
        // The selection changed mid burst: commit what was pending first.
        commitBurst();
        b = null;
      }
      if (!b) {
        b = { ids: [...app.selection], dx: 0, dy: 0, timer: null };
        burst.current = b;
      }
      b.dx += ux * step;
      b.dy += uy * step;
      if (b.timer !== null) window.clearTimeout(b.timer);
      b.timer = window.setTimeout(commitBurst, NUDGE_SILENCE_MS);

      void ipc
        .docPreview({ type: "move_elements", ids: b.ids, delta: { x: b.dx, y: b.dy }, stretch_connected: true })
        .then((result) => {
          if (burst.current === b) app.setPreview(result);
        })
        .catch(() => {});
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      const app = useApp.getState();
      const shell = useShell.getState();
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();

      // The palette opens from anywhere, even from a text field.
      if (mod && key === "k") {
        e.preventDefault();
        if (shell.overlay === "palette") shell.close();
        else if (!isModalOpen()) shell.open("palette");
        return;
      }

      if (isModalOpen() || isTyping(e.target)) return;

      if (mod) {
        // Alt changes e.key on macOS (Option+R is "®"), so match the physical key.
        if (e.altKey && e.code === "KeyR") {
          e.preventDefault();
          if (!e.repeat) void renderViews("current");
          return;
        }
        if (key === "z") {
          e.preventDefault();
          void (e.shiftKey ? app.redo() : app.undo());
        } else if (key === "y") {
          e.preventDefault();
          void app.redo();
        } else if (key === "e") {
          e.preventDefault();
          shell.open("export");
        } else if (key === "s") {
          e.preventDefault();
          if (e.shiftKey) shell.open("export");
          else void quickSaveVersion();
        } else if (key === "a") {
          e.preventDefault();
          selectAllOnLevel();
        } else if (key === "d") {
          e.preventDefault();
          duplicateSelection();
        } else if (key === "0") {
          e.preventDefault();
          bus.emit("zoom_to_fit");
        } else if (key === "=" || key === "+") {
          e.preventDefault();
          getActiveController()?.zoomStep(1);
        } else if (key === "-" || key === "_") {
          e.preventDefault();
          getActiveController()?.zoomStep(-1);
        } else if (key === ",") {
          e.preventDefault();
          shell.setDockTab("copilot");
        }
        return;
      }

      // Walking or flying: the 3D view owns every key without MOD, while it
      // is on screen. EditorShell ends the walk when the view goes plan-only.
      if (useViewer.getState().nav !== "orbit" && app.viewMode !== "2d") return;

      // Arrow keys nudge the selection. Repeats are allowed here (they
      // accumulate into the held burst); everything below this point cares
      // whether the key auto-repeated.
      if (ARROW_DELTA[e.key]) {
        if (app.selection.length > 0) {
          e.preventDefault();
          nudge(e);
        }
        return;
      }
      if (e.altKey) return;

      if (e.key === "Delete" || e.key === "Backspace") {
        if (app.selection.length > 0) {
          e.preventDefault();
          deleteSelection();
        }
        return;
      }
      if (e.key === "Escape") {
        escapeToSelect();
        return;
      }
      if (e.key === "?") {
        shell.open("shortcuts");
        return;
      }
      if (e.key === "F7") {
        e.preventDefault();
        app.toggle("gridVisible");
        return;
      }
      if (e.key === "F8") {
        e.preventDefault();
        app.toggle("orthoEnabled");
        return;
      }
      if (e.key === "F9") {
        e.preventDefault();
        app.toggle("snapEnabled");
        return;
      }

      // Shift+letter: a different, unmodified command from the plain key
      // below. Held shift never falls through to a tool shortcut.
      if (e.shiftKey) {
        if (key === "s") {
          e.preventDefault();
          app.toggle("snapEnabled");
        } else if (key === "o") {
          e.preventDefault();
          app.toggle("orthoEnabled");
        } else if (key === "r") {
          e.preventDefault();
          rotateSelectionCCW();
        } else if (key === "w") {
          e.preventDefault();
          void enterNav("walk");
        } else if (key === "u" || key === "i") {
          e.preventDefault();
          if (!e.repeat) stepSunPreset(key === "i" ? 1 : -1);
        } else if (key === "n") {
          e.preventDefault();
          if (!e.repeat) toggleLampsNow();
        }
        return;
      }

      // U and I repeat while held: holding one scrubs the sun.
      if (key === "u" || key === "i") {
        e.preventDefault();
        stepSunTime(key === "i" ? 1 : -1);
        return;
      }

      if (e.repeat) return;

      if (key === "1") app.setViewMode("2d");
      else if (key === "2") app.setViewMode("split");
      else if (key === "3") app.setViewMode("3d");
      else if (key === "f") bus.emit("zoom_to_fit");
      else if (key === "z") zoomToSelection();
      else if (key === "g") app.toggle("gridVisible");
      else if (key === "x") useViewer.getState().cycleShell();
      else if (key === "o") openAssetTool();
      else if (TOOL_KEYS[key]) activateTool(TOOL_KEYS[key]);
      else return;
      e.preventDefault();
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (ARROW_DELTA[e.key] && burst.current) commitBurst();
    };

    const onBlur = () => commitBurst();

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
      const b = burst.current;
      burst.current = null;
      if (b && b.timer !== null) window.clearTimeout(b.timer);
    };
  }, []);
}

const SHEET: Array<{ title: string; rows: Array<[string, string]> }> = [
  {
    title: "Draw",
    rows: [
      ["V", "Select"],
      ["W", "Wall"],
      ["R", "Room rectangle"],
      ["D", "Door"],
      ["N", "Window"],
      ["C", "Column"],
      ["S", "Stair"],
      ["O", "Objects"],
      ["P", "Services: pipes, conduit, aircon lines"],
      ["L", "Link a switch to its lights"],
      ["M", "Dimension"],
      ["T", "Text"],
      ["K", "Camera"],
      ["H", "Pan"],
    ],
  },
  {
    title: "Services tool, pointer on the plan",
    rows: [
      ["PgUp / PgDn", "Run height in 100 mm steps, Shift for 10"],
      ["H", "Type the height"],
      ["Enter", "Finish the run"],
    ],
  },
  {
    title: "Link tool, on the plan",
    rows: [["Esc / Enter", "End linking"]],
  },
  {
    title: "Live session, pointer on the plan",
    rows: [
      ["/", "Chat at your pointer"],
      ["Enter", "Send it"],
      ["Esc", "Close without sending"],
    ],
  },
  {
    title: "Toggles",
    rows: [
      ["G / F7", "Grid"],
      [`${SHIFT}S / F9`, "Snap"],
      [`${SHIFT}O / F8`, "Ortho lock"],
    ],
  },
  {
    title: "View",
    rows: [
      ["1", "Plan only"],
      ["2", "Plan and 3D"],
      ["3", "3D only"],
      [`F / ${MOD}0`, "Zoom to fit"],
      [`${MOD}=`, "Zoom in"],
      [`${MOD}-`, "Zoom out"],
      ["Z", "Zoom to selection"],
    ],
  },
  {
    title: "Walk and X-ray (3D view)",
    rows: [
      [`${SHIFT}W`, "Walk through the building"],
      ["W A S D", "Move while walking, or the arrows"],
      [MOD === "⌘" ? "⇧" : "Shift", "Run"],
      ["F", "Switch walk and fly"],
      ["E / Q", "Up and down while flying"],
      ["X", "Building solid, X-ray, hidden"],
      ["Esc", "Back to orbit"],
    ],
  },
  {
    title: "Sun and render",
    rows: [
      ["U / I", "Sun 15 minutes earlier or later. Hold to scrub"],
      [`${SHIFT}U / ${SHIFT}I`, "Previous or next sun preset"],
      [`${SHIFT}N`, "Lamps on, or auto"],
      [`${MOD}${ALT}R`, "Render this view"],
    ],
  },
  {
    title: "Review list, when it has focus",
    rows: [
      ["↑ ↓", "Move between items"],
      ["S", "Set aside, with a note"],
      ["O", "Reopen a set-aside item"],
      ["Enter", "Show it on the plan"],
    ],
  },
  {
    title: "Edit",
    rows: [
      [`${MOD}Z`, "Undo"],
      [`${SHIFT}${MOD}Z / ${MOD}Y`, "Redo"],
      ["Del", "Delete the selection"],
      ["Esc", "Back to Select, then clear selection"],
      ["Enter", "Apply a typed value"],
      [`${MOD}A`, "Select all on this level"],
      [`${MOD}D`, "Duplicate the selection"],
      [`${SHIFT}R`, "Rotate the selection 90°"],
      ["↑ ↓ ← →", "Nudge the selection by the grid step"],
      [`${SHIFT}↑ ↓ ← →`, "Nudge by 10x the grid step"],
      ["Alt+↑ ↓ ← →", "Nudge by 1 mm"],
    ],
  },
  {
    title: "Project",
    rows: [
      [`${MOD}K`, "Command palette"],
      [`${MOD}E`, "Export"],
      [`${MOD}S`, "Save a version"],
      [`${SHIFT}${MOD}S`, "Export to PDF"],
      ["?", "This list"],
      [`${MOD},`, "Copilot panel"],
    ],
  },
];

export function ShortcutsDialog({ onClose, stage }: { onClose: () => void; stage?: PresenceStage }) {
  return (
    <Dialog title="Keyboard shortcuts" onClose={onClose} width={800} stage={stage}>
      <div className={s.sheet}>
        {SHEET.map((group) => (
          <section key={group.title}>
            <h3>{group.title}</h3>
            <dl>
              {group.rows.map(([keys, label]) => (
                <div key={keys}>
                  <dt>
                    <kbd>{keys}</kbd>
                  </dt>
                  <dd>{label}</dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Dialog>
  );
}
