// The pipe tool's flyout on the tool rail: system, size and start height.
// While the size and height options are null the tool uses the system
// defaults (docs/CONTRACT.md, "Pipes"), and the flyout says so.
import type { PipeMaterial } from "../contract/bindings";
import { useApp, type ToolOptions } from "../state/store";
import { NumberField, Segmented, cx } from "../ui/controls";
import {
  PIPE_MATERIAL_LABEL,
  PIPE_SYSTEMS,
  PIPE_SYSTEM_LABEL,
  closestSize,
  formatDiameter,
  materialsFor,
  pipeToolSettings,
  sizeShort,
  sizesFor,
  switchToolSystem,
} from "./pipes";
import s from "./chrome.module.css";

function DefaultTag({ on }: { on: boolean }) {
  return (
    <span className={cx(s.defaultTag, !on && s.defaultTagOff)} aria-hidden={!on}>
      Default
    </span>
  );
}

export function PipeOptions() {
  const toolOptions = useApp((st) => st.toolOptions);
  const setTool = useApp((st) => st.setTool);
  const unit = useApp((st) => st.doc?.project.settings.display_unit ?? "mm");
  const cur = pipeToolSettings(toolOptions);
  const set = (patch: Partial<ToolOptions>) => setTool("pipe", patch);
  const materials = materialsFor(cur.system);
  const sizes = sizesFor(cur.system, cur.material);
  const chosen = toolOptions.pipeMaterial !== null || toolOptions.pipeDiameterMm !== null || toolOptions.pipeElevationMm !== null;

  return (
    <>
      <div className={s.flyoutTitle}>System</div>
      <div role="radiogroup" aria-label="Pipe system">
        {PIPE_SYSTEMS.map((sys) => {
          // What the tool would draw after switching: a chosen size is kept
          // when the other system's menu has it, else its default.
          const next = pipeToolSettings(switchToolSystem(toolOptions, sys.value));
          const on = cur.system === sys.value;
          return (
            <button
              key={sys.value}
              type="button"
              role="radio"
              aria-checked={on}
              className={cx(s.flyoutItem, s.pipeSystem, on && s.flyoutItemOn)}
              onClick={() => {
                if (!on) setTool("pipe", switchToolSystem(toolOptions, sys.value));
              }}
            >
              <span className={s.pipeSwatch} style={{ background: sys.color }} aria-hidden />
              <span className={s.pipeSystemName}>{sys.label}</span>
              <small>{sizeShort(next.material, next.diameterMm)}</small>
            </button>
          );
        })}
      </div>

      <div className={s.flyoutRule} />
      <div className={s.flyoutTitleRow}>
        <span>Size in mm</span>
        <DefaultTag on={cur.sizeIsDefault} />
      </div>
      <div className={s.pipeBlock}>
        {materials.length > 1 ? (
          <Segmented<PipeMaterial>
            label="Pipe material"
            stretch
            value={cur.material}
            options={materials.map((m) => ({ value: m, label: PIPE_MATERIAL_LABEL[m] }))}
            onChange={(material) => set({ pipeMaterial: material, pipeDiameterMm: closestSize(sizesFor(cur.system, material), cur.diameterMm) })}
          />
        ) : (
          <span className={s.pipeMaterialOnly}>{PIPE_MATERIAL_LABEL[cur.material]}</span>
        )}
        <div className={s.sizeChips} role="radiogroup" aria-label={`${PIPE_MATERIAL_LABEL[cur.material]} size in millimeters`}>
          {sizes.map((d) => {
            const on = Math.abs(d - cur.diameterMm) < 1e-6;
            return (
              <button
                key={d}
                type="button"
                role="radio"
                aria-checked={on}
                className={cx(s.sizeChip, on && s.sizeChipOn)}
                onClick={() => set({ pipeMaterial: cur.material, pipeDiameterMm: d })}
              >
                {formatDiameter(d)}
              </button>
            );
          })}
        </div>
      </div>

      <div className={s.flyoutRule} />
      <div className={s.flyoutTitleRow}>
        <span>Start height</span>
        <DefaultTag on={cur.elevationIsDefault} />
      </div>
      <div className={s.pipeBlock}>
        <NumberField
          label="Start height above the floor"
          kind="length"
          unit={unit}
          min={-5000}
          max={12000}
          step={50}
          value={cur.elevationMm}
          onCommit={(v) => set({ pipeElevationMm: v })}
        />
        <p className={s.flyoutHint}>Centerline above the floor. Below zero runs under the slab.</p>
      </div>
      <div className={s.flyoutFoot}>
        <button
          type="button"
          className={cx(s.flyoutLink, !chosen && s.flyoutLinkOff)}
          disabled={!chosen}
          onClick={() => set({ pipeMaterial: null, pipeDiameterMm: null, pipeElevationMm: null })}
        >
          Use the {PIPE_SYSTEM_LABEL[cur.system].toLowerCase()} defaults
        </button>
      </div>
    </>
  );
}
