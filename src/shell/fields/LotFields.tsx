// Lot fields (lane L1): the area, each side's bearing and length (from
// `Derived::site`), its role, setback and road, the grade, the building's
// coverage as a fact, and the title's technical description to set the lot
// from. Setbacks and coverage are drawn facts, never code checks.
import { useState } from "react";
import type { DocState, LotSide, LotSideRole } from "../../contract/bindings";
import { Button, Field, NumberField, ReadOnly, Section, Segmented, Switch, TextField, cx } from "../../ui/controls";
import { formatArea, formatLength } from "../../ui/units";
import { useSection } from "../shellStore";
import { update, type Of } from "./common";
import { Bump, setLotFromText } from "./permitCommon";
import css from "./permit.module.css";
import s from "../Inspector.module.css";

const ROLES: Array<{ value: LotSideRole; label: string }> = [
  { value: "road", label: "Road" },
  { value: "side", label: "Side" },
  { value: "rear", label: "Rear" },
];

/** A road's width picked when a road side gets one. */
const DEFAULT_ROAD_MM = 6000;

export function LotFields({ el, doc }: { el: Of<"lot">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const site = doc.derived.site && doc.derived.site.lot_id === el.id ? doc.derived.site : null;
  const [openSide, setOpenSide] = useState<number | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pasteOpen, togglePaste] = useSection("lot_paste", false);
  const area = site ? formatArea(site.area_m2) : "-";
  const n = el.corners.length;
  const setSide = (i: number, patch: Partial<LotSide>) => update(el, { ...el, sides: el.sides.map((sd, j) => (j === i ? { ...sd, ...patch } : sd)) });

  const setFromText = async () => {
    setBusy(true);
    setError(null);
    try {
      const start = el.corners[0] ?? { x: 0, y: 0 };
      const why = await setLotFromText(text, { x: start.x, y: start.y });
      if (why === null) setText("");
      else if (why !== "") setError(why);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className={s.group}>
        <Field label="Area">
          <ReadOnly>
            <Bump value={area} />
          </ReadOnly>
        </Field>
        <Field label="Grade" hint="Finish grade below the lowest floor">
          <NumberField label="Finish grade below the lowest floor" kind="length" unit={unit} min={0} max={10000} step={50} value={el.grade_mm} onCommit={(grade_mm) => update(el, { ...el, grade_mm })} />
        </Field>
        {site ? (
          <p className={css.fact}>
            Building covers <Bump value={`${Math.round(site.coverage_pct)}%`} /> of the lot.
          </p>
        ) : null}
        <ul className={css.sides}>
          {el.corners.map((_, i) => {
            const geo = site?.sides[i];
            const side = el.sides[i] ?? { role: "side", setback_mm: 0, road: "", road_width_mm: null };
            const open = openSide === i;
            return (
              <li key={i} className={cx(css.side, open && css.sideOpen)} style={{ animationDelay: `${Math.min(i, 7) * 30}ms` }}>
                <button type="button" className={css.sideHead} aria-expanded={open} onClick={() => setOpenSide(open ? null : i)}>
                  <span className={css.sideNo}>
                    {i + 1}-{((i + 1) % n) + 1}
                  </span>
                  <span className={css.sideBearing}>
                    {geo?.bearing ?? "-"}
                    <span className={css.roleTag}>{ROLES.find((r) => r.value === side.role)?.label}</span>
                  </span>
                  <span className={css.sideLength}>{geo ? formatLength(geo.length_mm, "m") : "-"}</span>
                </button>
                <div className={cx(css.sideReveal, open && css.sideRevealOpen)} inert={!open}>
                  <div className={css.sideClip}>
                    <div className={css.sideBody}>
                      <Field label="Faces">
                        <Segmented label={`Side ${i + 1} faces`} stretch value={side.role} options={ROLES} onChange={(role) => setSide(i, { role })} />
                      </Field>
                      <Field label="Setback" hint="Drawn along this side; 0 is none">
                        <NumberField label={`Side ${i + 1} setback`} kind="length" unit={unit} min={0} max={50000} step={100} value={side.setback_mm} onCommit={(setback_mm) => setSide(i, { setback_mm })} />
                      </Field>
                      {side.role === "road" ? (
                        <>
                          <Field label="Road">
                            <TextField label={`Side ${i + 1} road name`} value={side.road} placeholder="Road name" onCommit={(road) => setSide(i, { road })} />
                          </Field>
                          <Field label="Road width">
                            <NumberField
                              label={`Side ${i + 1} road width`}
                              kind="length"
                              unit={unit}
                              min={1000}
                              max={100000}
                              step={500}
                              disabled={side.road_width_mm === null}
                              value={side.road_width_mm ?? DEFAULT_ROAD_MM}
                              onCommit={(road_width_mm) => setSide(i, { road_width_mm })}
                            />
                          </Field>
                          <Field label="">
                            <label className={s.inlineSwitch}>
                              <Switch label="The road's width is known" checked={side.road_width_mm !== null} onChange={(known) => setSide(i, { road_width_mm: known ? DEFAULT_ROAD_MM : null })} />
                              <span>Width known</span>
                            </label>
                          </Field>
                        </>
                      ) : null}
                      {geo?.clearance_mm != null ? <p className={css.fact}>Nearest building face {formatLength(geo.clearance_mm, unit)} from this side.</p> : null}
                    </div>
                  </div>
                </div>
              </li>
            );
          })}
        </ul>
        <p className={s.note}>Setbacks and coverage are drawn as facts, not checked against a code.</p>
      </div>
      <Section title="Technical description" icon="pin" open={pasteOpen} onToggle={togglePaste}>
        <textarea
          className={css.paste}
          aria-label="Paste the technical description"
          placeholder={"Paste the technical description from the title, or one leg per line:\nN 45-30 E 15.00"}
          value={text}
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void setFromText();
          }}
        />
        {error ? (
          <p key={error} className={css.error} role="alert">
            {error}
          </p>
        ) : null}
        <div className={s.actionsRow}>
          <Button size="sm" icon="check" disabled={busy || text.trim() === ""} onClick={() => void setFromText()}>
            Set
          </Button>
        </div>
        <p className={s.note}>It starts at corner 1 and replaces the corners. The summary says the area and how far the legs miss closing.</p>
      </Section>
    </>
  );
}
