import { useEffect, useMemo, useRef, useState } from "react";
import type { AssetCategory, CatalogItem } from "../contract/bindings";
import { useApp } from "../state/store";
import { cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { Presence, useSlidingIndicator } from "../ui/motionDom";
import { formatLength } from "../ui/units";
import { DOOR_STYLES, TOOLS, WINDOW_STYLES, activateTool, type ToolDef } from "./actions";
import { useShell } from "./shellStore";
import s from "./chrome.module.css";

const CATEGORY_LABEL: Record<AssetCategory, string> = {
  furniture: "Furniture",
  sanitary: "Bathroom",
  kitchen: "Kitchen",
  appliance: "Appliances",
  plant: "Plants",
  vehicle: "Vehicles",
};

/** Common Philippine wall builds. null keeps the project default. */
const WALL_PRESETS: Array<{ mm: number | null; label: string; hint: string }> = [
  { mm: null, label: "Project default", hint: "" },
  { mm: 100, label: "100 mm", hint: "4 in CHB, interior" },
  { mm: 150, label: "150 mm", hint: "6 in CHB, exterior" },
  { mm: 200, label: "200 mm", hint: "8 in CHB or firewall" },
];

function Flyout({ tool, onClose, stage }: { tool: ToolDef; onClose: () => void; stage: "enter" | "idle" | "exit" }) {
  const ref = useRef<HTMLDivElement>(null);
  const toolOptions = useApp((st) => st.toolOptions);
  const setTool = useApp((st) => st.setTool);
  const catalog = useApp((st) => st.catalog);
  const loadCatalog = useApp((st) => st.loadCatalog);
  const settings = useApp((st) => st.doc?.project.settings);
  const [query, setQuery] = useState("");

  useEffect(() => {
    const down = (e: MouseEvent) => {
      const anchor = ref.current?.parentElement;
      if (anchor && !anchor.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [onClose]);

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const items = q ? catalog.filter((c) => `${c.name} ${c.category}`.toLowerCase().includes(q)) : catalog;
    const map = new Map<AssetCategory, CatalogItem[]>();
    for (const item of items) map.set(item.category, [...(map.get(item.category) ?? []), item]);
    return [...map.entries()];
  }, [catalog, query]);

  const unit = settings?.display_unit ?? "mm";

  return (
    <div
      ref={ref}
      className={cx(s.flyout, tool.flyout === "asset" && s.flyoutWide)}
      data-stage={stage}
      role="dialog"
      aria-label={`${tool.label} options`}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      {tool.flyout === "wall" ? (
        <>
          <div className={s.flyoutTitle}>Wall thickness</div>
          {WALL_PRESETS.map((p) => (
            <button
              key={String(p.mm)}
              type="button"
              className={cx(s.flyoutItem, toolOptions.wallThicknessMm === p.mm && s.flyoutItemOn)}
              onClick={() => {
                setTool("wall", { wallThicknessMm: p.mm });
                onClose();
              }}
            >
              <span>{p.mm === null && settings ? `Project default, ${formatLength(settings.default_wall_thickness_mm, unit)}` : p.label}</span>
              {p.hint ? <small>{p.hint}</small> : null}
            </button>
          ))}
        </>
      ) : null}

      {tool.flyout === "door" || tool.flyout === "window" ? (
        <>
          <div className={s.flyoutTitle}>{tool.flyout === "door" ? "Door type" : "Window type"}</div>
          {(tool.flyout === "door" ? DOOR_STYLES : WINDOW_STYLES).map((o) => (
            <button
              key={o.value}
              type="button"
              className={cx(s.flyoutItem, toolOptions.openingStyle === o.value && s.flyoutItemOn)}
              onClick={() => {
                setTool(tool.tool, { openingStyle: o.value });
                onClose();
              }}
            >
              <span>{o.label}</span>
            </button>
          ))}
        </>
      ) : null}

      {tool.flyout === "asset" ? (
        <>
          <label className={s.flyoutSearch}>
            <Icon name="search" size={14} />
            <input
              autoFocus
              type="text"
              value={query}
              placeholder="Find an object"
              aria-label="Find an object"
              spellCheck={false}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key !== "Escape") e.stopPropagation();
              }}
            />
          </label>
          <div className={s.flyoutScroll}>
            {catalog.length === 0 ? (
              <div className={s.flyoutEmpty}>
                The object library is not loaded.
                <button type="button" className={s.flyoutLink} onClick={() => void loadCatalog()}>
                  Try again
                </button>
              </div>
            ) : groups.length === 0 ? (
              <div className={s.flyoutEmpty}>Nothing matches "{query}".</div>
            ) : (
              groups.map(([category, items]) => (
                <div key={category}>
                  <div className={s.flyoutTitle}>{CATEGORY_LABEL[category] ?? category}</div>
                  {items.map((item) => (
                    <button
                      key={item.key}
                      type="button"
                      className={cx(s.flyoutItem, toolOptions.assetKey === item.key && s.flyoutItemOn)}
                      onClick={() => {
                        setTool("asset", { assetKey: item.key });
                        onClose();
                      }}
                    >
                      <span>{item.name}</span>
                      <small>
                        {Math.round(item.width_mm)} x {Math.round(item.depth_mm)}
                      </small>
                    </button>
                  ))}
                </div>
              ))
            )}
          </div>
        </>
      ) : null}
    </div>
  );
}

