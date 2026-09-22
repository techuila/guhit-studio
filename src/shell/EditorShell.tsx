// Editor frame: top bar, tool rail, canvas area (2D, split, 3D), inspector,
// side dock (Copilot, Visuals), status bar, plus the overlays.
import { Suspense, lazy, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { ipc, onDocChanged } from "../contract/ipc";
import { bus } from "../state/bus";
import { useApp } from "../state/store";
import { Spinner, cx } from "../ui/controls";
import { PanelBoundary } from "../ui/feedback";
import { Icon } from "../ui/icons";
import { Presence, useLastTruthy, useSlidingIndicator } from "../ui/motionDom";
import { CommandPalette } from "./CommandPalette";
import { ExportDialog } from "./ExportDialog";
import { Inspector } from "./Inspector";
import { SettingsDialog } from "./Settings";
import { ShortcutsDialog, useGlobalShortcuts } from "./shortcuts";
import { StatusBar } from "./StatusBar";
import { ToolRail } from "./ToolRail";
import { TopBar } from "./TopBar";
import { VersionsPanel } from "./VersionsPanel";
import { saveThumbnail } from "./actions";
import { useShell, type DockTab } from "./shellStore";
import s from "./EditorShell.module.css";

// The four big panels load on their own. A panel that fails to load or crashes
// lands in its PanelBoundary instead of blanking the app, and three.js stays
// out of the hub's bundle.
const PlanCanvas = lazy(() => import("../editor2d/PlanCanvas").then((m) => ({ default: m.PlanCanvas })));
const Viewer3D = lazy(() => import("../viewer3d/Viewer3D").then((m) => ({ default: m.Viewer3D })));
const RenderPanel = lazy(() => import("../viewer3d/RenderPanel").then((m) => ({ default: m.RenderPanel })));
const AiDock = lazy(() => import("../ai/AiDock").then((m) => ({ default: m.AiDock })));

function Panel({ name, probe, tone, children }: { name: string; probe: string; tone?: "light" | "dark"; children: ReactNode }) {
  return (
    <PanelBoundary name={name} tone={tone}>
      <Suspense
        fallback={
          <div className={s.panelLoading}>
            <Spinner />
          </div>
        }
      >
        <CrashProbe name={probe} />
        {children}
      </Suspense>
    </PanelBoundary>
  );
}

/** Dev only: `?crash=<panel>` throws inside that panel's boundary. */
function CrashProbe({ name }: { name: string }) {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).get("crash") === name) {
    throw new Error(`Dev crash probe in "${name}"`);
  }
  return null;
}

/** Tracks a pointer drag and reports the position as a 0..1 fraction of `container`. */
function useDragRatio(
  axis: "x" | "y",
  container: React.RefObject<HTMLElement | null>,
  onRatio: (ratio: number) => void,
) {
  const [dragging, setDragging] = useState(false);
  const onPointerDown = (e: ReactPointerEvent<HTMLElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
  };
  const onPointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    if (!dragging || !container.current) return;
    const r = container.current.getBoundingClientRect();
    onRatio(axis === "x" ? (e.clientX - r.left) / r.width : (e.clientY - r.top) / r.height);
  };
  const end = (e: ReactPointerEvent<HTMLElement>) => {
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    setDragging(false);
  };
  return { dragging, handlers: { onPointerDown, onPointerMove, onPointerUp: end, onPointerCancel: end } };
}

function CanvasArea() {
  const viewMode = useApp((st) => st.viewMode);
  const splitRatio = useShell((st) => st.splitRatio);
  const setSplitRatio = useShell((st) => st.setSplitRatio);
  const area = useRef<HTMLDivElement>(null);
  const drag = useDragRatio("x", area, setSplitRatio);

  const show2d = viewMode !== "3d";
  const show3d = viewMode !== "2d";
  const split = viewMode === "split";

  return (
    <div ref={area} className={cx(s.canvasArea, drag.dragging && s.canvasDragging)}>
      {show2d ? (
        <div
          className={cx(s.pane, drag.dragging && s.paneDragging)}
          style={{ flex: split ? `0 0 calc(${splitRatio * 100}% - 3px)` : "1 1 0" }}
        >
          <Panel name="The plan view" probe="plan">
            <PlanCanvas />
          </Panel>
        </div>
      ) : null}
      {split ? (
        <div
          className={cx(s.divider, drag.dragging && s.dividerActive)}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize plan and 3D views"
          aria-valuenow={Math.round(splitRatio * 100)}
          tabIndex={0}
          onDoubleClick={() => setSplitRatio(0.5)}
          onKeyDown={(e) => {
            if (e.key === "ArrowLeft") setSplitRatio(splitRatio - 0.02);
            else if (e.key === "ArrowRight") setSplitRatio(splitRatio + 0.02);
            else return;
            e.stopPropagation();
          }}
          {...drag.handlers}
        >
          <span className={s.dividerGrip} />
        </div>
      ) : null}
      {show3d ? (
        <div className={cx(s.pane, s.pane3d, drag.dragging && s.paneDragging)} style={{ flex: "1 1 0" }}>
          <Panel name="The 3D view" probe="view3d" tone="dark">
            <Viewer3D />
          </Panel>
        </div>
      ) : null}
      {/* Keeps the canvases from swallowing the pointer while a divider moves. */}
      {drag.dragging ? <div className={s.dragShield} /> : null}
    </div>
  );
}

const DOCK_TABS: Array<{ key: DockTab; label: string; icon: "copilot" | "visuals" }> = [
  { key: "copilot", label: "Copilot", icon: "copilot" },
  { key: "visuals", label: "Visuals", icon: "visuals" },
];

