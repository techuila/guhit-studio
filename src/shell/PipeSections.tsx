// Inspector parts for pipes that are not one element's fields: the Plumbing
// take-off shown when nothing is selected, and system and size for several
// selected pipes. Quantities come from `Derived::pipes`, computed by the
// engine; nothing here measures on its own.
import { useEffect, useRef, useState } from "react";
import type { Command, DocState, Element, PipeSystem } from "../contract/bindings";
import { useApp } from "../state/store";
import { Button, Field, Section, Select, cx } from "../ui/controls";
import { copyText } from "../ui/clipboard";
import { Icon } from "../ui/icons";
import { motionOK } from "../ui/motion";
import {
  EMPTY_NETWORK,
  PIPE_COLOR,
  PIPE_SIZES,
  PIPE_SYSTEMS,
  PIPE_SYSTEM_LABEL,
  TAKEOFF_NOTE,
  fittingsLine,
  runCount,
  sizeLabel,
  sizeShort,
  takeoffCsv,
  withSystem,
} from "./pipes";
import { useSection, useShell } from "./shellStore";
import s from "./Inspector.module.css";

type PipeElement = Extract<Element, { kind: "pipe" }>;

const dispatch = (command: Command) => void useApp.getState().dispatch(command);

// ---------------------------------------------------------------- copy button

/** A button whose label cross-fades to "Copied" with a drawn check mark, then back. */
function CopyButton({ label, text }: { label: string; text: () => string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const run = async () => {
    if (!(await copyText(text()))) {
      useApp.getState().toast("error", "The clipboard refused the copy. Try again.");
      return;
    }
    setCopied(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 1800);
  };

  return (
    <Button size="sm" onClick={() => void run()} className={s.copyButton} aria-label={copied ? "Copied to the clipboard" : label}>
      <span className={s.copyIcon} aria-hidden>
        <Icon name="copy" size={14} className={cx(s.copyLayer, !copied && s.copyLayerOn)} />
        <svg width={14} height={14} viewBox="0 0 20 20" fill="none" className={cx(s.copyLayer, s.copyCheck, copied && s.copyLayerOn)}>
          <path d="M4.5 10.5l3.5 3.5 7.5-8" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </span>
      <span className={s.copyStack} aria-live="polite">
        <span className={cx(s.copyText, !copied && s.copyTextOn)}>{label}</span>
        <span className={cx(s.copyText, copied && s.copyTextOn)}>Copied</span>
      </span>
    </Button>
  );
}

// ---------------------------------------------------------------- take-off

/** The reveal token already scrolled to, so a remount does not scroll again. */
let revealedToken = 0;

export function PlumbingSection({ doc }: { doc: DocState }) {
  const [open, toggle] = useSection("plumbing", true);
  const net = doc.derived.pipes ?? EMPTY_NETWORK;
  const rows = net.takeoff;
  const ref = useRef<HTMLDivElement>(null);
  const reveal = useShell((st) => st.revealRequest);
  const [flash, setFlash] = useState(false);

  // "Show the pipe take-off" from the palette: scroll here and flash once.
  useEffect(() => {
    if (!reveal || reveal.key !== "plumbing" || reveal.token === revealedToken) return;
    revealedToken = reveal.token;
    ref.current?.scrollIntoView({ block: "start", behavior: motionOK() ? "smooth" : "auto" });
    setFlash(false);
    const raf = requestAnimationFrame(() => setFlash(true));
    const t = window.setTimeout(() => setFlash(false), 900);
    return () => {
      cancelAnimationFrame(raf);
      window.clearTimeout(t);
    };
  }, [reveal]);

  return (
    <div ref={ref} className={cx(s.reveal, flash && s.revealFlash)}>
      <Section
        title="Plumbing"
        icon="pipe"
        open={open}
        onToggle={toggle}
        aside={rows.length > 0 ? <span className={s.sectionAside}>{net.total_length_m.toFixed(2)} m of pipe</span> : null}
      >
        {rows.length === 0 ? (
          <p className={s.note}>The take-off appears once the engine has measured the pipe runs.</p>
        ) : (
          <>
            <table className={s.takeoff}>
              <thead>
                <tr>
                  <th>System</th>
                  <th>Size</th>
                  <th className={s.num}>Length</th>
                  <th className={s.num}>Runs</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => {
                  const first = i === 0 || rows[i - 1].system !== r.system;
                  return (
                    <tr key={`${r.system}-${r.material}-${r.diameter_mm}`} className={cx(first && i > 0 && s.takeoffGroup)}>
                      <td>
                        {first ? (
                          <span className={s.takeoffSystem}>
                            <span className={s.pipeSwatch} style={{ background: PIPE_COLOR[r.system] }} aria-hidden />
                            {PIPE_SYSTEM_LABEL[r.system]}
                          </span>
                        ) : null}
                      </td>
                      <td>{sizeShort(r.material, r.diameter_mm)}</td>
                      <td className={s.num}>{r.length_m.toFixed(2)} m</td>
                      <td className={s.num}>{r.run_count}</td>
                    </tr>
                  );
                })}
              </tbody>
              <tfoot>
                <tr>
                  <td colSpan={2}>All pipe</td>
                  <td className={s.num}>{net.total_length_m.toFixed(2)} m</td>
                  <td className={s.num}>{runCount(rows)}</td>
                </tr>
              </tfoot>
            </table>
            <p className={s.takeoffFittings}>{fittingsLine(net)}.</p>
            <p className={s.disclaimer}>{TAKEOFF_NOTE}</p>
            <div className={s.actionsRow}>
              <CopyButton label="Copy as CSV" text={() => takeoffCsv(net)} />
            </div>
          </>
        )}
      </Section>
    </div>
  );
}

