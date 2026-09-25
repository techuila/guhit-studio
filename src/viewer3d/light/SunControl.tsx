// Sun and light, in the 3D toolbar in place of the old Shadows toggle: the
// button shows the time, the popover holds the presets, the time and date,
// the sky, brightness, lamps, shadows, the sun path and refine
// (docs/CONTRACT.md, "Sun and light"). Everything here changes
// `useViewer().light` or a view toggle only: nothing is saved in the project
// and nothing is an undo step. The U and I keys are bound by the shell and
// land in the same store.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { SkyKind } from "../../contract/bindings";
import { siteLabel, siteOf } from "../../shell/site";
import { sunPresets, type SunPreset } from "../../shell/sun";
import { bus } from "../../state/bus";
import { useApp } from "../../state/store";
import { Segmented, Switch } from "../../ui/controls";
import { useSlidingIndicator } from "../../ui/motionDom";
import { usePresence } from "../../ui/motion";
import { useViewer, type LampMode } from "../viewerStore";
import { currentYear } from "./model";
import { clockLabel, compassLabel, dateLabel, sunPosition, sunTimes } from "./sun";
import vs from "../Viewer3D.module.css";
import s from "./SunControl.module.css";

/** The time slider's range: 5:00 to 20:00. */
const SLIDER_MIN = 5 * 60;
const SLIDER_MAX = 20 * 60;

interface DateChip {
  key: string;
  label: string;
  /** Null: today at the site. */
  md: [number, number] | null;
  tip: string;
}

const DATE_CHIPS: DateChip[] = [
  { key: "today", label: "Today", md: null, tip: "Today's sun" },
  { key: "hot", label: "Hot season", md: [5, 15], tip: "May 15: the hottest month, the sun high and in the north at noon" },
  { key: "jun", label: "Jun 21", md: [6, 21], tip: "June solstice: the sun at its most northern" },
  { key: "dec", label: "Dec 21", md: [12, 21], tip: "December solstice: the sun at its most southern and lowest" },
];

const SKY_OPTIONS: Array<{ value: SkyKind; label: string; tip: string }> = [
  { value: "clear", label: "Clear", tip: "A physical sky that follows the sun" },
  { value: "cloudy", label: "Cloudy", tip: "Overcast: soft light, faint shadows" },
  { value: "photo", label: "Photo", tip: "A photographed sky, turned to the sun" },
];

const LAMP_OPTIONS: Array<{ value: LampMode; label: string; tip: string }> = [
  { value: "auto", label: "Auto", tip: "Fixtures light up after sunset" },
  { value: "on", label: "On", tip: "Fixtures lit at any time (Shift+N)" },
  { value: "off", label: "Off", tip: "Fixtures off" },
];

const stroke = { fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", strokeLinejoin: "round" } as const;

/** Today at the project's site, as month and day. */
function todayAt(utcOffsetMin: number): [number, number] {
  const d = new Date(Date.now() + utcOffsetMin * 60_000);
  return [d.getUTCMonth() + 1, d.getUTCDate()];
}

function SunGlyph({ night, dusk }: { night: boolean; dusk: boolean }) {
  if (night) {
    return <path {...stroke} d="M12.6 10.2A5.4 5.4 0 0 1 5.8 3.4a5.4 5.4 0 1 0 6.8 6.8z" />;
  }
  if (dusk) {
    return (
      <>
        <path {...stroke} d="M4.4 11a3.6 3.6 0 0 1 7.2 0M1.8 11h12.4M3.4 13.4h9.2" />
        <path {...stroke} d="M8 3.6v1.6M3 6l1.1 1.1M13 6l-1.1 1.1" />
      </>
    );
  }
  return (
    <>
      <circle {...stroke} cx="8" cy="8" r="2.9" />
      <path {...stroke} d="M8 1.6v1.5M8 12.9v1.5M1.6 8h1.5M12.9 8h1.5M3.5 3.5l1 1M11.5 11.5l1 1M3.5 12.5l1-1M11.5 4.5l1-1" />
    </>
  );
}

export interface SunControlProps {
  /** The exposure offset auto settled on, for switching Brightness to a fixed value without a jump. */
  lockedEv?: () => number;
}

export function SunControl({ lockedEv }: SunControlProps) {
  const [open, setOpen] = useState(false);
  const presence = usePresence(open, "base");
  const anchorRef = useRef<HTMLDivElement>(null);
  const light = useViewer((v) => v.light);
  const settings = useApp((a) => a.doc?.project.settings ?? null);
  const site = siteOf(settings);
  const year = currentYear();
  const times = useMemo(() => sunTimes(site, year, light.month, light.day), [site, year, light.month, light.day]);
  const sun = useMemo(() => sunPosition(site, year, light.month, light.day, light.minutes), [site, year, light.month, light.day, light.minutes]);
  const night = sun.altitudeDeg < -0.833;
  const dusk = !night && sun.altitudeDeg < 6 && light.minutes > 12 * 60;

  const close = useCallback(() => setOpen(false), []);

  // Outside click and Escape close it. Escape is taken before the global
  // shortcuts, which would otherwise read it.
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      if (anchorRef.current && !anchorRef.current.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      e.preventDefault();
      setOpen(false);
    };
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("keydown", key, true);
    };
  }, [open]);

  return (
    <div className={s.anchor} ref={anchorRef}>
      <button
        type="button"
        className={vs.btn}
        data-active={open}
        aria-expanded={open}
        aria-haspopup="dialog"
        data-testid="sun-control"
        title="Sun and light: time, date, sky, lamps and shadows (U and I move the sun)"
        onClick={() => setOpen((o) => !o)}
      >
        <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true" className={s.sunIcon} data-night={night}>
          <SunGlyph night={night} dusk={dusk} />
        </svg>
        <span className={`${vs.labelAlways} ${s.time}`} data-testid="sun-time">
          {clockLabel(light.minutes)}
        </span>
      </button>
      {presence.mounted && (
        <SunPopover
          stage={presence.stage}
          site={site}
          year={year}
          times={times}
          sunAlt={sun.altitudeDeg}
          sunAz={sun.azimuthDeg}
          lockedEv={lockedEv}
          onClose={close}
        />
      )}
    </div>
  );
}

