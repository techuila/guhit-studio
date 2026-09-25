import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { AssetCategory, CatalogItem } from "../contract/bindings";
import { useApp } from "../state/store";
import { cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { Presence, useSlidingIndicator } from "../ui/motionDom";
import { formatLength } from "../ui/units";
import type { DisplayUnit } from "../contract/bindings";
import { DOOR_STYLES, TOOLS, WINDOW_STYLES, activateTool, type ToolDef } from "./actions";
import { DEVICE_LABEL, hpLabel, mountRef, mountingHeight } from "./devices";
import { PipeOptions } from "./PipeOptions";
import { useShell } from "./shellStore";
import s from "./chrome.module.css";

const CATEGORY_LABEL: Record<AssetCategory, string> = {
  furniture: "Furniture",
  sanitary: "Bathroom",
  kitchen: "Kitchen",
  appliance: "Appliances",
  plant: "Plants",
  vehicle: "Vehicles",
  lighting: "Lighting",
  electrical: "Electrical",
  aircon: "Aircon",
  utility: "Utility",
};

/** Library order: rooms first, then the services, then the site. */
const CATEGORY_ORDER: AssetCategory[] = ["furniture", "sanitary", "kitchen", "appliance", "lighting", "electrical", "aircon", "utility", "plant", "vehicle"];

/** Words a search matches besides the name: the category, the device row, the key ("spo", "hp"). */
function searchText(item: CatalogItem): string {
  const device = item.device ? DEVICE_LABEL[item.device].join(" ") : "";
  const extra = item.light ? "light lamp fixture lumens" : item.aircon ? `aircon air conditioner ac split ${item.aircon.role} ${item.aircon.hp} hp` : "";
  return `${item.name} ${item.category} ${CATEGORY_LABEL[item.category] ?? ""} ${device} ${item.key.replace(/-/g, " ")} ${extra}`.toLowerCase();
}

/** The short reading beside an item: lumens for a light, HP for aircon, the height for a wall box, else its size. */
function itemMeta(item: CatalogItem, unit: DisplayUnit): string {
  if (item.light) return `${Math.round(item.light.lumens)} lm`;
  if (item.aircon) return hpLabel(item.aircon.hp);
  if (item.device && mountRef(item) === "center") return `at ${formatLength(mountingHeight(item, "center"), unit)}`;
  return `${Math.round(item.width_mm)} x ${Math.round(item.depth_mm)}`;
}

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

  // A tall flyout (pipe, objects) low on the rail would run off a short
  // window: lift it by the overflow, before the first paint.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.top = "";
    // Layout box, not the bounding rect: the entrance scale is still on.
    const top = (el.offsetParent?.getBoundingClientRect().top ?? 0) + el.offsetTop;
    const lift = Math.min(top + el.offsetHeight - (window.innerHeight - 12), top - 8);
    if (lift > 0) el.style.top = `${-4 - lift}px`;
  }, []);

  const groups = useMemo(() => {
    // Every word must match, in any order: "outlet spo", "1.5 hp split".
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const items = words.length > 0 ? catalog.filter((c) => words.every((w) => searchText(c).includes(w))) : catalog;
    const map = new Map<AssetCategory, CatalogItem[]>();
    for (const item of items) map.set(item.category, [...(map.get(item.category) ?? []), item]);
    const rank = (c: AssetCategory) => (CATEGORY_ORDER.indexOf(c) + CATEGORY_ORDER.length + 1) % (CATEGORY_ORDER.length + 1);
    return [...map.entries()].sort(([a], [b]) => rank(a) - rank(b));
  }, [catalog, query]);

  const unit = settings?.display_unit ?? "mm";

  return (
    <div
      ref={ref}
      className={cx(s.flyout, tool.flyout === "asset" && s.flyoutWide, tool.flyout === "pipe" && s.flyoutPipe)}
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

      {tool.flyout === "pipe" ? <PipeOptions /> : null}

      {tool.flyout === "asset" ? (
        <>
          <label className={s.flyoutSearch}>
            <Icon name="search" size={14} />
            <input
              autoFocus
              type="text"
              value={query}
              placeholder="Find an object, a light, an outlet"
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
                      <small>{itemMeta(item, unit)}</small>
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
        <div key={t.tool} className={cx(s.railSlot, i > 0 && TOOLS[i - 1].group !== t.group && s.railGap)}>
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
