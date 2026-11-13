// Schedules in the project inspector. The permit schedules (lane L1), shown
// when the project has doors, windows or rooms: doors and windows by mark
// with counts per level, rooms with net area, ceiling and finishes, and
// each level's areas, all from `Derived`, with the project's totals. Then
// the device schedules, shown when the project has devices or light
// fixtures: device counts in the rows of the PH electrical inspection form
// (per level and room), the light fixture table and the aircon units.
// Counts come from `Derived::schedule` and the model; nothing here sizes,
// loads or rates anything (DECISIONS D21).
import { useMemo, useRef, useState } from "react";
import type { Command, DocState, Element } from "../contract/bindings";
import { gridSystemOnWalls } from "../editor2d/permit/gridSystem";
import { useApp } from "../state/store";
import { Button, Section, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { formatArea, formatLength } from "../ui/units";
import { hpLabel } from "./devices";
import { Bump } from "./fields/permitCommon";
import { buildPermitSchedules, permitSchedulesCsv } from "./permitSchedules";
import { CopyButton, useReveal } from "./PipeSections";
import { SCHEDULE_NOTE, buildSchedules, hasScheduledDevices, roomCountsLine, schedulesCsv } from "./schedules";
import { useSection } from "./shellStore";
import css from "./fields/permit.module.css";
import s from "./Inspector.module.css";

/** A warm to cool dot for a color temperature, 2700 K to 6500 K. */
function kelvinColor(k: number): string {
  const t = Math.min(1, Math.max(0, (k - 2700) / 3800));
  const mix = (a: number, b: number) => Math.round(a + (b - a) * t);
  return `rgb(${mix(255, 214)}, ${mix(196, 230)}, ${mix(120, 255)})`;
}

/** The project's schedules (lane L1). Nothing while there is nothing to schedule. */
export function SchedulesSection({ doc }: { doc: DocState }) {
  const catalog = useApp((st) => st.catalog);
  const scheduled = useMemo(() => hasScheduledDevices(doc, catalog), [doc, catalog]);
  const permit = useMemo(() => doc.project.elements.some((e) => e.kind === "opening" || e.kind === "room"), [doc]);
  return (
    <>
      <PermitDrawing doc={doc} />
      {permit ? <PermitSchedules doc={doc} /> : null}
      {scheduled ? <DeviceSchedules doc={doc} /> : null}
    </>
  );
}

/**
 * Drawing aids for the permit plans (lane L1): automatic exterior
 * dimensions of the level on screen (`AutoDimension`, they follow the model
 * after every change) and a grid system on its walls (`AddGridSystem`).
 * Nothing while the level has no walls.
 */
function PermitDrawing({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("permit_drawing", true);
  const active = useApp((st) => st.activeLevelId);
  const [busy, setBusy] = useState<string | null>(null);
  const levelId = doc.project.levels.some((l) => l.id === active) ? active : (doc.project.levels[0]?.id ?? null);
  const walls = useMemo(
    () => doc.project.elements.filter((e): e is Extract<Element, { kind: "wall" }> => e.kind === "wall" && e.level_id === levelId && e.role !== "site"),
    [doc, levelId],
  );
  if (!levelId || walls.length === 0) return null;
  const level = doc.project.levels.find((l) => l.id === levelId);
  const grid = gridSystemOnWalls(walls);
  const autos = doc.project.elements.filter((e) => e.kind === "dimension" && e.level_id === levelId && !!e.auto).length;
  const run = async (key: string, command: Command) => {
    setBusy(key);
    try {
      const result = await useApp.getState().dispatch(command);
      if (result?.diff.summary) useApp.getState().toast("success", result.diff.summary);
    } finally {
      setBusy(null);
    }
  };
  return (
    <Section
      title="Permit drawing"
      icon="dimension"
      open={open}
      onToggle={toggle}
      aside={autos > 0 ? <span className={s.sectionAside}>{autos} automatic</span> : null}
    >
      <p className={s.note}>
        Automatic dimensions follow the walls and openings of {level?.name ?? "this level"} after every change. Editing one by hand makes it a plain dimension.
      </p>
      <div className={s.actionsRow}>
        <Button size="sm" icon="dimension" disabled={busy !== null} onClick={() => void run("exterior", { type: "auto_dimension", level_id: levelId, kinds: ["exterior"] })}>
          Dimension this level
        </Button>
        <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => void run("rooms", { type: "auto_dimension", level_id: levelId, kinds: ["rooms"] })}>
          Dimension the rooms
        </Button>
        {grid ? (
          <Button size="sm" variant="ghost" icon="grid" disabled={busy !== null} onClick={() => void run("grid", grid)}>
            Grid on the walls
          </Button>
        ) : null}
      </div>
    </Section>
  );
}

/** "45.34", for a table cell under an "m²" header. */
const m2 = (v: number): string => v.toFixed(2);

function PermitSchedules({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("permit_schedules", true);
  const sched = useMemo(() => buildPermitSchedules(doc), [doc]);
  const unit = doc.project.settings.display_unit;
  const levelCols = sched.levels.length > 1 && sched.levels.length <= 3;
  const multiLevel = sched.levels.length > 1;
  const doors = sched.openings.filter((o) => o.kind === "door");
  const windows = sched.openings.filter((o) => o.kind === "window");
  const openingRows = (rows: typeof sched.openings) =>
    rows.map((o) => (
      <tr key={o.key}>
        <td>
          {o.mark ? <strong>{o.mark}</strong> : <span className={s.schedSub}>Untyped</span>}
          <span className={s.schedSub}>{o.what}</span>
          {multiLevel && !levelCols ? (
            <span className={s.schedSub}>
              {o.perLevel
                .map((n, i) => (n > 0 ? `${sched.levels[i].name} ${n}` : ""))
                .filter(Boolean)
                .join(", ")}
            </span>
          ) : null}
        </td>
        <td className={s.num}>{o.size}</td>
        {levelCols ? o.perLevel.map((n, i) => <td key={sched.levels[i].id} className={s.num}>{n}</td>) : null}
        <td className={s.num}>
          <Bump value={o.total} />
        </td>
      </tr>
    ));
  const span = levelCols ? sched.levels.length + 2 : 2;

  return (
    <Section
      title="Door, window and room schedules"
      icon="schedule"
      open={open}
      onToggle={toggle}
      aside={<span className={s.sectionAside}>{formatArea(sched.totals.floorM2)}</span>}
    >
      {!sched.counted ? <p className={s.note}>The schedules appear once the engine has counted the doors, windows and rooms.</p> : null}

      {sched.openings.length > 0 ? (
        <>
          <div className={s.schedHead}>
            <Icon name="door" size={13} />
            <span>Doors and windows</span>
          </div>
          <table className={s.schedTable}>
            <thead>
              <tr>
                <th>Mark</th>
                <th className={s.num}>Size, mm</th>
                {levelCols ? sched.levels.map((l) => <th key={l.id} className={s.num}>{l.name}</th>) : null}
                <th className={s.num}>{levelCols ? "All" : "No."}</th>
              </tr>
            </thead>
            <tbody>
              {openingRows(doors)}
              {openingRows(windows)}
            </tbody>
            <tfoot>
              <tr>
                <td colSpan={span}>All doors</td>
                <td className={s.num}>
                  <Bump value={sched.totals.doors} />
                </td>
              </tr>
              <tr>
                <td colSpan={span}>All windows</td>
                <td className={s.num}>
                  <Bump value={sched.totals.windows} />
                </td>
              </tr>
            </tfoot>
          </table>
        </>
      ) : null}

      {sched.rooms.length > 0 ? (
        <>
          <div className={s.schedHead}>
            <Icon name="room" size={13} />
            <span>Rooms and finishes</span>
          </div>
          <table className={s.schedTable}>
            <thead>
              <tr>
                <th>Room</th>
                <th className={s.num}>Net, m²</th>
                <th className={s.num}>Ceiling</th>
              </tr>
            </thead>
            <tbody>
              {sched.rooms.map((r) => (
                <tr key={r.key}>
                  <td>
                    {multiLevel ? `${r.level}, ${r.name}` : r.name}
                    {r.finishes ? <span className={s.schedSub}>{r.finishes}</span> : null}
                    {r.remarks ? <span className={s.schedSub}>{r.remarks}</span> : null}
                  </td>
                  <td className={s.num}>
                    <Bump value={m2(r.netM2)} />
                  </td>
                  <td className={s.num}>{formatLength(r.ceilingMm, unit)}</td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <td>Floor area</td>
                <td className={s.num}>
                  <Bump value={m2(sched.totals.floorM2)} />
                </td>
                <td />
              </tr>
            </tfoot>
          </table>
        </>
      ) : null}

      {sched.levelAreas.length > 0 ? (
        <>
          <div className={s.schedHead}>
            <Icon name="level" size={13} />
            <span>Level areas, m²</span>
          </div>
          <table className={s.schedTable}>
            <thead>
              <tr>
                <th>Level</th>
                <th className={s.num}>Net</th>
                <th className={s.num}>Gross</th>
                <th className={s.num}>Open</th>
                <th className={s.num}>Void</th>
              </tr>
            </thead>
            <tbody>
              {sched.levelAreas.map((l) => (
                <tr key={l.key}>
                  <td>{l.name}</td>
                  <td className={s.num}>{m2(l.net)}</td>
                  <td className={s.num}>{m2(l.gross)}</td>
                  <td className={s.num}>{m2(l.open)}</td>
                  <td className={s.num}>{m2(l.void)}</td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <td>Project</td>
                <td className={s.num}>
                  <Bump value={m2(sched.totals.floorM2)} />
                </td>
                <td className={s.num}>
                  <Bump value={m2(sched.totals.grossM2)} />
                </td>
                <td className={s.num}>{m2(sched.totals.openM2)}</td>
                <td className={s.num}>{m2(sched.totals.voidM2)}</td>
              </tr>
            </tfoot>
          </table>
        </>
      ) : null}

      <p className={css.fact}>Net areas leave out voids, balconies and decks; open is balconies and decks.</p>
      <div className={s.actionsRow}>
        <CopyButton label="Copy as CSV" text={() => permitSchedulesCsv(sched)} />
      </div>
    </Section>
  );
}

function DeviceSchedules({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("schedules", true);
  const catalog = useApp((st) => st.catalog);
  const ref = useRef<HTMLDivElement>(null);
  const flash = useReveal("schedules", ref);
  const sched = useMemo(() => buildSchedules(doc, catalog), [doc, catalog]);
  const levelCols = sched.levels.length > 1 && sched.levels.length <= 3;
  const multiLevel = sched.levels.length > 1;
  const devices = sched.form.reduce((n, r) => n + r.total, 0);
  const counted = (doc.derived.schedule ?? []).length > 0;

  return (
    <div ref={ref} className={cx(s.reveal, flash && s.revealFlash)}>
      <Section
        title="Schedules"
        icon="schedule"
        open={open}
        onToggle={toggle}
        aside={devices > 0 ? <span className={s.sectionAside}>{devices} devices</span> : null}
      >
        {!counted ? <p className={s.note}>The counts appear once the engine has counted the objects.</p> : null}

        {sched.form.length > 0 ? (
          <>
            <div className={s.schedHead}>
              <Icon name="plug" size={13} />
              <span>Electrical devices</span>
            </div>
            <table className={s.schedTable}>
              <thead>
                <tr>
                  <th>Inspection form row</th>
                  {levelCols ? sched.levels.map((l) => <th key={l.id} className={s.num}>{l.name}</th>) : null}
                  <th className={s.num}>{levelCols ? "All" : "Count"}</th>
                </tr>
              </thead>
              <tbody>
                {sched.form.map((r) => (
                  <tr key={r.kind}>
                    <td>{r.label}</td>
                    {levelCols ? r.perLevel.map((n, i) => <td key={sched.levels[i].id} className={s.num}>{n}</td>) : null}
                    <td className={s.num}>{r.total}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className={s.schedHead}>
              <Icon name="room" size={13} />
              <span>By room</span>
            </div>
            <ul className={s.roomLines}>
              {sched.rooms.map((r) => (
                <li key={r.key}>
                  <strong>{multiLevel ? `${r.levelName}, ${r.roomName}` : r.roomName}</strong>
                  <span>{roomCountsLine(r.counts)}</span>
                </li>
              ))}
            </ul>
          </>
        ) : null}

        {sched.fixtures.length > 0 ? (
          <>
            <div className={s.schedHead}>
              <Icon name="bulb" size={13} />
              <span>Light fixtures</span>
            </div>
            <table className={s.schedTable}>
              <thead>
                <tr>
                  <th>Type</th>
                  <th className={s.num}>No.</th>
                  <th className={s.num}>Each</th>
                  <th className={s.num}>Color</th>
                </tr>
              </thead>
              <tbody>
                {sched.fixtures.map((f) => (
                  <tr key={f.key}>
                    <td>
                      {f.name}
                      {f.plugIn ? <span className={s.schedSub}>Plug-in, not a lighting outlet</span> : null}
                    </td>
                    <td className={s.num}>{f.count}</td>
                    <td className={s.num}>{Math.round(f.lumens)} lm</td>
                    <td className={s.num}>
                      <span className={s.swatchK} style={{ background: kelvinColor(f.kelvin) }} aria-hidden />
                      {Math.round(f.kelvin)} K
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        ) : null}

        {sched.aircon.length > 0 ? (
          <>
            <div className={s.schedHead}>
              <Icon name="aircon" size={13} />
              <span>Aircon units</span>
            </div>
            <table className={s.schedTable}>
              <thead>
                <tr>
                  <th>Room and unit</th>
                  <th className={s.num}>HP</th>
                  <th className={s.num}>No.</th>
                </tr>
              </thead>
              <tbody>
                {sched.aircon.map((a) => (
                  <tr key={a.key}>
                    <td>
                      {multiLevel ? `${a.levelName}, ${a.roomName}` : a.roomName}
                      <span className={s.schedSub}>
                        {a.role}: {a.name}
                      </span>
                    </td>
                    <td className={s.num}>{a.hp}</td>
                    <td className={s.num}>{a.count}</td>
                  </tr>
                ))}
              </tbody>
              {sched.coolingUnits > 0 ? (
                <tfoot>
                  <tr>
                    <td>Units that cool a room</td>
                    <td className={s.num}>{hpLabel(sched.coolingHp)}</td>
                    <td className={s.num}>{sched.coolingUnits}</td>
                  </tr>
                </tfoot>
              ) : null}
            </table>
          </>
        ) : null}

        <p className={s.disclaimer}>{SCHEDULE_NOTE}</p>
        <div className={s.actionsRow}>
          <CopyButton label="Copy as CSV" text={() => schedulesCsv(sched)} />
        </div>
      </Section>
    </div>
  );
}
