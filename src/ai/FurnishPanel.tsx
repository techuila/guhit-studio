// "Furnish rooms" in the copilot dock (lane L5): the bar that starts it and
// the room cards of its answer, one proposal per room (D8). Logic in
// furnish.ts; the engine places everything (guhit_core::placement).
import { useLayoutEffect, useRef, useState } from "react";
import type { AiProposal } from "../contract/bindings";
import { useApp } from "../state/store";
import { growIn } from "../ui/motionWaapi";
import type { ChatItem, FurnishRoom } from "./copilotStore";
import { FURNISH_STYLES, applyRoom, discardRoom, previewRoom, roomsToFurnish } from "./furnish";
import { DrawnCheck } from "./icons";
import s from "./Furnish.module.css";

function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

/** The bar above the composer: style pills and the Furnish button. `tip`: why it is off (the swarm lock). */
export function FurnishBar({ disabled, tip = null, onFurnish }: { disabled: boolean; tip?: string | null; onFurnish: (style: string) => void }) {
  const doc = useApp((st) => st.doc);
  const selection = useApp((st) => st.selection);
  const activeLevelId = useApp((st) => st.activeLevelId);
  const [style, setStyle] = useState<string>("modern");
  const rooms = roomsToFurnish(doc, selection, activeLevelId);
  const selected = rooms.some((r) => selection.includes(r.id));
  const label = rooms.length === 0 ? "No closed rooms" : selected ? `Furnish ${rooms.length === 1 ? rooms[0].name : `${rooms.length} selected rooms`}` : `Furnish ${rooms.length} ${rooms.length === 1 ? "room" : "rooms"}`;
  return (
    <div className={s.bar} data-testid="furnish-bar">
      <div className={s.styles} role="radiogroup" aria-label="Furnishing style">
        {FURNISH_STYLES.map((st) => (
          <button
            key={st.key}
            type="button"
            role="radio"
            aria-checked={style === st.key}
            className={cx(s.style, style === st.key && s.styleOn)}
            onClick={() => setStyle(st.key)}
          >
            {st.label}
          </button>
        ))}
      </div>
      <button type="button" className={s.go} disabled={disabled || rooms.length === 0} onClick={() => onFurnish(style)} data-tip={tip ?? undefined} data-testid="furnish-go">
        {label}
      </button>
    </div>
  );
}

/** Counts of what a room's proposal adds, by name: "Sofa, leather", "2 x Nightstand". */
function addedNames(p: AiProposal): string[] {
  const ids = new Set(p.preview.diff.added);
  const counts = new Map<string, number>();
  for (const e of p.preview.state.project.elements) {
    if (!ids.has(e.id) || e.kind !== "asset") continue;
    counts.set(e.name, (counts.get(e.name) ?? 0) + 1);
  }
  return [...counts.entries()].map(([name, n]) => (n > 1 ? `${n} x ${name}` : name));
}

const STATE_LABEL: Record<string, string> = {
  pending: "Preview on the plan",
  applying: "Applying",
  applied: "Applied",
  discarded: "Discarded",
  stale: "Out of date",
};

function RoomCard({ item, room, active, onPick }: { item: ChatItem; room: FurnishRoom; active: boolean; onPick: (room: FurnishRoom) => void }) {
  const ref = useRef<HTMLElement>(null);
  const live = room.status === "pending" || room.status === "applying";
  const names = addedNames(room.proposal);
  useLayoutEffect(() => {
    growIn(ref.current, "panel");
  }, []);
  return (
    <section
      ref={ref}
      className={cx(s.room, active && live && s.roomActive, !live && s.roomDone)}
      aria-label={`Furnishing of ${room.roomName}`}
      onMouseEnter={() => live && onPick(room)}
      onFocus={() => live && onPick(room)}
      data-testid="furnish-room"
    >
      <header className={s.roomHead}>
        <span className={s.roomName}>{room.roomName}</span>
        <span key={room.status} className={s.roomState} data-state={room.status}>
          {STATE_LABEL[room.status]}
        </span>
      </header>
      <p className={s.items}>{names.length > 0 ? names.join(", ") : "Nothing new"}</p>
      {live ? (
        <div className={s.actions}>
          <button
            type="button"
            className={cx(s.apply, room.status === "applying" && s.applyBusy)}
            disabled={room.status === "applying"}
            onClick={() => void applyRoom(item.id, room.roomId, item.furnishStyle ?? "modern")}
            data-testid="furnish-apply"
          >
            {room.status === "applying" ? "Applying" : "Apply"}
          </button>
          <button type="button" className={s.discard} disabled={room.status === "applying"} onClick={() => discardRoom(item.id, room.roomId)}>
            Discard
          </button>
          <span className={s.count}>{room.proposal.preview.diff.added.length} objects, one undo step</span>
        </div>
      ) : (
        <p className={s.note} data-state={room.status}>
          {room.status === "applied" ? <DrawnCheck className={s.check} /> : null}
          {room.note}
        </p>
      )}
      {live && room.note ? <p className={s.note}>{room.note}</p> : null}
    </section>
  );
}

/**
 * The room cards of one "Furnish rooms" answer. Hovering a card shows its
 * room on the plan. Cards go by proposal id: two rooms may share a name.
 */
export function FurnishCards({ item }: { item: ChatItem }) {
  const rooms = item.furnish ?? [];
  const [active, setActive] = useState<string | null>(rooms.find((r) => r.status === "pending")?.proposal.id ?? null);
  const pick = (room: FurnishRoom) => {
    setActive(room.proposal.id);
    previewRoom(room);
  };
  return (
    <div className={s.cards} onMouseLeave={() => previewRoom(rooms.find((r) => r.proposal.id === active) ?? null)}>
      {rooms.map((r) => (
        <RoomCard key={r.proposal.id} item={item} room={r} active={active === r.proposal.id} onPick={pick} />
      ))}
    </div>
  );
}
