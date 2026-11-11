// Shared form and chrome primitives used by the hub and the editor shell.
import {
  useEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import type { DisplayUnit } from "../contract/bindings";
import { Icon, type IconName } from "./icons";
import { DUR } from "./motion";
import { useFlash, useSlidingIndicator } from "./motionDom";
import { lengthToInput, numberToInput, parseLength, parseNumber } from "./units";
import s from "./controls.module.css";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

/** Sentence-case section heading inside a panel (replaces tracked caps eyebrows). */
export const sectionLabelClass = s.sectionLabel;
/** A plain tabular count after a label, in ink-3, no fill. */
export const countClass = s.count;

// ---------------------------------------------------------------- buttons

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "primary" | "ghost" | "danger" | "chrome";
  size?: "sm" | "md";
  icon?: IconName;
}

export function Button({ variant = "default", size = "md", icon, children, className, ...rest }: ButtonProps) {
  return (
    <button type="button" {...rest} className={cx(s.button, s[`v_${variant}`], size === "sm" && s.sm, className)}>
      {icon ? <Icon name={icon} size={size === "sm" ? 14 : 16} /> : null}
      {children ? <span>{children}</span> : null}
    </button>
  );
}

interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: IconName;
  label: string;
  /** Tooltip text. Defaults to the label. */
  tip?: string;
  tipSide?: "bottom" | "right" | "top" | "top-start" | "top-end" | "bottom-start" | "bottom-end";
  active?: boolean;
  tone?: "light" | "chrome";
  size?: number;
}

export function IconButton({
  icon,
  label,
  tip,
  tipSide = "bottom",
  active,
  tone = "light",
  size = 18,
  className,
  ...rest
}: IconButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      aria-label={label}
      aria-pressed={active}
      data-tip={tip ?? label}
      data-tip-side={tipSide}
      className={cx(s.iconButton, tone === "chrome" && s.iconChrome, active && s.iconActive, className)}
    >
      <Icon name={icon} size={size} />
    </button>
  );
}

// ---------------------------------------------------------------- roving focus

export type RovingOrientation = "horizontal" | "vertical" | "both";

/**
 * The index a key moves to in a group of `count` items (WAI-ARIA roving
 * tabindex), or null when the key does not move. Arrows wrap; Home and End go
 * to the ends.
 */
/** Which way a key moves in a group: one step on, one step back, to an end, or not at all. */
function rovingStep(key: string, orientation: RovingOrientation): 1 | -1 | "home" | "end" | null {
  const nextKeys = orientation === "vertical" ? ["ArrowDown"] : orientation === "horizontal" ? ["ArrowRight"] : ["ArrowRight", "ArrowDown"];
  const prevKeys = orientation === "vertical" ? ["ArrowUp"] : orientation === "horizontal" ? ["ArrowLeft"] : ["ArrowLeft", "ArrowUp"];
  if (nextKeys.includes(key)) return 1;
  if (prevKeys.includes(key)) return -1;
  if (key === "Home") return "home";
  if (key === "End") return "end";
  return null;
}

export function rovingNext(key: string, index: number, count: number, orientation: RovingOrientation = "both"): number | null {
  if (count <= 0) return null;
  const step = rovingStep(key, orientation);
  if (step === null) return null;
  if (step === "home") return 0;
  if (step === "end") return count - 1;
  if (index < 0 || index >= count) return step === 1 ? 0 : count - 1;
  return (index + step + count) % count;
}

/**
 * `rovingNext` over a full list where some items are disabled: `index` and
 * the result are indices into the full list, and disabled items are skipped.
 * From a disabled or unknown index, the arrows go to the nearest enabled item
 * in their direction.
 */
export function rovingNextEnabled(key: string, index: number, enabled: boolean[], orientation: RovingOrientation = "both"): number | null {
  const ok = enabled.flatMap((on, i) => (on ? [i] : []));
  if (ok.length === 0) return null;
  const step = rovingStep(key, orientation);
  if (step === null) return null;
  if (step === "home") return ok[0];
  if (step === "end") return ok[ok.length - 1];
  const pos = ok.indexOf(index);
  if (pos >= 0) return ok[(pos + step + ok.length) % ok.length];
  // From a disabled or unknown item: the nearest enabled one that way, wrapping.
  const found = step === 1 ? ok.find((i) => i > index) : [...ok].reverse().find((i) => i < index);
  return found ?? (step === 1 ? ok[0] : ok[ok.length - 1]);
}

