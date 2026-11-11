import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { AssetCategory, CatalogItem } from "../contract/bindings";
import { useApp } from "../state/store";
import { cx } from "../ui/controls";
import { isModalOpen } from "../ui/Dialog";
import { Icon } from "../ui/icons";
import { Presence, useSlidingIndicator } from "../ui/motionDom";
import { formatLength } from "../ui/units";
import type { DisplayUnit } from "../contract/bindings";
import { DOOR_STYLES, SLAB_KINDS, TOOLS, WINDOW_STYLES, activateTool, type ToolDef } from "./actions";
import { DEVICE_LABEL, hpLabel, mountRef, mountingHeight } from "./devices";
import { StairToolOptions } from "../editor2d/StairToolOptions";
import { PipeOptions } from "./PipeOptions";
import { STAYS_OPEN, WALL_PRESETS, WALL_ROLES, railClick } from "./railFlyout";
import { useShell } from "./shellStore";
import { useSwarmLocked } from "../swarm/swarmStore";
import { LOCK_HINT } from "../swarm/view";
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
  site: "Site",
};

/** Library order: rooms first, then the services, then the site. */
const CATEGORY_ORDER: AssetCategory[] = ["furniture", "sanitary", "kitchen", "appliance", "lighting", "electrical", "aircon", "utility", "plant", "vehicle", "site"];

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