function SunPopover(props: {
  stage: string;
  site: ReturnType<typeof siteOf>;
  year: number;
  times: ReturnType<typeof sunTimes>;
  sunAlt: number;
  sunAz: number;
  lockedEv?: () => number;
  onClose: () => void;
}) {
  const { site, year, times } = props;
  const light = useViewer((v) => v.light);
  const setLight = useViewer((v) => v.setLight);
  const shadows = useViewer((v) => v.shadows);
  const toggleShadows = useViewer((v) => v.toggleShadows);
  const sunPath = useViewer((v) => v.sunPath);
  const toggleSunPath = useViewer((v) => v.toggleSunPath);
  const refine = useViewer((v) => v.refine);
  const setRefine = useViewer((v) => v.setRefine);

  const presets = useMemo(() => sunPresets(site, year, light.month, light.day), [site, year, light.month, light.day]);
  const active: SunPreset | undefined = presets.find((p) => Math.abs(p.minutes - light.minutes) < 0.5);
  const [tm, td] = todayAt(site.utc_offset_min);

  const rowRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLButtonElement>(null);
  const pill = useSlidingIndicator(rowRef, activeRef, [active?.id ?? "", presets.length]);

  const pct = (m: number) => `${((Math.min(Math.max(m, SLIDER_MIN), SLIDER_MAX) - SLIDER_MIN) / (SLIDER_MAX - SLIDER_MIN)) * 100}%`;
  const auto = light.exposureEv === null;
  const ev = light.exposureEv ?? 0;

  return (
    <div className={s.popover} data-stage={props.stage} role="dialog" aria-label="Sun and light" data-testid="sun-popover">
      <div className={s.head}>
        <span className={s.clock}>{clockLabel(light.minutes)}</span>
        <span className={s.where}>
          {siteLabel(site)}, {dateLabel(light.month, light.day)}
          <br />
          {times.sunrise !== null && times.sunset !== null
            ? `Sunrise ${clockLabel(times.sunrise)}, sunset ${clockLabel(times.sunset)}`
            : "No sunrise or sunset today"}
        </span>
      </div>
      <div className={s.sunLine} data-testid="sun-line">
        {props.sunAlt > -0.833
          ? `Sun ${Math.round(props.sunAlt)} degrees up, ${compassLabel(props.sunAz)} (${Math.round(props.sunAz)} degrees)`
          : `Sun down, ${Math.round(-props.sunAlt)} degrees below the horizon`}
      </div>

      <div className={s.presets} ref={rowRef} role="radiogroup" aria-label="Presets">
        <span
          aria-hidden="true"
          className={s.presetPill}
          data-hidden={!active || !pill.visible}
          data-instant={pill.instant}
          style={pill.style}
        />
        {presets.map((p) => (
          <button
            key={p.id}
            ref={p.id === active?.id ? activeRef : undefined}
            type="button"
            role="radio"
            aria-checked={p.id === active?.id}
            className={s.preset}
            data-active={p.id === active?.id}
            data-testid={`sun-preset-${p.id}`}
            title={p.id === "dusk" ? "Twenty minutes after sunset, lamps on" : `${p.label}, ${clockLabel(p.minutes)} (Shift+U, Shift+I)`}
            onClick={() => setLight({ minutes: p.minutes, lamps: p.lamps })}
          >
            {p.label}
            <small>{clockLabel(p.minutes).replace(":00", "")}</small>
          </button>
        ))}
      </div>

      <div className={s.section}>
        <span className={s.label}>Time</span>
        <div className={s.slider}>
          <input
            className={s.range}
            type="range"
            min={SLIDER_MIN}
            max={SLIDER_MAX}
            step={5}
            value={Math.min(Math.max(light.minutes, SLIDER_MIN), SLIDER_MAX)}
            aria-label="Time of day"
            aria-valuetext={clockLabel(light.minutes)}
            data-testid="sun-time-slider"
            onChange={(e) => setLight({ minutes: Number(e.target.value) })}
            onKeyDown={(e) => e.stopPropagation()}
          />
          {times.sunrise !== null && (
            <span className={s.tick} style={{ left: pct(times.sunrise) }}>
              Sunrise
            </span>
          )}
          {times.sunset !== null && (
            <span className={s.tick} style={{ left: pct(times.sunset) }}>
              Sunset
            </span>
          )}
        </div>
      </div>

      <div className={s.section}>
        <span className={s.label}>Date</span>
        <div className={s.chips} role="group" aria-label="Date">
          {DATE_CHIPS.map((c) => {
            const [m, d] = c.md ?? [tm, td];
            const on = light.month === m && light.day === d;
            return (
              <button
                key={c.key}
                type="button"
                className={s.chip}
                data-active={on}
                aria-pressed={on}
                title={c.tip}
                data-testid={`sun-date-${c.key}`}
                onClick={() => setLight({ month: m, day: d })}
              >
                {c.label}
              </button>
            );
          })}
        </div>
      </div>

      <div className={s.section}>
        <span className={s.label}>Sky</span>
        <Segmented label="Sky" stretch value={light.sky} options={SKY_OPTIONS.map((o) => ({ ...o }))} onChange={(sky) => setLight({ sky })} />
      </div>

      <div className={s.section}>
        <div className={s.row}>
          <span className={s.label}>Brightness</span>
          <span className={s.row}>
            Auto
            <Switch
              label="Automatic brightness"
              checked={auto}
              onChange={(next) => setLight({ exposureEv: next ? null : Math.round((props.lockedEv?.() ?? 0) * 10) / 10 })}
            />
          </span>
        </div>
        <div className={s.ev} data-open={!auto}>
          <div className={s.evInner}>
            <input
              className={s.range}
              type="range"
              min={-2}
              max={2}
              step={0.1}
              value={ev}
              disabled={auto}
              aria-label="Brightness in EV"
              data-testid="sun-ev"
              onChange={(e) => setLight({ exposureEv: Number(e.target.value) })}
              onKeyDown={(e) => e.stopPropagation()}
            />
            <span className={s.evValue}>
              {ev > 0 ? "+" : ""}
              {ev.toFixed(1)} EV
            </span>
          </div>
        </div>
      </div>

      <div className={s.section}>
        <span className={s.label}>Lamps</span>
        <Segmented label="Lamps" stretch value={light.lamps} options={LAMP_OPTIONS.map((o) => ({ ...o }))} onChange={(lamps) => setLight({ lamps })} />
      </div>

      <div className={s.toggles}>
        <label className={s.row}>
          Shadows
          <Switch label="Sun shadows" checked={shadows} onChange={() => toggleShadows()} />
        </label>
        <label className={s.row}>
          Sun path
          <Switch label="Sun path" checked={sunPath} onChange={() => toggleSunPath()} />
        </label>
        <label className={s.row} title="When the camera rests, soften shadows and edges, then stop drawing">
          Refine when still
          <Switch label="Refine when still" checked={refine} onChange={(next) => setRefine(next)} />
        </label>
      </div>

      <div className={s.foot}>
        <span className={s.hint}>
          <kbd>U</kbd> <kbd>I</kbd> move the sun
        </span>
        <button
          type="button"
          className={s.footBtn}
          data-testid="sun-shadow-study"
          onClick={() => {
            props.onClose();
            bus.emit("shadow_study");
          }}
        >
          Shadow study
        </button>
      </div>
    </div>
  );
}
