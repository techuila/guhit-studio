// The Tours panel (lane L9): make a tour, play it, edit it as shots on a
// timeline, and export it as a video to Visuals. Mounted once by
// EditorShell; it opens from the top bar's Tours button, a tour's fields or
// the tours section. Playing is the `play_tour` bus event, which the 3D view
// answers (viewer3d/tour/player.ts). While open, the tour's camera path shows
// in 3D (viewer3d/tour/pathPreview.ts).
import { useEffect, useState } from "react";
import type { AutoTour, Ease, RenderSize, Shot, TransitionKind } from "../../contract/bindings";
import { useApp } from "../../state/store";
import { Button, IconButton, NumberField, Select, Switch, TextField } from "../../ui/controls";
import type { PresenceStage } from "../../ui/motion";
import { Presence } from "../../ui/motionDom";
import { tourFrames } from "../../viewer3d/tour/frames";
import { markTourPath, showTourPath } from "../../viewer3d/tour/pathPreview";
import { endScrub, stopTour } from "../../viewer3d/tour/player";
import { cancelTourExport, useTours } from "../../viewer3d/tour/tourStore";
import { Timeline } from "./Timeline";
import { MOVES, TRANSITIONS, clock, cleanSeconds, exportFromPanel, makeTour, playTour, saveTour, shotFromView, toursOf, tourSeconds, withShot, type TourEl } from "./tourActions";
import s from "./Tours.module.css";

export function ToursPanel() {
  const open = useTours((t) => t.open);
  const hasDoc = useApp((a) => a.doc !== null);
  return (
    <Presence open={open && hasDoc} exit="panel">
      {(stage) => <Panel stage={stage} />}
    </Presence>
  );
}

const SIZES: Array<{ value: RenderSize; label: string }> = [
  { value: "hd", label: "HD 1920 x 1080" },
  { value: "qhd", label: "QHD 2560 x 1440" },
  { value: "4k", label: "4K 3840 x 2160" },
];

function Panel({ stage }: { stage: PresenceStage }) {
  const doc = useApp((a) => a.doc);
  const tours = toursOf(doc?.project.elements ?? []);
  const tourId = useTours((t) => t.tourId);
  const tour = tours.find((t) => t.id === tourId) ?? tours[0] ?? null;
  const [selected, setSelected] = useState<number | null>(null);
  const [making, setMaking] = useState(false);

  // Keep the store on a tour that exists.
  useEffect(() => {
    if (tour && tour.id !== tourId) useTours.getState().select(tour.id);
  }, [tour, tourId]);
  useEffect(() => setSelected(null), [tour?.id]);

  // Escape closes the panel (a playing tour takes Escape first).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || useTours.getState().playing || e.defaultPrevented) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      useTours.getState().setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <section className={s.panel} data-stage={stage} aria-label="Tours" data-testid="tours-panel">
      <header className={s.head}>
        <h2 className={s.title}>Tours</h2>
        {tour && tours.length > 1 ? (
          <Select label="Tour" value={tour.id} options={tours.map((t) => ({ value: t.id, label: t.name }))} onChange={(id) => useTours.getState().select(id)} />
        ) : tour ? (
          <span className={s.rowName}>{tour.name}</span>
        ) : null}
        {tour ? <HeadTools tour={tour} /> : null}
        <IconButton icon="close" label="Close tours" tip="Close (Esc)" tipSide="top-end" size={15} onClick={() => useTours.getState().setOpen(false)} />
      </header>
      <div className={s.body} key={tour?.id ?? "none"}>
        {!tour || making ? (
          <MakeTour onDone={() => setMaking(false)} onCancel={tour ? () => setMaking(false) : undefined} />
        ) : (
          <>
            <PathSync tour={tour} />
            <Timeline tour={tour} selected={selected} onSelect={setSelected} />
            {selected !== null && tour.shots[selected] ? <ShotFields tour={tour} index={selected} onRemoved={() => setSelected(null)} /> : null}
            <footer className={s.foot}>
              <span>
                {tour.shots.length} shots, {clock(tourSeconds(tour), true)}, {tour.fps} frames a second. Click a shot to edit it; drag its right edge to change its length.
              </span>
              <Button
                size="sm"
                icon="plus"
                onClick={() => {
                  const next = shotFromView(tour, selected);
                  if (next) saveTour(tour, next);
                  else useApp.getState().toast("info", "Open the 3D view to add a shot from it.");
                }}
              >
                Shot from the view
              </Button>
              <Button size="sm" onClick={() => setMaking(true)}>
                New tour
              </Button>
            </footer>
          </>
        )}
      </div>
    </section>
  );
}

