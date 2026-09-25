// The shadow study dialog (`bus.emit("shadow_study")`): pick the view, the
// dates and the step, then the live 3D view draws a frame per step and the
// frames land on a contact sheet, saved as a PNG and added to Visuals.

import { useMemo, useState } from "react";
import { create } from "zustand";
import { ipc, isTauri } from "../../contract/ipc";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { Button, Segmented, Select } from "../../ui/controls";
import { Dialog } from "../../ui/Dialog";
import { Presence } from "../../ui/motionDom";
import { useViewer } from "../viewerStore";
import { useLiveView } from "./liveView";
import { runShadowStudy, studyFrameCount, type StudyOptions } from "./shadowStudy";
import s from "./RenderSection.module.css";

interface StudyState {
  open: boolean;
  setOpen: (open: boolean) => void;
}

export const useShadowStudy = create<StudyState>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));

const DATES: Array<{ key: string; label: string; md: [number, number] | null }> = [
  { key: "today", label: "Today", md: null },
  { key: "mar", label: "Mar 20", md: [3, 20] },
  { key: "may", label: "May 15", md: [5, 15] },
  { key: "jun", label: "Jun 21", md: [6, 21] },
  { key: "sep", label: "Sep 23", md: [9, 23] },
  { key: "dec", label: "Dec 21", md: [12, 21] },
];

function today(): [number, number] {
  const d = new Date(Date.now() + 8 * 3600e3);
  return [d.getUTCMonth() + 1, d.getUTCDate()];
}

export function ShadowStudyDialog() {
  const open = useShadowStudy((st) => st.open);
  return (
    <Presence open={open} exit="panel">
      {(stage) => <StudyDialog stage={stage} />}
    </Presence>
  );
}

function StudyDialog({ stage }: { stage: "enter" | "idle" | "exit" }) {
  // The element list is a stable reference; the filtered one is made here.
  const elements = useApp((a) => a.doc?.project.elements);
  const views = useMemo(() => (elements ?? []).filter((e) => e.kind === "camera"), [elements]);
  const hasLive = useLiveView((l) => l.engine !== null);
  const [view, setView] = useState("current");
  const [picked, setPicked] = useState<string[]>(["jun", "dec"]);
  const [step, setStep] = useState<"60" | "30">("60");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(0);
  const [cancel, setCancel] = useState<{ v: boolean }>({ v: false });
  const [error, setError] = useState<string | null>(null);

  const dates = DATES.filter((d) => picked.includes(d.key)).map((d) => d.md ?? today());
  const opts: StudyOptions = { view, dates, fromMin: 6 * 60, toMin: 18 * 60, stepMin: step === "30" ? 30 : 60 };
  const total = studyFrameCount(opts);
  const close = () => {
    if (busy) cancel.v = true;
    useShadowStudy.getState().setOpen(false);
  };

  const run = async () => {
    const engine = useLiveView.getState().engine;
    const doc = useApp.getState().doc;
    if (!engine || !doc || dates.length === 0) return;
    const token = { v: false };
    setCancel(token);
    setBusy(true);
    setDone(0);
    setError(null);
    try {
      const result = await runShadowStudy(engine, doc, opts, (n) => setDone(n), () => token.v);
      if (!result) return;
      const name = `shadow-study-${doc.project.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "project"}`;
      let path: string | null = null;
      if (isTauri) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        path = await save({ defaultPath: `${name}.png`, filters: [{ name: "PNG image", extensions: ["png"] }] });
      }
      const app = useApp.getState();
      await ipc.renderCapture(result.camera, result.png, {
        revision: doc.revision,
        info: { kind: "shadow_study", width: result.width, height: result.height, samples: 0, seconds: 0, quality: null, gpu: "" },
      });
      useViewer.getState().bumpRenders();
      if (!isTauri || path) {
        const exported = await ipc.exportImage(result.png, name, path);
        app.toast("success", `Shadow study saved to ${exported.path} and added to Visuals`);
      } else {
        app.toast("success", "Shadow study added to Visuals");
      }
      useShadowStudy.getState().setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleDate = (key: string) => setPicked((p) => (p.includes(key) ? p.filter((k) => k !== key) : [...p, key]));

  return (
    <Dialog
      title="Shadow study"
      onClose={close}
      width={460}
      stage={stage}
      footer={
        <>
          <Button onClick={close}>{busy ? "Stop" : "Cancel"}</Button>
          <Button variant="primary" disabled={busy || !hasLive || dates.length === 0} onClick={() => void run()} data-testid="study-run" data-autofocus>
            {busy ? `Frame ${done} of ${total}` : "Make contact sheet"}
          </Button>
        </>
      }
    >
      <div className={s.study} data-testid="study-dialog">
        <p className={s.hint}>
          Frames of the 3D view from 6:00 AM to 6:00 PM on the dates you pick, with the time, date, place and a north arrow on each.
          The sheet is saved as a PNG and added to Visuals.
        </p>
        {!hasLive && <p className={s.error}>Open the 3D view first: the study draws its frames there.</p>}
        <div className={s.field}>
          <span className={s.fieldLabel}>View</span>
          <Select
            label="View"
            value={view}
            onChange={setView}
            options={[
              { value: "current", label: "The current 3D view" },
              { value: "top", label: "Top view, north up" },
              ...views.map((v) => ({ value: v.id, label: v.kind === "camera" ? v.name || "Saved view" : "View" })),
            ]}
          />
        </div>
        <div className={s.field}>
          <span className={s.fieldLabel}>Dates</span>
          <div className={s.actions} role="group" aria-label="Dates">
            {DATES.map((d) => (
              <button
                key={d.key}
                type="button"
                className={s.btn}
                aria-pressed={picked.includes(d.key)}
                data-on={picked.includes(d.key)}
                onClick={() => toggleDate(d.key)}
              >
                {d.label}
              </button>
            ))}
          </div>
        </div>
        <div className={s.field}>
          <span className={s.fieldLabel}>Step</span>
          <Segmented<"60" | "30">
            label="Step"
            value={step}
            options={[
              { value: "60", label: "Every hour" },
              { value: "30", label: "Every 30 minutes" },
            ]}
            onChange={setStep}
          />
        </div>
        <p className={s.hint}>
          {total} frames. The 3D view shows each one as it is drawn.
        </p>
        {busy && (
          <div className={s.bar} role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={done}>
            <span className={s.fill} style={{ width: `${Math.round((done / Math.max(total, 1)) * 100)}%` }} />
          </div>
        )}
        {error && (
          <p className={s.error} role="alert">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}

const offStudy = bus.on("shadow_study", () => {
  const app = useApp.getState();
  if (app.viewMode === "2d") app.setViewMode("split");
  useShadowStudy.getState().setOpen(true);
});
if (import.meta.hot) import.meta.hot.dispose(() => offStudy());
