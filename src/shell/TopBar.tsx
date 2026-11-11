import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useAgents } from "../live/agentsStore";
import { useLive } from "../live/liveStore";
import { ChatButton } from "../live/ChatButton";
import { LiveAvatars, ShareButton, useStepAuthor } from "../live/TopBarLive";
import { bus } from "../state/bus";
import { useApp, type ViewMode } from "../state/store";
import { Menu } from "../ui/Dialog";
import { Button, IconButton, Segmented, Spinner, TextField, cx } from "../ui/controls";
import { BrandMark, Icon } from "../ui/icons";
import { Presence } from "../ui/motionDom";
import { ConnectButton } from "./connect/ConnectButton";
import { MOD, SHIFT, leaveEditor } from "./actions";
import { HistoryButton } from "./HistoryPanel";
import { SheetsButton } from "./sheets/SheetsButton";
import { useProjectName, useShell } from "./shellStore";
import { ToursButton } from "./tours/ToursButton";
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
      <span className={s.savedWord}>{saving ? "Saving" : "Saved"}</span>
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
              { key: "bundle", label: "Open .guhit file", icon: "folder", onSelect: () => requestImport("bundle") },
            ]}
          />
        )}
      </Presence>
    </span>
  );
}

/**
 * The steps the bar takes when it runs out of room, in order (chrome.module.css).
 * `fold` is on the right group: 1 the palette button's words, 2 also Versions
 * and Import, 3 also Connect agent and Share. `squeeze` is on the bar: 1 the
 * view mode shows its icons only, 2 the right group takes the width it needs
 * and the middle group moves left of centre, 3 tighter gaps and "Saved" keeps
 * only its mark. Every folded word stays its button's accessible name.
 */
export const FOLD_STEPS: ReadonlyArray<{ fold: number; squeeze: number }> = [
  { fold: 0, squeeze: 0 },
  { fold: 1, squeeze: 0 },
  { fold: 2, squeeze: 0 },
  { fold: 3, squeeze: 0 },
  { fold: 3, squeeze: 1 },
  { fold: 3, squeeze: 2 },
  { fold: 3, squeeze: 3 },
];

/**
 * When no step fits, the bar stays here: the right group overflows its own
 * column, which is how the live agents (TopBarLive `useCompact`) see that
 * they must fold into the Agents button. Then the bar measures again.
 */
const LAST_RESORT = 4;

/** The least room kept between the left group's words and the middle group. */
const MIN_GAP = 8;

/** Where the bar's groups end, in px from the window's left. */
export interface BarEdges {
  /** Right edge of the left group's last item. */
  leftEnd: number;
  centerStart: number;
  centerEnd: number;
  /** Left edge of the right group's first item, and of the group's own box. */
  rightStart: number;
  rightBox: number;
  /** Right edge of the right group's last item, and of the bar's content box. */
  rightEnd: number;
  barEnd: number;
}

/** True when nothing in the bar overlaps or spills out: the right group fits its column and the groups keep apart. */
export function barFits(b: BarEdges): boolean {
  const e = 0.5;
  return b.rightStart >= b.rightBox - e && b.leftEnd + MIN_GAP <= b.centerStart + e && b.centerEnd <= b.rightStart + e && b.rightEnd <= b.barEnd + e;
}

function edges(bar: HTMLElement, left: HTMLElement, center: HTMLElement, right: HTMLElement): BarEdges {
  const box = (el: Element | null) => (el ?? right).getBoundingClientRect();
  const barBox = bar.getBoundingClientRect();
  const pad = parseFloat(getComputedStyle(bar).paddingRight) || 0;
  const c = center.getBoundingClientRect();
  return {
    leftEnd: box(left.lastElementChild).right,
    centerStart: c.left,
    centerEnd: c.right,
    rightStart: box(right.firstElementChild).left,
    rightBox: right.getBoundingClientRect().left,
    rightEnd: box(right.lastElementChild).right,
    barEnd: barBox.right - pad,
  };
}

/**
 * Takes the steps above one at a time, only as far as needed, so nothing in
 * the bar overlaps. Measured again on resize, when web fonts load and when
 * people or agents come or go.
 */
function useFoldToFit(
  refs: { bar: RefObject<HTMLElement | null>; left: RefObject<HTMLDivElement | null>; center: RefObject<HTMLDivElement | null>; right: RefObject<HTMLDivElement | null> },
  key: string,
): void {
  useLayoutEffect(() => {
    const bar = refs.bar.current;
    const left = refs.left.current;
    const center = refs.center.current;
    const right = refs.right.current;
    if (!bar || !left || !center || !right) return;
    const apply = (i: number) => {
      right.dataset.fold = String(FOLD_STEPS[i].fold);
      bar.dataset.squeeze = String(FOLD_STEPS[i].squeeze);
    };
    const fit = () => {
      for (let i = 0; i < FOLD_STEPS.length; i++) {
        apply(i);
        if (barFits(edges(bar, left, center, right))) return;
      }
      apply(LAST_RESORT);
    };
    fit();
    // The bar for the window's width, the right group for what comes and goes in it.
    const ro = new ResizeObserver(fit);
    ro.observe(bar);
    ro.observe(right);
    // Web fonts change the words' widths without resizing the group.
    let alive = true;
    void document.fonts?.ready.then(() => alive && fit());
    return () => {
      alive = false;
      ro.disconnect();
    };
  }, [refs.bar, refs.left, refs.center, refs.right, key]);
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
  const barRef = useRef<HTMLElement>(null);
  const leftRef = useRef<HTMLDivElement>(null);
  const centerRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const liveKey = useLive((st) => `${st.status.mode}:${st.status.participants.length}:${st.unread > 0}`);
  // Agents take room in the bar too (DECISIONS D47): measure again when they come or go.
  const agentCount = useAgents((st) => st.views.length);
  const [refs] = useState(() => ({ bar: barRef, left: leftRef, center: centerRef, right: rightRef }));
  useFoldToFit(refs, `${liveKey}:${agentCount}`);

  return (
    <header className={s.topbar} ref={barRef}>
      <div className={s.topLeft} ref={leftRef}>
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

      <div className={s.topCenter} ref={centerRef}>
        <div className={s.history}>
          <IconButton
            icon="undo"
            tone="chrome"
            label="Undo"
            tip={canUndo ? `Undo ${undoLabel ?? ""}${undoBy ? `, by ${undoBy}` : ""} (${MOD}Z)`.replace("  ", " ") : "Nothing to undo"}
            disabled={!canUndo}
            onClick={() => void undo()}
          />
          <IconButton
            icon="redo"
            tone="chrome"
            label="Redo"
            tip={canRedo ? `Redo ${redoLabel ?? ""}${redoBy ? `, by ${redoBy}` : ""} (${SHIFT}${MOD}Z)`.replace("  ", " ") : "Nothing to redo"}
            disabled={!canRedo}
            onClick={() => void redo()}
          />
          <HistoryButton />
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
        <IconButton icon="settings" tone="chrome" label="Settings" tip={`Settings (${MOD},)`} onClick={() => open("settings")} />
        <span className={s.topRule} aria-hidden />
        <LiveAvatars />
        <ChatButton />
        <ConnectButton variant="chrome" tipSide="bottom" />
        <ShareButton />
        <span className={s.topRule} aria-hidden />
        <ToursButton />
        <SheetsButton />
        <Button variant="primary" size="md" icon="export" className={s.exportButton} onClick={() => open("export")}>
          Export
        </Button>
      </div>
    </header>
  );
}
