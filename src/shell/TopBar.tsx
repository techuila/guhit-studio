import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useLive } from "../live/liveStore";
import { LiveAvatars, ShareButton, useStepAuthor } from "../live/TopBarLive";
import { bus } from "../state/bus";
import { useApp, type ViewMode } from "../state/store";
import { Menu } from "../ui/Dialog";
import { Button, IconButton, Segmented, Spinner, TextField, cx } from "../ui/controls";
import { BrandMark, Icon } from "../ui/icons";
import { Presence } from "../ui/motionDom";
import { MOD, SHIFT, leaveEditor } from "./actions";
import { useProjectName, useShell } from "./shellStore";
import s from "./chrome.module.css";

const VIEW_OPTIONS: Array<{ value: ViewMode; label: string; icon: "view2d" | "split" | "view3d"; tip: string }> = [
  { value: "2d", label: "2D", icon: "view2d", tip: "Plan only (1)" },
  { value: "split", label: "Split", icon: "split", tip: "Plan and 3D side by side (2)" },
  { value: "3d", label: "3D", icon: "view3d", tip: "3D only (3)" },
];

function SaveStatus() {
  const saving = useApp((st) => st.saving) > 0;
  return (
    <span className={s.saved} data-tip={saving ? "Saving to this computer" : "Every change is saved to this computer as you work"} data-tip-side="bottom">
      <span className={s.savedIcon}>
        <Spinner size={13} className={cx(s.savedIconLayer, saving && s.savedIconOn)} />
        <Icon name="check" size={13} className={cx(s.savedIconLayer, !saving && s.savedIconOn)} />
      </span>
      {saving ? "Saving" : "Saved"}
    </span>
  );
}

function ImportMenu() {
  const [open, setOpen] = useState(false);
  const requestImport = useShell((st) => st.requestImport);
  return (
    <span className={s.importAnchor}>
      <Button variant="chrome" icon="import" className={s.foldable} data-tip="Import" onClick={() => setOpen((v) => !v)}>
        Import
      </Button>
      <Presence open={open} exit="hover">
        {(stage) => (
          <Menu
            stage={stage}
            onClose={() => setOpen(false)}
            align="end"
            items={[
              { key: "cad", label: "DXF or DWG", icon: "layers", onSelect: () => requestImport("cad") },
              { key: "model", label: "3D model (glTF, GLB, OBJ)", icon: "model", onSelect: () => requestImport("model") },
              { key: "bundle", label: "Open .guhit bundle", icon: "folder", onSelect: () => requestImport("bundle") },
            ]}
          />
        )}
      </Presence>
    </span>
  );
}

/**
 * Folds the right group's words away one step at a time, only when they do
 * not fit (chrome.module.css, data-fold): first the palette button, then
 * Versions and Import. Measured again on resize and when people come or go.
 */
function useFoldToFit(ref: RefObject<HTMLDivElement | null>, key: string): void {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      for (const level of ["0", "1", "2"]) {
        el.dataset.fold = level;
        const first = el.firstElementChild;
        if (!first || first.getBoundingClientRect().left >= el.getBoundingClientRect().left - 0.5) return;
      }
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    // Web fonts change the words' widths without resizing the group.
    let alive = true;
    void document.fonts?.ready.then(() => alive && fit());
    return () => {
      alive = false;
      ro.disconnect();
    };
  }, [ref, key]);
}

export function TopBar() {
  const name = useProjectName();
  const canUndo = useApp((st) => st.doc?.can_undo ?? false);
  const canRedo = useApp((st) => st.doc?.can_redo ?? false);
  const undoLabel = useApp((st) => st.doc?.undo_label ?? null);
  const redoLabel = useApp((st) => st.doc?.redo_label ?? null);
  // In a live session, someone else's step names them: "Undo Move wall, by Ana".
  const undoBy = useStepAuthor("undo");
  const redoBy = useStepAuthor("redo");
  const undo = useApp((st) => st.undo);
  const redo = useApp((st) => st.redo);
  const viewMode = useApp((st) => st.viewMode);
  const setViewMode = useApp((st) => st.setViewMode);
  const open = useShell((st) => st.open);
  const rightRef = useRef<HTMLDivElement>(null);
  const liveKey = useLive((st) => `${st.status.mode}:${st.status.participants.length}`);
  useFoldToFit(rightRef, liveKey);

  return (
    <header className={s.topbar}>
      <div className={s.topLeft}>
        <button type="button" className={s.backButton} onClick={() => void leaveEditor()} data-tip="Back to all projects" data-tip-side="bottom-start" aria-label="Back to all projects">
          <Icon name="back" size={16} />
          <BrandMark size={20} />
        </button>
        <span className={s.topRule} />
        <div className={s.projectName}>
          <TextField value={name} label="Project name" required className={s.projectNameInput} onCommit={(v) => void useApp.getState().renameProject(v)} />
        </div>
        <SaveStatus />
      </div>

      <div className={s.topCenter}>
        <div className={s.history}>
          <IconButton
            icon="undo"
            tone="chrome"
            label="Undo"
            className={s.historyButton}
            tip={canUndo ? `Undo ${undoLabel ?? ""}${undoBy ? `, by ${undoBy}` : ""} (${MOD}Z)`.replace("  ", " ") : "Nothing to undo"}
            disabled={!canUndo}
            onClick={() => void undo()}
          />
          <IconButton
            icon="redo"
            tone="chrome"
            label="Redo"
            className={s.historyButton}
            tip={canRedo ? `Redo ${redoLabel ?? ""}${redoBy ? `, by ${redoBy}` : ""} (${SHIFT}${MOD}Z)`.replace("  ", " ") : "Nothing to redo"}
            disabled={!canRedo}
            onClick={() => void redo()}
          />
        </div>
        <Segmented label="View mode" tone="chrome" options={VIEW_OPTIONS} value={viewMode} onChange={setViewMode} />
        <IconButton icon="fit" tone="chrome" label="Zoom to fit" tip="Zoom to fit (F)" onClick={() => bus.emit("zoom_to_fit")} />
      </div>

      <div className={s.topRight} ref={rightRef}>
        <button type="button" className={s.paletteButton} onClick={() => open("palette")} aria-label="Open the command palette" data-tip={`Search commands (${MOD}K)`}>
          <Icon name="search" size={14} />
          <span>Search commands</span>
          <kbd>{MOD}K</kbd>
        </button>
        <Button variant="chrome" icon="versions" className={s.foldable} data-tip="Versions" onClick={() => open("versions")}>
          Versions
        </Button>
        <ImportMenu />
        <IconButton icon="settings" tone="chrome" label="Settings" tip="Settings" onClick={() => open("settings")} />
        <LiveAvatars />
        <ShareButton />
        <Button variant="primary" size="md" icon="export" className={s.exportButton} onClick={() => open("export")}>
          Export
        </Button>
      </div>
    </header>
  );
}