/**
 * The tab stop of item `i`: only the active item is reachable with Tab, or the
 * first enabled item when nothing (or a disabled item) is active.
 */
export function rovingTabIndex(i: number, active: number, isDisabled?: (i: number) => boolean): 0 | -1 {
  const off = isDisabled ?? (() => false);
  if (active >= 0 && !off(active)) return i === active ? 0 : -1;
  let first = 0;
  while (first < 256 && off(first)) first++;
  return i === first ? 0 : -1;
}

/**
 * One tab stop for a radio group, tab list or option row; the arrow keys,
 * Home and End move between its items and skip disabled ones. Mark each
 * item with `data-roving` and give it `tabIndex={roving.tabIndexFor(i)}`;
 * put `onKeyDown` on the group. Every index (`active`, `tabIndexFor`,
 * `onMove`, `isDisabled`) is an index into the full list of `data-roving`
 * items, disabled ones included, so `options[index]` is always the right
 * option. `onMove` gets the item that took focus: a radio group selects it
 * (arrows select, per WAI-ARIA), a tab list opens it.
 */
export function useRovingGroup({
  active,
  onMove,
  orientation = "both",
  isDisabled,
}: {
  active: number;
  onMove: (el: HTMLElement, index: number) => void;
  orientation?: RovingOrientation;
  /** Which items are disabled, so the tab stop never lands on one. */
  isDisabled?: (index: number) => boolean;
}) {
  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    const items = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-roving]")];
    const enabled = items.map(
      (el) => !(el as HTMLButtonElement).disabled && el.getAttribute("aria-disabled") !== "true",
    );
    const from = items.indexOf(document.activeElement as HTMLElement);
    const next = rovingNextEnabled(e.key, from >= 0 ? from : active, enabled, orientation);
    if (next === null) return;
    e.preventDefault();
    // Arrows here must not also nudge the selection on the plan.
    e.stopPropagation();
    const el = items[next];
    el.focus();
    onMove(el, next);
  };
  return { onKeyDown, tabIndexFor: (i: number) => rovingTabIndex(i, active, isDisabled) };
}

// ---------------------------------------------------------------- segmented

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon?: IconName;
  tip?: string;
}

interface SegmentedProps<T extends string> {
  options: Array<SegmentedOption<T>>;
  value: T;
  onChange: (value: T) => void;
  tone?: "light" | "chrome";
  label: string;
  iconOnly?: boolean;
  stretch?: boolean;
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  tone = "light",
  label,
  iconOnly,
  stretch,
}: SegmentedProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLButtonElement>(null);
  const thumb = useSlidingIndicator(containerRef, activeRef, [value, options.length]);
  const roving = useRovingGroup({
    active: options.findIndex((o) => o.value === value),
    onMove: (el) => {
      const next = options.find((o) => o.value === el.dataset.value);
      if (next && next.value !== value) onChange(next.value);
    },
  });
  return (
    <div
      ref={containerRef}
      role="radiogroup"
      aria-label={label}
      className={cx(s.segmented, tone === "chrome" && s.segChrome, stretch && s.segStretch)}
      onKeyDown={roving.onKeyDown}
    >
      {thumb.visible ? <span aria-hidden className={cx(s.segThumb, thumb.instant && s.segThumbInstant)} style={thumb.style} /> : null}
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={o.value === value ? activeRef : undefined}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          aria-label={iconOnly ? o.label : undefined}
          tabIndex={roving.tabIndexFor(i)}
          data-roving=""
          data-value={o.value}
          data-tip={o.tip}
          className={cx(s.segment, o.value === value && s.segmentOn)}
          onClick={() => onChange(o.value)}
        >
          {o.icon ? <Icon name={o.icon} size={15} /> : null}
          {iconOnly ? null : <span>{o.label}</span>}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- switch

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={cx(s.switch, checked && s.switchOn)}
      onClick={() => onChange(!checked)}
    >
      <span className={s.switchKnob} />
    </button>
  );
}