// ---------------------------------------------------------------- several pipes

const MIXED = "";

/** System and size for every selected pipe, each change one undo step. */
export function PipesMultiFields({ pipes }: { pipes: PipeElement[] }) {
  const systems = new Set(pipes.map((p) => p.system));
  const system: PipeSystem | null = systems.size === 1 ? pipes[0].system : null;
  const sizeKeys = new Set(pipes.map((p) => `${p.material}:${p.diameter_mm}`));
  const size = sizeKeys.size === 1 ? [...sizeKeys][0] : MIXED;
  const n = pipes.length;

  const sizeOptions = system
    ? PIPE_SIZES[system].flatMap((g) => g.sizes.map((d) => ({ value: `${g.material}:${d}`, label: sizeLabel(g.material, d) })))
    : [];
  if (size !== MIXED && !sizeOptions.some((o) => o.value === size)) sizeOptions.push({ value: size, label: sizeLabel(pipes[0].material, pipes[0].diameter_mm) });

  const setSystem = (next: string) => {
    if (next === MIXED) return;
    const target = next as PipeSystem;
    const commands: Command[] = pipes.filter((p) => p.system !== target).map((p) => ({ type: "update_element", element: withSystem(p, target) }));
    if (commands.length === 0) return;
    dispatch({ type: "batch", label: n === 1 ? "Set pipe system" : `Set system of ${n} pipes`, commands });
  };

  const setSize = (value: string) => {
    if (value === MIXED) return;
    const [material, d] = value.split(":");
    const diameter = Number(d);
    dispatch({
      type: "batch",
      label: n === 1 ? "Set pipe size" : `Set size of ${n} pipes`,
      commands: pipes.map((p) => ({ type: "update_element", element: { ...p, material: material as PipeElement["material"], diameter_mm: diameter } })),
    });
  };

  return (
    <>
      <Field label="System" hint="Moves the pipes to that system's layer">
        {system ? <span className={s.pipeSwatch} style={{ background: PIPE_COLOR[system] }} aria-hidden /> : null}
        <Select<string>
          label={`System for the ${n} selected pipes`}
          value={system ?? MIXED}
          options={[...(system ? [] : [{ value: MIXED, label: "Mixed systems" }]), ...PIPE_SYSTEMS.map((p) => ({ value: p.value, label: p.label }))]}
          onChange={setSystem}
        />
      </Field>
      <Field label="Size">
        <Select<string>
          label={`Size for the ${n} selected pipes`}
          disabled={!system}
          value={system ? size : MIXED}
          options={system ? [...(size === MIXED ? [{ value: MIXED, label: "Mixed sizes" }] : []), ...sizeOptions] : [{ value: MIXED, label: "Pick one system first" }]}
          onChange={setSize}
        />
      </Field>
    </>
  );
}
