// Schedules in the project inspector, shown when the project has devices or
// light fixtures: device counts in the rows of the PH electrical inspection
// form (per level and room), the light fixture table and the aircon units.
// Counts come from `Derived::schedule` and the model; nothing here sizes,
// loads or rates anything (DECISIONS D21).
import { useMemo, useRef } from "react";
import type { DocState } from "../contract/bindings";
import { useApp } from "../state/store";
import { Section, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { hpLabel } from "./devices";
import { CopyButton, useReveal } from "./PipeSections";
import { SCHEDULE_NOTE, buildSchedules, roomCountsLine, schedulesCsv } from "./schedules";
import { useSection } from "./shellStore";
import s from "./Inspector.module.css";

/** A warm to cool dot for a color temperature, 2700 K to 6500 K. */
function kelvinColor(k: number): string {
  const t = Math.min(1, Math.max(0, (k - 2700) / 3800));
  const mix = (a: number, b: number) => Math.round(a + (b - a) * t);
  return `rgb(${mix(255, 214)}, ${mix(196, 230)}, ${mix(120, 255)})`;
}

export function SchedulesSection({ doc }: { doc: DocState }) {
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