export function ToolRail() {
  const tool = useApp((st) => st.tool);
  const assetKey = useApp((st) => st.toolOptions.assetKey);
  const openPalette = useShell((st) => st.open);
  const flyoutRequest = useShell((st) => st.flyoutRequest);
  const [flyout, setFlyout] = useState<ToolDef["flyout"] | null>(null);
  const railRef = useRef<HTMLElement>(null);
  const activeButtonRef = useRef<HTMLButtonElement>(null);
  const pill = useSlidingIndicator(railRef, activeButtonRef, [tool]);

  // Leaving a tool by any route (shortcut, Escape) closes its flyout.
  useEffect(() => {
    setFlyout((f) => (f && TOOLS.find((t) => t.flyout === f)?.tool !== tool ? null : f));
  }, [tool]);

  // A keyboard shortcut (O for objects) asking to open a flyout it cannot
  // reach the local state for directly.
  useEffect(() => {
    if (flyoutRequest) setFlyout(flyoutRequest.flyout);
  }, [flyoutRequest]);

  const click = (t: ToolDef) => {
    if (t.flyout === "asset") {
      // The asset tool needs a chosen object before it can place anything.
      if (assetKey) activateTool("asset");
      setFlyout((f) => (f === "asset" ? null : "asset"));
      return;
    }
    const wasActive = tool === t.tool;
    activateTool(t.tool);
    if (t.flyout) setFlyout((f) => (f === t.flyout ? null : wasActive || t.flyout !== "wall" ? t.flyout! : null));
    else setFlyout(null);
  };

  return (
    <nav ref={railRef} className={s.rail} aria-label="Drawing tools">
      {pill.visible ? <span aria-hidden className={cx(s.railPill, pill.instant && s.instant)} style={pill.style} /> : null}
      {TOOLS.map((t, i) => (
        <div key={t.tool} className={cx(s.railSlot, (i === 1 || i === 3 || i === 8 || i === 11) && s.railGap)}>
          <button
            ref={tool === t.tool ? activeButtonRef : undefined}
            type="button"
            className={cx(s.railButton, tool === t.tool && s.railButtonOn)}
            aria-label={t.label}
            aria-pressed={tool === t.tool}
            data-tip={t.key ? `${t.label} (${t.key})` : t.label}
            data-tip-side="right"
            data-tip-off={flyout === t.flyout && t.flyout ? "true" : undefined}
            onClick={() => click(t)}
          >
            <Icon name={t.icon} size={20} />
            {t.flyout ? <span className={s.railCorner} aria-hidden /> : null}
          </button>
          {t.flyout ? (
            <Presence open={flyout === t.flyout} exit="base">
              {(stage) => <Flyout tool={t} onClose={() => setFlyout(null)} stage={stage} />}
            </Presence>
          ) : null}
        </div>
      ))}
      <div className={s.railSpacer} />
      <button
        type="button"
        className={s.railButton}
        aria-label="Keyboard shortcuts"
        data-tip="Keyboard shortcuts"
        data-tip-side="right"
        onClick={() => openPalette("shortcuts")}
      >
        <Icon name="keyboard" size={20} />
      </button>
    </nav>
  );
}