export function CheckRow({
  checked,
  onChange,
  children,
  hint,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  children: ReactNode;
  hint?: string;
}) {
  return (
    <label className={s.checkRow}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className={s.checkBox} aria-hidden>
        <Icon name="check" size={12} />
      </span>
      <span className={s.checkText}>
        {children}
        {hint ? <small>{hint}</small> : null}
      </span>
    </label>
  );
}

// ---------------------------------------------------------------- select

export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  disabled,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <span className={s.selectWrap}>
      <select
        aria-label={label}
        className={s.select}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as T)}
        onKeyDown={(e) => e.stopPropagation()}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" size={13} className={s.selectChevron} />
    </span>
  );
}

// ---------------------------------------------------------------- text and number fields

/** Shared commit-on-Enter-or-blur, revert-on-Escape behaviour. */
function useDraft(external: string, commit: (text: string) => boolean) {
  const [draft, setDraft] = useState<string | null>(null);
  const [invalid, setInvalid] = useState(false);
  const cancelled = useRef(false);

  useEffect(() => {
    if (!invalid) return;
    const t = window.setTimeout(() => setInvalid(false), 600);
    return () => window.clearTimeout(t);
  }, [invalid]);

  const finish = (text: string | null) => {
    if (text !== null && text !== external) {
      if (!commit(text)) setInvalid(true);
    }
    setDraft(null);
  };

  return {
    value: draft ?? external,
    invalid,
    setDraft,
    onFocus: (e: React.FocusEvent<HTMLInputElement | HTMLTextAreaElement>) => e.currentTarget.select(),
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setDraft(e.target.value),
    onBlur: () => {
      if (cancelled.current) {
        cancelled.current = false;
        setDraft(null);
        return;
      }
      finish(draft);
    },
    cancel: (el: HTMLElement) => {
      cancelled.current = true;
      el.blur();
    },
    commitNow: (el: HTMLElement) => {
      el.blur();
    },
  };
}

interface TextFieldProps {
  value: string;
  onCommit: (value: string) => void;
  label: string;
  placeholder?: string;
  /** When true an empty value is rejected and the field reverts. */
  required?: boolean;
  multiline?: boolean;
  className?: string;
  autoFocus?: boolean;
  onCancel?: () => void;
}

export function TextField({
  value,
  onCommit,
  label,
  placeholder,
  required,
  multiline,
  className,
  autoFocus,
  onCancel,
}: TextFieldProps) {
  const d = useDraft(value, (text) => {
    const next = multiline ? text : text.trim();
    if (required && next === "") return false;
    onCommit(next);
    return true;
  });
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    e.stopPropagation();
    if (e.key === "Enter" && (!multiline || e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      d.commitNow(e.currentTarget);
    } else if (e.key === "Escape") {
      e.preventDefault();
      d.cancel(e.currentTarget);
      onCancel?.();
    }
  };
  const shared = {
    "aria-label": label,
    placeholder,
    value: d.value,
    onFocus: d.onFocus,
    onChange: d.onChange,
    onBlur: () => {
      d.onBlur();
    },
    onKeyDown,
    autoFocus,
    spellCheck: false,
    className: cx(s.input, d.invalid && s.inputInvalid, className),
  };
  return multiline ? <textarea rows={3} {...shared} /> : <input type="text" {...shared} />;
}

interface NumberFieldProps {
  /** Millimeters when kind is "length", a plain number otherwise. */
  value: number;
  onCommit: (value: number) => void;
  label: string;
  kind?: "length" | "number";
  unit?: DisplayUnit;
  /** Suffix shown for plain numbers, for example "deg". Lengths show the unit. */
  suffix?: string;
  min?: number;
  max?: number;
  /** Nudge step in stored units (mm for lengths). Shift multiplies by 10. */
  step?: number;
  decimals?: number;
  integer?: boolean;
  disabled?: boolean;
  /**
   * The value differs across a selection: the field shows "Mixed" and any
   * number typed commits, even one equal to `value`.
   */
  mixed?: boolean;
}