function RightColumn() {
  const column = useRef<HTMLDivElement>(null);
  const tab = useShell((st) => st.dockTab);
  const collapsed = useShell((st) => st.dockCollapsed);
  const ratio = useShell((st) => st.dockRatio);
  const setDockTab = useShell((st) => st.setDockTab);
  const setCollapsed = useShell((st) => st.setDockCollapsed);
  const setDockRatio = useShell((st) => st.setDockRatio);
  const drag = useDragRatio("y", column, (r) => setDockRatio(1 - r));

  const tabsRef = useRef<HTMLDivElement>(null);
  const activeTabRef = useRef<HTMLButtonElement>(null);
  const tabIndicator = useSlidingIndicator(tabsRef, activeTabRef, [tab, collapsed]);

  return (
    <aside ref={column} className={s.right}>
      <div className={s.inspectorWrap}>
        <PanelBoundary name="The inspector">
          <CrashProbe name="inspector" />
          <Inspector />
        </PanelBoundary>
      </div>

      <section
        className={cx(s.dock, collapsed && s.dockCollapsed, drag.dragging && s.dockDragging)}
        style={collapsed ? undefined : { flexBasis: `${ratio * 100}%` }}
        aria-label="Side dock"
      >
        {collapsed ? null : (
          <div
            className={cx(s.dockResize, drag.dragging && s.dividerActive)}
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize the dock"
            {...drag.handlers}
          />
        )}
        <header className={s.dockHead}>
          <div ref={tabsRef} className={s.dockTabs} role="tablist" aria-label="Dock panels">
            {DOCK_TABS.map((t) => (
              <button
                key={t.key}
                ref={tab === t.key ? activeTabRef : undefined}
                type="button"
                role="tab"
                aria-selected={!collapsed && tab === t.key}
                className={cx(s.dockTab, !collapsed && tab === t.key && s.dockTabOn, t.key === "copilot" && s.dockTabAi)}
                onClick={() => setDockTab(t.key)}
              >
                <Icon name={t.icon} size={15} />
                <span>{t.label}</span>
              </button>
            ))}
            {!collapsed && tabIndicator.visible ? (
              <span
                aria-hidden
                className={cx(s.dockTabIndicator, tabIndicator.instant && s.instant, tab === "copilot" && s.dockTabIndicatorAi)}
                style={tabIndicator.style}
              />
            ) : null}
          </div>
          <button
            type="button"
            className={s.dockCollapse}
            aria-label={collapsed ? "Expand the dock" : "Collapse the dock"}
            aria-expanded={!collapsed}
            data-tip={collapsed ? "Expand" : "Collapse"}
            data-tip-side="top-end"
            onClick={() => setCollapsed(!collapsed)}
          >
            <Icon name={collapsed ? "chevronUp" : "chevronDown"} size={14} />
          </button>
        </header>
        {/* Both stay mounted so a chat in progress survives a tab switch. */}
        <div className={s.dockBody} hidden={collapsed}>
          <div className={s.dockPanel} role="tabpanel" hidden={tab !== "copilot"}>
            <Panel name="The copilot" probe="copilot">
              <AiDock />
            </Panel>
          </div>
          <div className={s.dockPanel} role="tabpanel" hidden={tab !== "visuals"}>
            <Panel name="Visuals" probe="visuals">
              <RenderPanel />
            </Panel>
          </div>
        </div>
      </section>
      {drag.dragging ? <div className={s.dragShield} /> : null}
    </aside>
  );
}

export function EditorShell() {
  const overlay = useShell((st) => st.overlay);
  const lastOverlay = useLastTruthy(overlay);
  const open = useShell((st) => st.open);
  const close = useShell((st) => st.close);
  const hasDoc = useApp((st) => st.doc !== null);

  useGlobalShortcuts();

  useEffect(() => bus.on("open_palette", () => open("palette")), [open]);

  // Hub thumbnail: refreshed at most once a minute while editing, and on leave (see leaveEditor).
  useEffect(() => {
    const t = window.setInterval(() => void saveThumbnail({ minIntervalMs: 60_000 }), 60_000);
    return () => window.clearInterval(t);
  }, []);

  // Changes made by something other than this window: the MCP server driving
  // the app from Claude Code, or another client. Refetch the document; a null
  // one means the project was closed, so go back to the hub. See docs/MCP.md.
  useEffect(
    () =>
      onDocChanged((revision) => {
        if (useApp.getState().doc?.revision === revision) return; // our own edit
        void ipc
          .docState()
          .then((doc) => {
            useApp.getState().setDoc(doc);
            if (!doc) useApp.setState({ screen: "hub" });
          })
          .catch(() => {
            // The backend is unreachable; the next change tries again.
          });
      }),
    [],
  );

  if (!hasDoc) return null;

  return (
    <div className={s.shell}>
      <TopBar />
      <div className={s.middle}>
        <ToolRail />
        <CanvasArea />
        <RightColumn />
      </div>
      <StatusBar />

      <Presence open={overlay !== null} exit={lastOverlay === "palette" ? "base" : "panel"}>
        {(stage) => {
          switch (lastOverlay) {
            case "palette":
              return <CommandPalette onClose={close} stage={stage} />;
            case "export":
              return <ExportDialog onClose={close} stage={stage} />;
            case "versions":
              return <VersionsPanel onClose={close} stage={stage} />;
            case "shortcuts":
              return <ShortcutsDialog onClose={close} stage={stage} />;
            case "settings":
              return <SettingsDialog onClose={close} stage={stage} />;
            default:
              return null;
          }
        }}
      </Presence>
    </div>
  );
}