/** Keeps the 3D path preview on the open tour while the panel shows. */
function PathSync({ tour }: { tour: TourEl }) {
  const show = useTours((t) => t.showPath);
  const revision = useApp((a) => a.doc?.revision ?? 0);
  const playhead = useTours((t) => t.playhead);
  useEffect(() => {
    if (!show) {
      showTourPath(null);
      return;
    }
    let live = true;
    tourFrames(tour.id).then(
      (f) => {
        if (live) {
          showTourPath(f);
          markTourPath(useTours.getState().playhead);
        }
      },
      () => live && showTourPath(null),
    );
    return () => {
      live = false;
    };
  }, [tour.id, revision, show]);
  useEffect(() => {
    if (show) markTourPath(playhead);
  }, [playhead, show]);
  useEffect(
    () => () => {
      showTourPath(null);
      endScrub();
    },
    [tour.id],
  );
  return null;
}

function HeadTools({ tour }: { tour: TourEl }) {
  const playing = useTours((t) => t.playing);
  const playhead = useTours((t) => t.playhead);
  const showPath = useTours((t) => t.showPath);
  const exporting = useTours((t) => t.exporting);
  const [size, setSize] = useState<RenderSize>("hd");
  const total = tourSeconds(tour);
  return (
    <div className={s.headTools}>
      {playing ? (
        <Button size="sm" icon="close" onClick={() => stopTour()} data-testid="tour-stop">
          Stop
        </Button>
      ) : (
        <Button size="sm" variant="primary" icon="walk" onClick={() => playTour(tour.id)} disabled={!!exporting} data-testid="tour-play">
          Play
        </Button>
      )}
      <span className={s.clock} aria-live="off">
        {clock(playhead, true)} / {clock(total, true)}
      </span>
      <label className={s.pathToggle}>
        <Switch label="Show the path in 3D" checked={showPath} onChange={(on) => useTours.getState().setShowPath(on)} />
        <span>Path</span>
      </label>
      {exporting && exporting.tourId === tour.id ? (
        <span className={s.exportBox} data-testid="tour-exporting">
          <span className={s.progress} aria-hidden>
            <span className={s.progressFill} style={{ transform: `scaleX(${exporting.total ? exporting.done / exporting.total : 0})` }} />
          </span>
          <span className={s.progressText}>
            {exporting.phase} {exporting.total ? `${exporting.done} / ${exporting.total}` : ""}
            {exporting.codec ? `, ${exporting.codec === "avc" ? "H.264" : exporting.codec === "vp9" ? "VP9" : "PNG frames"}` : ""}
          </span>
          <Button size="sm" onClick={() => cancelTourExport()}>
            Stop
          </Button>
        </span>
      ) : (
        <span className={s.exportBox}>
          <Select label="Video size" value={size} options={SIZES} onChange={setSize} />
          <Button size="sm" icon="export" disabled={!!exporting || playing} onClick={() => void exportFromPanel(tour.id, size)} data-testid="tour-export">
            Export video
          </Button>
        </span>
      )}
    </div>
  );
}