export function NumberField({
  value,
  onCommit,
  label,
  kind = "number",
  unit = "mm",
  suffix,
  min,
  max,
  step,
  decimals = 2,
  integer,
  disabled,
  mixed,
}: NumberFieldProps) {
  const isLength = kind === "length";
  const toText = (n: number) => (isLength ? lengthToInput(n, unit) : numberToInput(n, integer ? 0 : decimals));
  const parse = (text: string) => (isLength ? parseLength(text, unit) : parseNumber(text));
  const clamp = (n: number) => {
    let v = integer ? Math.round(n) : n;
    if (min !== undefined) v = Math.max(min, v);
    if (max !== undefined) v = Math.min(max, v);
    return v;
  };
  // A committed value flashes teal briefly (MOTION.md, Inspector row).
  const flash = useFlash(DUR.panel);
  const send = (n: number) => {
    const v = clamp(n);
    if (mixed || Math.abs(v - value) > 1e-6) {
      onCommit(v);
      flash.fire();
    }
  };
  const d = useDraft(mixed ? "" : toText(value), (text) => {
    const n = parse(text);
    if (n === null) return false;
    send(n);
    return true;
  });
  const nudgeStep = step ?? (isLength ? (unit === "m" ? 50 : 10) : 1);

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      d.commitNow(e.currentTarget);
    } else if (e.key === "Escape") {
      e.preventDefault();
      d.cancel(e.currentTarget);
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const base = parse(d.value) ?? value;
      const delta = (e.key === "ArrowUp" ? 1 : -1) * nudgeStep * (e.shiftKey ? 10 : 1);
      d.setDraft(null);
      send(base + delta);
    }
  };

  return (
    <span className={cx(s.numberWrap, d.invalid && s.inputInvalid, flash.active && s.numberFlash, disabled && s.numberDisabled)}>
      <input
        type="text"
        inputMode="decimal"
        aria-label={label}
        className={s.numberInput}
        value={d.value}
        placeholder={mixed ? "Mixed" : undefined}
        disabled={disabled}
        onFocus={d.onFocus}
        onChange={d.onChange}
        onBlur={d.onBlur}
        onKeyDown={onKeyDown}
        spellCheck={false}
      />
      <span className={s.numberSuffix}>{isLength ? unit : suffix}</span>
    </span>
  );
}

// ---------------------------------------------------------------- layout helpers

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <div className={s.field}>
      <span className={s.fieldLabel} title={hint}>
        {label}
      </span>
      <div className={s.fieldControl}>{children}</div>
    </div>
  );
}

export function ReadOnly({ children }: { children: ReactNode }) {
  return <span className={s.readOnly}>{children}</span>;
}

export function Section({
  title,
  icon,
  count,
  open,
  onToggle,
  children,
  aside,
}: {
  title: string;
  icon?: IconName;
  count?: number;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
  aside?: ReactNode;
}) {
  return (
    <section className={s.section}>
      <header className={s.sectionHead}>
        <button type="button" className={s.sectionToggle} aria-expanded={open} onClick={onToggle}>
          <Icon name="chevronRight" size={12} className={cx(s.sectionChevron, open && s.sectionChevronOpen)} />
          {icon ? <Icon name={icon} size={15} /> : null}
          <span>{title}</span>
          {count !== undefined && count > 0 ? <span className={s.sectionCount}>{count}</span> : null}
        </button>
        {aside}
      </header>
      {/* Height reveal via grid-template-rows 0fr -> 1fr (MOTION.md rule 5), not a
          layout-property animation loop: it only runs once per toggle. */}
      <div className={cx(s.sectionReveal, open && s.sectionRevealOpen)} inert={!open}>
        {/* The clip takes the 0fr row; the padded body inside it collapses fully. */}
        <div className={s.sectionClip}>
          <div className={s.sectionBody}>{children}</div>
        </div>
      </div>
    </section>
  );
}

export function Spinner({ size = 18, className }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" className={cx(s.spinner, className)} aria-hidden>
      <circle cx="10" cy="10" r="7" stroke="currentColor" strokeOpacity={0.25} strokeWidth={1.5} />
      <path d="M10 3a7 7 0 0 1 7 7" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" />
    </svg>
  );
}