function Flyout({ tool, onClose, stage }: { tool: ToolDef; onClose: () => void; stage: "enter" | "idle" | "exit" }) {
  const ref = useRef<HTMLDivElement>(null);
  const toolOptions = useApp((st) => st.toolOptions);
  const setTool = useApp((st) => st.setTool);
  const catalog = useApp((st) => st.catalog);
  const loadCatalog = useApp((st) => st.loadCatalog);
  const settings = useApp((st) => st.doc?.project.settings);
  const [query, setQuery] = useState("");

  // An outside press closes it (the wall flyout stays open after a pick, so
  // this and Esc, handled by the rail, are its way out).
  useEffect(() => {
    if (stage === "exit") return;
    const down = (e: MouseEvent) => {
      const anchor = ref.current?.parentElement;
      if (anchor && !anchor.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", down, true);
    return () => window.removeEventListener("mousedown", down, true);
  }, [onClose, stage]);

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
  // The wall flyout stays open after a pick, so thickness and type take one trip.
  const picked = () => {
    if (!tool.flyout || !STAYS_OPEN.has(tool.flyout)) onClose();
  };

  return (
    <div
      ref={ref}
      className={cx(s.flyout, tool.flyout === "asset" && s.flyoutWide, tool.flyout === "pipe" && s.flyoutPipe)}
      data-stage={stage}
      role="dialog"
      aria-label={`${tool.label} options`}
    >
      {tool.flyout === "wall" ? (
        <>
          <div className={s.flyoutTitle}>Wall thickness</div>
          {WALL_PRESETS.map((p) => (
            <button
              key={String(p.mm)}
              type="button"
              aria-pressed={toolOptions.wallThicknessMm === p.mm}
              className={cx(s.flyoutItem, toolOptions.wallThicknessMm === p.mm && s.flyoutItemOn)}
              onClick={() => {
                setTool("wall", { wallThicknessMm: p.mm });
                picked();
              }}
            >
              <span>{p.mm === null && settings ? `Project default, ${formatLength(settings.default_wall_thickness_mm, unit)}` : p.label}</span>
              {p.hint ? <small>{p.hint}</small> : null}
            </button>
          ))}
          <div className={s.flyoutTitle}>Wall type</div>
          {WALL_ROLES.map((r) => (
            <button
              key={r.role}
              type="button"
              aria-pressed={toolOptions.wallRole === r.role}
              className={cx(s.flyoutItem, toolOptions.wallRole === r.role && s.flyoutItemOn)}
              onClick={() => {
                setTool("wall", { wallRole: r.role });
                picked();
              }}
            >
              <span>{r.label}</span>
              <small>{r.hint}</small>
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
              aria-pressed={toolOptions.openingStyle === o.value}
              className={cx(s.flyoutItem, toolOptions.openingStyle === o.value && s.flyoutItemOn)}
              onClick={() => {
                setTool(tool.tool, { openingStyle: o.value });
                picked();
              }}
            >
              <span>{o.label}</span>
            </button>
          ))}
        </>
      ) : null}

      {tool.flyout === "slab" ? (
        <>
          <div className={s.flyoutTitle}>Slab type</div>
          {SLAB_KINDS.map((k) => (
            <button
              key={k.value}
              type="button"
              aria-pressed={toolOptions.slabKind === k.value}
              className={cx(s.flyoutItem, toolOptions.slabKind === k.value && s.flyoutItemOn)}
              onClick={() => {
                setTool("slab", { slabKind: k.value });
                picked();
              }}
            >
              <span>{k.label}</span>
              <small>{k.hint}</small>
            </button>
          ))}
          <div className={s.flyoutTitle}>Railing</div>
          {[true, false].map((on) => (
            <button
              key={String(on)}
              type="button"
              aria-pressed={toolOptions.slabRailing === on}
              className={cx(s.flyoutItem, toolOptions.slabRailing === on && s.flyoutItemOn)}
              onClick={() => {
                setTool("slab", { slabRailing: on });
                picked();
              }}
            >
              <span>{on ? "Railing on" : "No railing"}</span>
              <small>{on ? "1000 mm bars" : "add one later"}</small>
            </button>
          ))}
        </>
      ) : null}

      {tool.flyout === "stair" ? <StairToolOptions onPicked={picked} /> : null}

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
                        picked();
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

/**
 * Whether an open flyout takes this Esc: pressed in the flyout or the rail,
 * on what opened it, or on the plan (or nowhere) with no field focused.
 * Anywhere else (the copilot's composer, a popover, a field) it is theirs.
 */
export function flyoutOwnsEscape(target: EventTarget | null, rail: Pick<Element, "contains"> | null, opener: Pick<Element, "contains"> | null): boolean {
  const el = target as (Pick<Element, "closest" | "tagName"> & { isContentEditable?: boolean }) | null;
  // The window, the document or the body: nothing has focus.
  if (!el || typeof el.closest !== "function" || el.tagName === "BODY" || el.tagName === "HTML") return true;
  if (rail?.contains(el as unknown as Node) || opener?.contains(el as unknown as Node)) return true;
  const typing = !!el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName);
  return !typing && el.closest("[data-plan-area]") !== null;
}

export function ToolRail() {
  const tool = useApp((st) => st.tool);
  const assetKey = useApp((st) => st.toolOptions.assetKey);
  const openPalette = useShell((st) => st.open);
  const flyoutRequest = useShell((st) => st.flyoutRequest);
  const [flyout, setFlyout] = useState<ToolDef["flyout"] | null>(null);
  // After Esc closes a flyout, focus goes back to its button: its tooltip
  // stays hidden until the pointer moves or focus leaves, so it does not pin
  // itself over the plan.
  const [tipOff, setTipOff] = useState<ToolDef["tool"] | null>(null);
  const railRef = useRef<HTMLElement>(null);
  const activeButtonRef = useRef<HTMLButtonElement>(null);
  const pill = useSlidingIndicator(railRef, activeButtonRef, [tool]);
  // A swarm draft is open (D57): only looking and selecting stay on.
  const swarmLocked = useSwarmLocked();
  const setTool = useApp((st) => st.setTool);
  const lockedOut = (t: ToolDef) => swarmLocked && t.tool !== "select" && t.tool !== "pan";
  useEffect(() => {
    if (swarmLocked && tool !== "select" && tool !== "pan") setTool("select");
    // Only when the lock comes on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [swarmLocked]);

  // Leaving a tool by any route (shortcut, Escape) closes its flyout.
  useEffect(() => {
    setFlyout((f) => (f && TOOLS.find((t) => t.flyout === f)?.tool !== tool ? null : f));
  }, [tool]);

  // What opened the flyout (its rail button, or the status bar's style
  // button): Esc gives focus back to it.
  const opener = useRef<HTMLElement | null>(null);

  // A keyboard shortcut (O for objects) or the status bar asking to open a
  // flyout it cannot reach the local state for directly.
  useEffect(() => {
    if (!flyoutRequest) return;
    const from = document.activeElement;
    opener.current = from instanceof HTMLElement && from !== document.body ? from : null;
    setFlyout(flyoutRequest.flyout);
  }, [flyoutRequest]);

  // The first click activates the tool; a click on the active tool or its
  // corner mark opens its options (railFlyout.ts).
  const click = (t: ToolDef, button: HTMLElement, corner: boolean) => {
    const next = railClick(t, { tool, open: flyout ?? null, assetKey }, corner);
    if (next.activate) activateTool(t.tool);
    if (next.flyout) opener.current = button;
    setFlyout(next.flyout);
  };

  // Esc closes an open flyout first and keeps the tool; the next Esc goes
  // back to Select. Registered when the rail mounts, so it runs before the
  // plan's own Esc (which returns to Select), registered later by PlanCanvas.
  // It takes Esc only from the flyout, the rail, what opened it, or the plan
  // with nothing typed in: the composer, a popover or a field keep theirs.
  const open = useRef(flyout);
  open.current = flyout;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const def = open.current ? TOOLS.find((x) => x.flyout === open.current) : undefined;
      if (e.key !== "Escape" || !def || e.defaultPrevented || isModalOpen()) return;
      if (!flyoutOwnsEscape(e.target, railRef.current, opener.current)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      setFlyout(null);
      const railButton = railRef.current?.querySelector<HTMLButtonElement>(`button[aria-label="${CSS.escape(def.label)}"]`) ?? null;
      const back = opener.current?.isConnected ? opener.current : railButton;
      if (back === railButton) setTipOff(def.tool);
      back?.focus({ preventScroll: true });
      opener.current = null;
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, []);

  return (
    <nav ref={railRef} className={s.rail} aria-label="Drawing tools" onPointerMove={tipOff ? () => setTipOff(null) : undefined}>
      {pill.visible ? <span aria-hidden className={cx(s.railPill, pill.instant && s.instant)} style={pill.style} /> : null}
      {TOOLS.map((t, i) => (
        <div key={t.tool} className={cx(s.railSlot, i > 0 && TOOLS[i - 1].group !== t.group && s.railGap)}>
          <button
            ref={tool === t.tool ? activeButtonRef : undefined}
            type="button"
            className={cx(s.railButton, tool === t.tool && s.railButtonOn)}
            aria-label={t.label}
            aria-pressed={tool === t.tool}
            aria-haspopup={t.flyout ? "dialog" : undefined}
            aria-expanded={t.flyout ? flyout === t.flyout : undefined}
            disabled={lockedOut(t)}
            data-tip={lockedOut(t) ? LOCK_HINT : t.key ? `${t.label} (${t.key})` : t.label}
            data-tip-side="right"
            data-tip-off={(flyout === t.flyout && t.flyout) || tipOff === t.tool ? "true" : undefined}
            onBlur={tipOff === t.tool ? () => setTipOff(null) : undefined}
            onClick={(e) => click(t, e.currentTarget, (e.target as HTMLElement).closest("[data-rail-corner]") !== null)}
          >
            <Icon name={t.icon} size={20} />
            {t.flyout ? <span className={s.railCorner} data-rail-corner aria-hidden /> : null}
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
