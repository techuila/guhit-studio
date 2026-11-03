// Inspector fields for one pipe run (DECISIONS D39), Revit's way: the whole
// run, the segment or the point picked on the plan (src/editor2d/pipePick.ts).
// A level run has one Height; a sloped run has Start and End height, and a
// run that falls has its Slope with the end that holds. Every geometry edit
// is one pipe command with joins kept (src/editor2d/pipeEdit.ts), so joined
// runs follow. Stored in mm, shown in the display unit.
import { Fragment } from "react";
import type { Command, DocState, Element, PipeEnd } from "../contract/bindings";
import { usePipePick } from "../editor2d/pipePick";
import {
  heightSetCommand,
  insertBendCommand,
  isLevelRun,
  lengthCommand,
  nodeOf,
  pointHeightCommand,
  pointPlanCommand,
  rangeSlopePct,
  removePointCommand,
  runEndHeightCommand,
  segmentIsRiser,
  segmentLength,
  slopeCommand,
  validPick,
} from "../editor2d/pipeEdit";
import { useApp } from "../state/store";
import { Button, Field, NumberField, ReadOnly, Section, Segmented, Select, TextField, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { formatLength } from "../ui/units";
import { walkTo } from "./actions";
import {
  PIPE_COLOR,
  PIPE_MATERIAL_LABEL,
  PIPE_SYSTEMS,
  drainMinSlopePct,
  formatDiameter,
  inMenu,
  materialsFor,
  midPoint,
  pipeFalls,
  pipeLength,
  reversed,
  segmentFalls,
  sizesFor,
  tradeOf,
  withMaterial,
  withSystem,
  type SegmentFall,
} from "./pipes";
import { useSection } from "./shellStore";
import s from "./Inspector.module.css";
import p from "./PipeFields.module.css";

type PipeOf = Extract<Element, { kind: "pipe" }>;
type Unit = "mm" | "m";

const run = (command: Command | null): Promise<unknown> => (command ? useApp.getState().dispatch(command) : Promise.resolve(null));
const update = (element: Element) => void run({ type: "update_element", element });

function pct(value: number, digits: number): string {
  return `${value.toFixed(digits)}%`;
}

/** How one drainage segment falls, in words. */
function fallText(f: SegmentFall, min: number, unit: Unit): string {
  const over = `over ${formatLength(f.horizontalMm, unit)}`;
  if (f.pct === null) return `Vertical, ${f.dropMm >= 0 ? "drops" : "rises"} ${formatLength(Math.abs(f.dropMm), unit)}`;
  if (f.steep) return `${f.dropMm >= 0 ? "Drops" : "Rises"} ${formatLength(Math.abs(f.dropMm), unit)} ${over}`;
  // A low fall never reads as the default itself: 1.996 shows 1.996, not 2.00.
  const digits = f.low && Math.abs(Number(f.pct.toFixed(2))) >= min ? 3 : 2;
  if (Math.abs(f.dropMm) < 0.05) return `Level ${over}`;
  if (f.dropMm < 0) return `Rises ${pct(-f.pct, digits)} ${over}`;
  return `Falls ${pct(f.pct, digits)} ${over}`;
}

const HEIGHT_LIMITS = { min: -5000, max: 20000, step: 10 } as const;

function Height({ label, aria, value, unit, onCommit }: { label: string; aria: string; value: number; unit: Unit; onCommit: (z: number) => void }) {
  return (
    <Field label={label}>
      <NumberField label={aria} kind="length" unit={unit} {...HEIGHT_LIMITS} value={value} onCommit={onCommit} />
    </Field>
  );
}

/** Which end keeps its place when a length or a slope changes. Shared with the plan tags. */
function Hold() {
  const hold = usePipePick((st) => st.hold);
  const setHold = usePipePick((st) => st.setHold);
  return (
    <Field label="Keep fixed" hint="The end that stays put when a length or slope changes">
      <Segmented<PipeEnd>
        label="End that stays put"
        stretch
        value={hold}
        onChange={setHold}
        options={[
          { value: "start", label: "Start" },
          { value: "end", label: "End" },
        ]}
      />
    </Field>
  );
}

function Slope({ value, aria, onCommit }: { value: number; aria: string; onCommit: (pct: number) => void }) {
  return (
    <Field label="Slope" hint="Fall in percent of the plan length, in flow order. Negative rises">
      <NumberField label={aria} suffix="%" decimals={2} step={0.5} value={value} onCommit={onCommit} />
    </Field>
  );
}

/** What is being edited, with the way back to the whole run. */
function PartBar({ el, title }: { el: PipeOf; title: string | null }) {
  const setPick = usePipePick((st) => st.setPick);
  return (
    <div className={p.partBar}>
      <button type="button" className={cx(p.crumb, title === null && p.crumbOn)} onClick={() => setPick(null)} disabled={title === null}>
        Whole run
      </button>
      {title ? (
        <>
          <Icon name="chevronRight" size={12} className={p.crumbSep} />
          <span key={title} className={cx(p.crumb, p.crumbOn, p.crumbIn)}>
            {title}
          </span>
        </>
      ) : (
        <span className={p.partHint}>Click a segment or point of the {el.points.length > 2 ? "run" : "pipe"} on the plan</span>
      )}
    </div>
  );
}

function RunGeometry({ el, unit }: { el: PipeOf; unit: Unit }) {
  const pts = el.points;
  const falls = pipeFalls(el.system);
  const level = isLevelRun(pts);
  const hold = usePipePick((st) => st.hold);
  const slope = falls ? rangeSlopePct(pts) : null;
  return (
    <>
      {level ? (
        <Height label="Height" aria="Run height" value={pts[0].z} unit={unit} onCommit={(z) => void run(heightSetCommand(el, null, z))} />
      ) : (
        <>
          <Height label="Start height" aria="Start height" value={pts[0].z} unit={unit} onCommit={(z) => void run(runEndHeightCommand(el, "start", z))} />
          <Height label="End height" aria="End height" value={pts[pts.length - 1].z} unit={unit} onCommit={(z) => void run(runEndHeightCommand(el, "end", z))} />
        </>
      )}
      {falls && slope !== null ? (
        <>
          <Slope value={slope} aria="Run slope" onCommit={(v) => void run(slopeCommand(el, 0, pts.length - 2, v, hold))} />
          <Hold />
        </>
      ) : null}
    </>
  );
}

function SegmentGeometry({ el, index, unit }: { el: PipeOf; index: number; unit: Unit }) {
  const pts = el.points;
  const hold = usePipePick((st) => st.hold);
  const setPick = usePipePick((st) => st.setPick);
  // A slope only for the systems that fall, and never on a riser, as for the run.
  const slope = pipeFalls(el.system) && !segmentIsRiser(pts, index) ? rangeSlopePct(pts, index, index) : null;
  return (
    <>
      <Field label="Length" hint="Along the segment, in 3D">
        <NumberField label="Segment length" kind="length" unit={unit} value={segmentLength(pts, index)} onCommit={(v) => void run(lengthCommand(el, index, v, hold))} />
      </Field>
      <Height label="Start height" aria="Segment start height" value={pts[index].z} unit={unit} onCommit={(z) => void run(pointHeightCommand(el, index, z, true))} />
      <Height label="End height" aria="Segment end height" value={pts[index + 1].z} unit={unit} onCommit={(z) => void run(pointHeightCommand(el, index + 1, z, true))} />
      {slope !== null ? <Slope value={slope} aria="Segment slope" onCommit={(v) => void run(slopeCommand(el, index, index, v, hold))} /> : null}
      <Hold />
      <div className={s.actionsRow}>
        <Button
          size="sm"
          icon="plus"
          onClick={() =>
            void run(insertBendCommand(el, index)).then((r) => {
              if (r) setPick({ pipeId: el.id, kind: "point", index: index + 1 });
            })
          }
        >
          Add bend here
        </Button>
      </div>
    </>
  );
}

function PointGeometry({ el, index, unit }: { el: PipeOf; index: number; unit: Unit }) {
  const pts = el.points;
  const setPick = usePipePick((st) => st.setPick);
  const node = nodeOf(pts, index);
  const first = pts[node.index];
  const last = pts[node.index + node.count - 1];
  const riser = node.count > 1 && Math.abs(last.z - first.z) >= 1;
  const end = node.index === 0 || node.index + node.count === pts.length;
  const x = (first.x + last.x) / 2;
  const y = (first.y + last.y) / 2;
  return (
    <>
      <Field label="Position" hint="Plan x and y. A riser moves as one">
        <NumberField label="Point x" kind="length" unit={unit} value={x} onCommit={(v) => void run(pointPlanCommand(el, node.index, { x: first.x + (v - x), y: first.y }))} />
        <NumberField label="Point y" kind="length" unit={unit} value={y} onCommit={(v) => void run(pointPlanCommand(el, node.index, { x: first.x, y: first.y + (v - y) }))} />
      </Field>
      {riser ? (
        <>
          <Height label="Riser from" aria="Riser start height" value={first.z} unit={unit} onCommit={(z) => void run(pointHeightCommand(el, node.index, z, true))} />
          <Height label="Riser to" aria="Riser end height" value={last.z} unit={unit} onCommit={(z) => void run(pointHeightCommand(el, node.index + node.count - 1, z, true))} />
        </>
      ) : (
        <Height label="Height" aria="Point height" value={first.z} unit={unit} onCommit={(z) => void run(pointHeightCommand(el, node.index, z))} />
      )}
      <div className={s.actionsRow}>
        <Button
          size="sm"
          icon="trash"
          disabled={pts.length <= 2}
          onClick={() =>
            void run(removePointCommand(el, node.index)).then((r) => {
              if (r) setPick(null);
            })
          }
        >
          {end ? "Remove point" : "Remove bend"}
        </Button>
      </div>
    </>
  );
}

/** Every point with editable x, y and z, tucked away under the run. */
function PointList({ el, unit }: { el: PipeOf; unit: Unit }) {
  const [open, toggle] = useSection("pipePoints", false);
  const falls = pipeFalls(el.system);
  const segFalls = falls ? segmentFalls(el) : [];
  const min = drainMinSlopePct(el.diameter_mm);
  const one = (i: number, axis: "x" | "y", v: number) => {
    const q = el.points[i];
    return pointPlanCommand(el, i, axis === "x" ? { x: v, y: q.y } : { x: q.x, y: v }, true);
  };
  return (
    <Section title={falls ? "Points, in flow order" : "Points"} count={el.points.length} open={open} onToggle={toggle}>
      <div className={p.pointsHead}>
        <span />
        <span>x</span>
        <span>y</span>
        <span>Height</span>
      </div>
      <div className={s.points}>
        {el.points.map((q, i) => (
          <Fragment key={i}>
            <div className={p.pointRow}>
              <span className={s.pointIndex}>{i + 1}</span>
              <NumberField label={`Point ${i + 1} x`} kind="length" unit={unit} value={q.x} onCommit={(v) => void run(one(i, "x", v))} />
              <NumberField label={`Point ${i + 1} y`} kind="length" unit={unit} value={q.y} onCommit={(v) => void run(one(i, "y", v))} />
              <NumberField label={`Height of point ${i + 1}`} kind="length" unit={unit} {...HEIGHT_LIMITS} value={q.z} onCommit={(z) => void run(pointHeightCommand(el, i, z, true))} />
            </div>
            {falls && i < segFalls.length ? (
              <div className={cx(s.fall, segFalls[i].low && s.fallLow)} title={segFalls[i].low ? (segFalls[i].dropMm < 0 ? "Runs uphill" : `Less than the ${min}% default fall for this size`) : undefined}>
                <Icon name={segFalls[i].low ? "warning" : "chevronDown"} size={12} />
                <span>{fallText(segFalls[i], min, unit)}</span>
              </div>
            ) : null}
          </Fragment>
        ))}
      </div>
    </Section>
  );
}

export function PipeFields({ el, doc }: { el: PipeOf; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const rawPick = usePipePick((st) => st.pick);
  const pick = rawPick && rawPick.pipeId === el.id ? validPick(el.points, rawPick) : null;
  // Drainage, storm and condensate flow from the first point to the last, and fall.
  const drainage = pipeFalls(el.system);
  const falls = drainage ? segmentFalls(el) : [];
  const min = drainMinSlopePct(el.diameter_mm);
  const lowCount = falls.filter((f) => f.low).length;

  const materials = materialsFor(el.system);
  const materialOptions = (materials.includes(el.material) ? materials : [...materials, el.material]).map((m) => ({ value: m, label: PIPE_MATERIAL_LABEL[m] }));
  const sizes = sizesFor(el.system, el.material);
  const sizeOptions = (inMenu(el.system, el.material, el.diameter_mm) ? sizes : [...sizes, el.diameter_mm].sort((a, b) => a - b)).map((d) => ({
    value: String(d),
    label: `${formatDiameter(d)} mm`,
  }));
  const segments = el.points.length - 1;
  const title = !pick ? null : pick.kind === "segment" ? `Segment ${pick.index + 1} of ${segments}` : `Point ${pick.index + 1}`;
  const partKey = pick ? `${pick.kind}:${pick.index}` : "run";

  return (
    <>
      <div className={s.group}>
        <Field label="Name">
          <TextField label="Run name" value={el.name} placeholder={tradeOf(el.system).group === "plumbing" ? "For example, sink waste" : tradeOf(el.system).group === "electrical" ? "For example, kitchen outlets" : "For example, bedroom line set"} onCommit={(name) => update({ ...el, name })} />
        </Field>
        <Field label="System" hint="Each system has its own layer">
          <span className={s.pipeSwatch} style={{ background: PIPE_COLOR[el.system] }} aria-hidden />
          <Select label="Pipe system" value={el.system} options={PIPE_SYSTEMS.map((o) => ({ value: o.value, label: o.label }))} onChange={(system) => update(withSystem(el, system))} />
        </Field>
        <Field label="Material">
          <Select label="Pipe material" value={el.material} options={materialOptions} onChange={(material) => update(withMaterial(el, material))} />
        </Field>
        <Field label={el.system === "refrigerant" ? "Gas line" : "Size"} hint={`Nominal size${el.system === "refrigerant" ? " of the gas line; the liquid line is 6.35 mm" : ""}. Sizing is for ${tradeOf(el.system).pro}.`}>
          <Select label="Pipe size" value={String(el.diameter_mm)} options={sizeOptions} onChange={(v) => update({ ...el, diameter_mm: Number(v) })} />
        </Field>
        <Field label="Length" hint="Centerline, along every segment">
          <ReadOnly>{formatLength(pipeLength(el.points), unit)}</ReadOnly>
        </Field>
      </div>
      <div className={s.group}>
        <PartBar el={el} title={title} />
        <div key={partKey} className={p.part}>
          {!pick ? <RunGeometry el={el} unit={unit} /> : pick.kind === "segment" ? <SegmentGeometry el={el} index={pick.index} unit={unit} /> : <PointGeometry el={el} index={pick.index} unit={unit} />}
        </div>
        {drainage ? (
          <p className={s.note}>
            Flows from point 1 to point {el.points.length}. {lowCount > 0 ? `${lowCount === 1 ? "One segment falls" : `${lowCount} segments fall`} less than the ${min}% default.` : `Every segment falls at least the ${min}% default.`} A suggestion, not a code check.
          </p>
        ) : null}
        <div className={s.actionsRow}>
          {drainage ? (
            <Button size="sm" icon="flip" onClick={() => update(reversed(el))}>
              Reverse flow
            </Button>
          ) : null}
          <Button size="sm" icon="walk" onClick={() => void walkTo([el.id], midPoint(el.points))}>
            Walk here
          </Button>
        </div>
      </div>
      <PointList el={el} unit={unit} />
    </>
  );
}
