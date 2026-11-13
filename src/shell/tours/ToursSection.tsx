// The project inspector's tours section (lane L9): the project's tours and
// the videos exported from each, with play and make. Shows nothing on a
// project without tours or walls.
import { useEffect, useState } from "react";
import type { DocState, RenderRecord } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { Button, IconButton, Section } from "../../ui/controls";
import { useViewer } from "../../viewer3d/viewerStore";
import { useTours } from "../../viewer3d/tour/tourStore";
import { useSection } from "../shellStore";
import t from "./Tours.module.css";
import { clock, playTour, toursOf, tourSeconds } from "./tourActions";

const CODEC: Record<string, string> = { avc: "H.264", vp9: "VP9", png: "PNG frames" };

/** Video records of the open project, newest first; refreshed when Visuals changes. */
function useVideos(projectId: string): RenderRecord[] {
  const bump = useViewer((v) => v.rendersVersion);
  const [list, setList] = useState<RenderRecord[]>([]);
  useEffect(() => {
    let live = true;
    ipc.renderList().then(
      (all) => live && setList(all.filter((r) => r.video)),
      () => live && setList([]),
    );
    return () => {
      live = false;
    };
  }, [projectId, bump]);
  return list;
}

export function ToursSection({ doc }: { doc: DocState }) {
  const tours = toursOf(doc.project.elements);
  const [open, toggle] = useSection("tours", tours.length > 0);
  const videos = useVideos(doc.project.id);
  const hasWalls = doc.project.elements.some((e) => e.kind === "wall");
  if (!hasWalls && tours.length === 0) return null;
  const panel = (id: string | null) => {
    if (id) useTours.getState().select(id);
    useTours.getState().setOpen(true);
  };
  return (
    <Section title="Tours" icon="tours" count={tours.length} open={open} onToggle={toggle}>
      {tours.length === 0 ? (
        <div className={t.actions}>
          <Button size="sm" icon="plus" onClick={() => panel(null)}>
            Make a tour
          </Button>
        </div>
      ) : (
        <ul className={t.list}>
          {tours.map((tour) => {
            const mine = videos.filter((v) => v.video?.tour_id === tour.id);
            return (
              <li key={tour.id} className={t.row}>
                <span className={t.rowText}>
                  <span className={t.rowName}>{tour.name}</span>
                  <span className={t.rowMeta}>
                    {clock(tourSeconds(tour), true)}, {tour.shots.length} shots
                  </span>
                  {mine.slice(0, 3).map((v) => (
                    <span key={v.id} className={t.rowMeta} title={v.video?.path}>
                      <span className={t.videoPill}>Video</span> {v.video?.width} x {v.video?.height}, {CODEC[v.video?.codec ?? ""] ?? v.video?.codec}, {v.video?.seconds.toFixed(1)} s
                    </span>
                  ))}
                </span>
                <IconButton icon="walk" label={`Play ${tour.name}`} size={15} onClick={() => playTour(tour.id)} />
                <IconButton icon="pencil" label={`Edit ${tour.name}`} size={15} onClick={() => panel(tour.id)} />
              </li>
            );
          })}
        </ul>
      )}
    </Section>
  );
}
