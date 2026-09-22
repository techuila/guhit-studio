// Material picker: a swatch button that expands in place into swatches
// grouped by category. Inline (not floating) so it never clips in the inspector.
import { useMemo, useState } from "react";
import type { Material, MaterialCategory } from "../contract/bindings";
import { cx } from "./controls";
import { Icon } from "./icons";
import { Presence } from "./motionDom";
import s from "./MaterialPicker.module.css";

const CATEGORY_LABEL: Record<MaterialCategory, string> = {
  wall: "Wall finishes",
  floor: "Floors",
  roof: "Roofing",
  glass: "Glass",
  wood: "Wood",
  metal: "Metal",
  generic: "Other",
};

const ORDER: MaterialCategory[] = ["wall", "floor", "roof", "wood", "metal", "glass", "generic"];

interface Props {
  materials: Material[];
  value: string | null;
  onChange: (materialId: string | null) => void;
  label: string;
  /** Categories listed first because they fit the element. */
  prefer?: MaterialCategory[];
  /** Text for the "no material" choice. Omit to require a material. */
  noneLabel?: string;
  /** Shown instead of a name when a multi-selection has different materials. */
  mixed?: boolean;
}

function Swatch({ material, size = 16 }: { material: Material | null; size?: number }) {
  if (!material) return <span className={cx(s.swatch, s.swatchNone)} style={{ width: size, height: size }} />;
  return (
    <span
      className={s.swatch}
      style={{ width: size, height: size, background: material.color, opacity: Math.max(0.35, material.opacity) }}
    />
  );
}

export function MaterialPicker({ materials, value, onChange, label, prefer = [], noneLabel, mixed }: Props) {
  const [open, setOpen] = useState(false);
  const current = materials.find((m) => m.id === value) ?? null;

  const groups = useMemo(() => {
    const order = [...prefer, ...ORDER.filter((c) => !prefer.includes(c))];
    return order
      .map((category) => ({ category, items: materials.filter((m) => m.category === category) }))
      .filter((g) => g.items.length > 0);
  }, [materials, prefer]);

  return (
    <div className={s.root}>
      <button
        type="button"
        className={cx(s.trigger, open && s.triggerOpen)}
        aria-label={label}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <Swatch material={mixed ? null : current} />
        <span className={s.triggerName}>{mixed ? "Mixed" : (current?.name ?? noneLabel ?? "Choose a material")}</span>
        <Icon name="chevronDown" size={13} className={cx(s.chevron, open && s.chevronOpen)} />
      </button>
      <Presence open={open} exit="base">
        {(stage) => (
          <div className={s.panel} data-stage={stage} onKeyDown={(e) => e.key === "Escape" && (e.stopPropagation(), setOpen(false))}>
          {noneLabel ? (
            <button
              type="button"
              className={cx(s.noneRow, value === null && !mixed && s.noneRowOn)}
              onClick={() => {
                onChange(null);
                setOpen(false);
              }}
            >
              <Swatch material={null} size={14} />
              <span>{noneLabel}</span>
            </button>
          ) : null}
          {groups.map((g) => (
            <div key={g.category} className={s.group}>
              <div className={s.groupLabel}>{CATEGORY_LABEL[g.category]}</div>
              <div className={s.grid}>
                {g.items.map((m) => (
                  <button
                    key={m.id}
                    type="button"
                    className={cx(s.cell, m.id === value && !mixed && s.cellOn)}
                    data-tip={m.name}
                    data-tip-side="top"
                    aria-label={m.name}
                    aria-pressed={m.id === value}
                    onClick={() => {
                      onChange(m.id);
                      setOpen(false);
                    }}
                  >
                    <span className={s.cellFill} style={{ background: m.color, opacity: Math.max(0.35, m.opacity) }} />
                  </button>
                ))}
              </div>
            </div>
          ))}
          {groups.length === 0 ? <div className={s.empty}>This project has no materials yet.</div> : null}
          </div>
        )}
      </Presence>
    </div>
  );
}
