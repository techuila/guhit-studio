// Stair fields (lane L2): the shape (straight, L or U) and turn, size and
// risers, the first flight, landing and the gap of a U, rotation, the
// tread count and each tread and riser as the engine worked them out
// (`Derived::stairs`), and the handrail. Fields only some shapes take open
// and close with the shape.
import type { ReactNode } from "react";
import type { DocState, StairShape, Turn } from "../../contract/bindings";
import { MIN_TURNING_RISERS, U_WELL_MM } from "../../editor2d/stairTool";
import { Field, NumberField, ReadOnly, Segmented, Switch, cx } from "../../ui/controls";
import { formatLength } from "../../ui/units";
import { PointFields, Position, RailingFields, Rotation, update, type Of } from "./common";
import { Bump } from "./permitCommon";
import css from "./roofStair.module.css";
import s from "../Inspector.module.css";

/** New stairs get a 900 mm handrail (DECISIONS D50). */
const STAIR_RAILING_HEIGHT_MM = 900;

const SHAPES: Array<{ value: StairShape; label: string }> = [
  { value: "straight", label: "Straight" },
  { value: "l", label: "L" },
  { value: "u", label: "U" },
];

const TURNS: Array<{ value: Turn; label: string }> = [
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

/** Fields only some shapes take: they open and close by grid rows. */
export function Reveal({ open, children }: { open: boolean; children: ReactNode }) {
  return (
    <div className={cx(css.reveal, open && css.revealOpen)} inert={!open} aria-hidden={!open}>
      <div className={css.revealClip}>
        <div className={css.revealBody}>{children}</div>
      </div>
    </div>
  );
}

/** Treads of a stair: risers less one per flight (D44 extended by D54). */
export function treadCount(el: Pick<Of<"stair">, "riser_count" | "shape">): number {
  const turning = (el.shape ?? "straight") !== "straight" && el.riser_count >= MIN_TURNING_RISERS;
  return Math.max(1, el.riser_count - (turning ? 2 : 1));
}

/**
 * The stair with another shape: an L or U takes at least 4 risers, a new U
 * a 100 mm gap, and the run follows so each tread keeps its depth.
 */
export function withShape(el: Of<"stair">, shape: StairShape): Of<"stair"> {
  const going = el.run_mm / treadCount(el);
  const turning = shape !== "straight";
  const riser_count = turning ? Math.max(MIN_TURNING_RISERS, el.riser_count) : el.riser_count;
  const next: Of<"stair"> = { ...el, shape, riser_count };
  next.run_mm = Math.round(going * treadCount(next));
  if (shape === "u" && !el.well_mm) next.well_mm = U_WELL_MM;
  return next;
}

export function StairFields({ el, doc }: { el: Of<"stair">; doc: DocState }) {
  const unit = doc.project.settings.display_unit;
  const level = doc.project.levels.find((l) => l.id === el.level_id);
  const g = (doc.derived.stairs ?? []).find((x) => x.stair_id === el.id) ?? null;
  const shape = el.shape ?? "straight";
  const turning = shape !== "straight";
  const n = el.riser_count;
  const treads = g ? g.flights.reduce((t, f) => t + f.nosings.length, 0) : treadCount(el);
  const tread = g ? g.tread_mm : el.run_mm / treads;
  const riser = g ? g.riser_mm : level ? level.height_mm / Math.max(1, n) : null;
  // What the engine made of "half": the first flight's treads and its top riser.
  const firstRisers = g && turning && g.flights.length > 1 ? g.flights[0].nosings.length + 1 : Math.min(Math.max(2, Math.ceil(n / 2)), Math.max(2, n - 2));
  const set = (patch: Partial<Of<"stair">>) => update(el, { ...el, ...patch });
  return (
    <>
      <div className={s.group}>
        <Field label="Shape">
          <Segmented label="Stair shape" stretch value={shape} options={SHAPES} onChange={(next) => next !== shape && update(el, withShape(el, next))} />
        </Field>
        <Reveal open={turning}>
          <Field label="Turns" hint="Which way it turns going up">
            <Segmented label="Stair turns" stretch value={el.turn ?? "left"} options={TURNS} onChange={(turn) => set({ turn })} />
          </Field>
        </Reveal>
        <Field label="Width">
          <NumberField label="Stair width" kind="length" unit={unit} min={500} max={5000} step={50} value={el.width_mm} onCommit={(width_mm) => set({ width_mm })} />
        </Field>
        <Field label="Run" hint="Every tread together, landings left out">
          <NumberField label="Stair run" kind="length" unit={unit} min={500} max={20000} step={50} value={el.run_mm} onCommit={(run_mm) => set({ run_mm })} />
        </Field>
        <Field label="Risers">
          <NumberField label="Number of risers" integer min={turning ? MIN_TURNING_RISERS : 2} max={40} value={n} onCommit={(riser_count) => set({ riser_count })} />
        </Field>
        <Reveal open={turning}>
          <Field label="First flight" hint="Risers up to the landing; Half splits them evenly">
            <NumberField label="Risers in the first flight" integer min={2} max={Math.max(2, n - 2)} value={el.first_flight_risers ?? firstRisers} onCommit={(first_flight_risers) => set({ first_flight_risers })} />
            <label className={css.auto}>
              <Switch label="Half the risers in the first flight" checked={el.first_flight_risers == null} onChange={(on) => set({ first_flight_risers: on ? null : firstRisers })} />
              <span>Half</span>
            </label>
          </Field>
          <Field label="Landing" hint="Depth along the way up; Width makes it square">
            <NumberField label="Landing depth" kind="length" unit={unit} min={300} max={10000} step={50} value={el.landing_mm ?? el.width_mm} onCommit={(landing_mm) => set({ landing_mm })} />
            <label className={css.auto}>
              <Switch label="Landing as deep as the stair is wide" checked={el.landing_mm == null} onChange={(on) => set({ landing_mm: on ? null : el.width_mm })} />
              <span>Width</span>
            </label>
          </Field>
        </Reveal>
        <Reveal open={shape === "u"}>
          <Field label="Gap" hint="Clear gap between the two flights">
            <NumberField label="Gap between flights" kind="length" unit={unit} min={0} max={10000} step={10} value={el.well_mm ?? 0} onCommit={(well_mm) => set({ well_mm })} />
          </Field>
        </Reveal>
        <Rotation value={el.rotation_deg} onCommit={(rotation_deg) => set({ rotation_deg })} />
        <Field label="Treads">
          <ReadOnly>
            <Bump value={treads} />
          </ReadOnly>
        </Field>
        <Field label="Each tread">
          <ReadOnly>
            <Bump value={formatLength(tread, unit)} />
          </ReadOnly>
        </Field>
        <Field label="Each riser" hint="Level height divided by the number of risers">
          <ReadOnly>
            <Bump value={riser ? formatLength(riser, unit) : "-"} />
          </ReadOnly>
        </Field>
        <p className={css.fact}>
          {turning
            ? `${n} risers make ${Math.max(1, n - 2)} treads: each flight's top riser lands on the landing or the floor above.`
            : `${n} risers make ${Math.max(1, n - 1)} treads: the top riser lands on the floor above.`}
        </p>
        <RailingFields railing={el.railing} unit={unit} defaultHeight={STAIR_RAILING_HEIGHT_MM} what="Handrail" onChange={(railing) => set({ railing })} />
      </div>
      <Position>
        <PointFields label="First riser" point={el.origin} unit={unit} onCommit={(origin) => set({ origin })} />
      </Position>
    </>
  );
}