function MakeTour({ onDone, onCancel }: { onDone: () => void; onCancel?: () => void }) {
  const [exterior, setExterior] = useState(true);
  const [dusk, setDusk] = useState(true);
  const [pace, setPace] = useState(1);
  const [busy, setBusy] = useState(false);
  const go = async () => {
    setBusy(true);
    const options: AutoTour = { exterior, day_to_dusk: dusk, pace_mps: pace, levels: [] };
    const id = await makeTour(options);
    setBusy(false);
    if (id) onDone();
  };
  return (
    <div className={s.make}>
      <p className={s.makeLead}>
        A tour the way property videos are shot: a drone fly-in and orbit, in through the front door, every room with a door and a hold in each, up the stairs, and a dusk finish outside. You can edit every shot after.
      </p>
      <label className={s.option}>
        <Switch label="Fly in and orbit first" checked={exterior} onChange={setExterior} />
        Fly in and orbit
      </label>
      <label className={s.option}>
        <Switch label="Day to dusk at the end" checked={dusk} onChange={setDusk} />
        Day to dusk
      </label>
      <label className={s.option}>
        Walking pace
        <span className={s.pace}>
          <NumberField label="Walking pace" kind="number" suffix="m/s" min={0.3} max={3} step={0.1} decimals={1} value={pace} onCommit={setPace} />
        </span>
      </label>
      <Button variant="primary" icon="plus" disabled={busy} onClick={() => void go()} data-testid="tour-make">
        {busy ? "Making the tour" : "Make tour"}
      </Button>
      {onCancel ? (
        <Button variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      ) : null}
    </div>
  );
}

const EASES: Array<{ value: Ease; label: string }> = [
  { value: "in_out", label: "Ease in and out" },
  { value: "linear", label: "Even" },
];

function ShotFields({ tour, index, onRemoved }: { tour: TourEl; index: number; onRemoved: () => void }) {
  const shot = tour.shots[index];
  const set = (patch: Partial<Shot>) => saveTour(tour, withShot(tour, index, patch));
  return (
    <div className={s.shotFields} key={shot.id} data-testid="tour-shot-fields">
      <label className={s.label}>
        Shot
        <TextField label="Shot name" value={shot.name} required onCommit={(name) => set({ name })} />
      </label>
      <label className={s.label}>
        Move
        <Select label="Camera move" value={shot.kind} options={MOVES} onChange={(kind) => set({ kind })} />
      </label>
      <label className={s.label}>
        Length
        <NumberField label="Shot length" kind="number" suffix="s" min={0.1} max={600} step={0.5} decimals={1} value={shot.seconds} onCommit={(v) => set({ seconds: cleanSeconds(v) })} />
      </label>
      <label className={s.label}>
        Easing
        <Select label="Easing" value={shot.ease} options={EASES} onChange={(ease) => set({ ease })} />
      </label>
      <div className={s.label}>
        Transition in
        <span className={s.transFields}>
          <Select<TransitionKind>
            label="Transition into the shot"
            value={shot.transition.kind}
            options={TRANSITIONS.map((t) => ({ value: t.value, label: t.label }))}
            onChange={(kind) => set({ transition: { kind, seconds: kind === "cut" ? 0 : shot.transition.seconds > 0 ? shot.transition.seconds : kind === "plan_lift" ? 2 : 0.8 } })}
          />
          {shot.transition.kind !== "cut" ? (
            <NumberField label="Transition length" kind="number" suffix="s" min={0.1} max={10} step={0.1} decimals={1} value={shot.transition.seconds} onCommit={(v) => set({ transition: { ...shot.transition, seconds: Math.min(10, Math.max(0.1, Math.round(v * 10) / 10)) } })} />
          ) : null}
        </span>
      </div>
      <IconButton
        icon="trash"
        label="Delete the shot"
        tip={tour.shots.length > 1 ? "Delete the shot" : "A tour keeps at least one shot"}
        disabled={tour.shots.length <= 1}
        onClick={() => {
          saveTour(tour, { ...tour, shots: tour.shots.filter((_, i) => i !== index) });
          onRemoved();
        }}
      />
    </div>
  );
}
