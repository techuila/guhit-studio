// Dev page: every swarm panel state side by side, and the draft bar in each
// of its states, from the statuses in ../mock.ts. No bridge needed. Open
// /src/swarm/dev/index.html on the Vite dev server; scripts/swarm-check.mjs
// screenshots it.
//   ?only=running   one state, full height (for a single screenshot)
import React from "react";
import ReactDOM from "react-dom/client";
import "../../styles/tokens.css";
import "../../ui/global.css";
import { DraftBar, LockBanner } from "../DraftBar";
import { panelStates } from "../mock";
import { reveal } from "../swarmStore";
import { SwarmPanelView } from "../SwarmPanel";
import type { SwarmCost, SwarmStatus } from "../types";
import { acceptAllAsk, acceptAsk, discardAsk } from "../view";

const params = new URLSearchParams(location.search);
const only = params.get("only");
const states = panelStates();

// A reveal in progress, so the rows show a revealed fill behind the landed one.
const t0 = performance.now();
reveal.known("shell", 4, []);
reveal.known("rooms", 2, []);
reveal.known("lead", 1, []);
reveal.arrive("shell", [{ id: "dev-w", part: "shell", kind: "line", from: { x: 0, y: 0 }, to: { x: 30_000, y: 0 }, lengthMm: 30_000, hosts: [], label: "Laying out the outer walls", levelId: null, removed: false }], t0);

const base = { note: null, ask: null, acting: null, brief: "", capUsd: null, estimate: null, speed: 1.25 } as const;

// The estimate on a priced model, and on one the price table does not know.
const priced: SwarmCost = { model: "claude-opus-5-5", model_label: "Claude Opus 5.5", low_usd: 0.76, high_usd: 2.42, spent_usd: 0, cap_usd: 3, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 };
const unpriced: SwarmCost = { ...priced, model: "claude-opus-5-5-20260401", model_label: "claude-opus-5-5-20260401", price_note: "Billed at the highest rate: price unknown", low_usd: 1.9, high_usd: 6.05 };

function Column({ name, status, brief, hasKey = true, estimate = null, children }: { name: string; status: SwarmStatus | null; brief?: string; hasKey?: boolean; estimate?: SwarmCost | null; children?: React.ReactNode }) {
  return (
    <figure data-state={name} style={{ margin: 0, display: "grid", gap: 6, alignContent: "start" }}>
      <figcaption style={{ font: "600 11px var(--font-mono)", color: "var(--ink-3)", textTransform: "uppercase", letterSpacing: "0.06em" }}>{name}</figcaption>
      <div style={{ width: 340, height: only ? "calc(100vh - 60px)" : 640, display: "flex", flexDirection: "column", background: "var(--surface)", border: "1px solid var(--line)", borderRadius: 8, overflow: "hidden" }}>
        {children ?? <SwarmPanelView {...base} status={status} brief={brief ?? ""} estimate={estimate} hasKey={hasKey} hasDoc />}
      </div>
    </figure>
  );
}

function Bars() {
  const running = states.running as SwarmStatus;
  const ready = states.ready as SwarmStatus;
  const conflicts = states.conflicts as SwarmStatus;
  const ask = acceptAsk(running, "rooms", ["shell", "rooms"]);
  const drop = discardAsk(running, "shell", ["rooms", "roof"]);
  const differ = (message: string) => ({ part: "rooms", step: 4, label: "Add room", code: "differs", message, element_ids: [] });
  const differs = acceptAllAsk(ready, [differ("Room 1 in the draft comes in as Room 3."), differ("Wall 4000 mm joins the Kitchen wall at its end, not its middle.")]);
  const skipped = acceptAsk(ready, "rooms", ["shell", "rooms"], [], [{ part: "rooms", step: 9, label: "Auto dimension", code: "nothing_to_dimension", message: "Nothing to dimension on this level.", element_ids: [] }]);
  const row = (name: string, el: React.ReactNode) => (
    <figure data-state={name} style={{ margin: 0, display: "grid", gap: 6, justifyItems: "center", padding: 16, background: "var(--paper)", border: "1px solid var(--line)", borderRadius: 8 }}>
      <figcaption style={{ font: "600 11px var(--font-mono)", color: "var(--ink-3)", textTransform: "uppercase", letterSpacing: "0.06em", justifySelf: "start" }}>{name}</figcaption>
      {el}
    </figure>
  );
  return (
    <div style={{ display: "grid", gap: 12, maxWidth: 760 }}>
      {row("bar-drawing", <><DraftBar status={running} showDraft acting={null} ask={null} /><LockBanner /></>)}
      {row("bar-ready", <DraftBar status={ready} showDraft acting={null} ask={null} />)}
      {row("bar-conflicts", <DraftBar status={conflicts} showDraft={false} acting={null} ask={null} />)}
      {row("bar-accept-asks", <DraftBar status={running} showDraft acting={null} ask={ask ? { ...ask, origin: "bar" } : null} />)}
      {row("bar-discard-asks", <DraftBar status={running} showDraft acting={null} ask={drop ? { ...drop, origin: "bar" } : null} />)}
      {row("bar-accept-differs", <DraftBar status={ready} showDraft acting={null} ask={differs ? { ...differs, origin: "bar" } : null} />)}
      {row("bar-accept-skipped", <DraftBar status={ready} showDraft acting={null} ask={skipped ? { ...skipped, origin: "bar" } : null} />)}
    </div>
  );
}

function Page() {
  if (only === "bars") return <div style={{ padding: 16 }}><Bars /></div>;
  const brief = "Two storey house for a 12 x 15 m lot in Quezon City: 3 bedrooms, 2.5 baths, open living and dining, carport, lanai.";
  const columns: Array<[string, SwarmStatus | null, { brief?: string; hasKey?: boolean; estimate?: SwarmCost }]> = [
    ["idle", null, { brief, estimate: priced }],
    ["idle-unpriced", null, { brief, estimate: unpriced }],
    ["no-key", null, { hasKey: false }],
    ["planning", states.planning, {}],
    ["running", states.running, {}],
    ["paused", states.paused, {}],
    ["waiting-ai", states.waiting_ai, {}],
    ["ready", states.ready, {}],
    ["mcp", states.mcp, {}],
    ["failed", states.failed, {}],
    ["accepted-one", states.accepted_one, {}],
  ];
  const shown = only ? columns.filter(([n]) => n === only) : columns;
  return (
    <div style={{ padding: 16, display: "grid", gap: 20, background: "var(--surface-2)", minHeight: "100vh", overflow: "auto" }}>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 16 }}>
        {shown.map(([name, status, more]) => (
          <Column key={name} name={name} status={status} {...more} />
        ))}
      </div>
      {only ? null : <Bars />}
    </div>
  );
}

document.body.style.overflow = "auto";
ReactDOM.createRoot(document.getElementById("root")!).render(<Page />);
