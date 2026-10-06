import { useEffect, useState } from "react";
import { ProjectHub } from "./hub/ProjectHub";
import { EditorShell } from "./shell/EditorShell";
import { ImportController } from "./shell/Import";
import { useApp } from "./state/store";
import { BusyOverlay, PanelBoundary, Toasts } from "./ui/feedback";
import { dur } from "./ui/motion";
import { ipc, onDocChanged } from "./contract/ipc";
import { startWindowTasks } from "./shell/windowTasks";
import { LiveRoot } from "./live/LiveRoot";

function isTextTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
}

export default function App() {
  const screen = useApp((s) => s.screen);
  const loadCatalog = useApp((s) => s.loadCatalog);

  useEffect(() => {
    void loadCatalog();
  }, [loadCatalog]);

  // Answers MCP render and capture requests (DECISIONS D31).
  useEffect(() => startWindowTasks(), []);

  // A project opened or created from outside this window (Claude Code through
  // guhit-mcp) must bring the editor up. The editor handles later changes itself.
  useEffect(() => {
    if (screen !== "hub") return;
    return onDocChanged(async () => {
      try {
        const state = await ipc.docState();
        if (state && useApp.getState().screen === "hub") {
          useApp.getState().setDoc(state);
          useApp.setState({ screen: "editor", tool: "select", selection: [], preview: null });
        }
      } catch {
        // bridge or backend unavailable: nothing to switch to
      }
    });
  }, [screen]);

  // The screen on display lags the store by one short fade, so the leaving
  // screen fades out before the entering one fades in (docs/MOTION.md rule 6).
  const [shown, setShown] = useState(screen);
  const leaving = shown !== screen;
  useEffect(() => {
    if (shown === screen) return;
    const id = window.setTimeout(() => setShown(screen), dur("hover"));
    return () => window.clearTimeout(id);
  }, [screen, shown]);

  // Desktop feel: no browser context menu on chrome, no accidental file drops navigating away.
  useEffect(() => {
    const onContext = (e: MouseEvent) => {
      if (!isTextTarget(e.target)) e.preventDefault();
    };
    const onDrop = (e: DragEvent) => {
      if (!e.defaultPrevented) e.preventDefault();
    };
    window.addEventListener("contextmenu", onContext);
    window.addEventListener("dragover", onDrop);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("contextmenu", onContext);
      window.removeEventListener("dragover", onDrop);
      window.removeEventListener("drop", onDrop);
    };
  }, []);

  return (
    <>
      <div
        style={{
          height: "100%",
          opacity: leaving ? 0 : 1,
          transition: "opacity var(--dur-hover) var(--ease-in)",
        }}
      >
        <PanelBoundary name={shown === "hub" ? "The project list" : "The editor"}>
          {shown === "hub" ? <ProjectHub /> : <EditorShell />}
        </PanelBoundary>
      </div>
      <Toasts />
      <BusyOverlay />
      <ImportController />
      {/* Live sessions: wiring, share and join dialogs, confirmations (DECISIONS D29). */}
      <LiveRoot />
    </>
  );
}
