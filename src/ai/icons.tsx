// Small line icons for the copilot dock. Same 20 px grid and 1.5 stroke as
// the shell icon set, kept local so the dock has no dependency on it.
import type { Element } from "../contract/bindings";

function Svg({ d, size = 16 }: { d: string; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d={d} />
    </svg>
  );
}

/** Settings: two sliders. */
export const GearIcon = () => (
  <Svg d="M3.5 6.5h7.7M14.8 6.5h1.7M13 8.3a1.8 1.8 0 1 0 0-3.6 1.8 1.8 0 0 0 0 3.6zM3.5 13.5h1.7M8.8 13.5h7.7M7 15.3a1.8 1.8 0 1 0 0-3.6 1.8 1.8 0 0 0 0 3.6z" />
);
export const SendIcon = () => <Svg d="M10 16V4.5M5.5 9L10 4.5 14.5 9" />;
export const StopIcon = () => <Svg d="M6.5 6.5h7v7h-7z" />;
export const CloseIcon = () => <Svg d="M5 5l10 10M15 5L5 15" size={14} />;

/** Apply turns into this: the stroke is drawn, it does not just appear. */
export const DrawnCheck = ({ size = 16, className }: { size?: number; className?: string }) => (
  <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <path d="M4.5 10.5l3.6 3.6L15.5 6.5" pathLength={1} className={className} />
  </svg>
);

const KIND_PATH: Record<Element["kind"], string> = {
  wall: "M2.5 7.5h15v5h-15z",
  opening: "M2.5 16H6M14 16h3.5M6 16V8M6 8a8 8 0 0 1 8 8",
  room: "M3 3h14v14H3zM5.5 5.5h9v9h-9z",
  column: "M5 5h10v10H5z",
  stair: "M3 16.5h3.5V13H10V9.5h3.5V6H17",
  asset: "M5 9V6.5A1.5 1.5 0 0 1 6.5 5h7A1.5 1.5 0 0 1 15 6.5V9M3 9.5h14v5H3z",
  annotation: "M4.5 6V4.5h11V6M10 4.5v11M8 15.5h4",
  dimension: "M3.5 5v10M16.5 5v10M3.5 10h13",
  camera: "M2.5 6.5h10v7h-10zM12.5 9l5-2.2v6.4l-5-2.2",
  underlay: "M3 4h14v12H3zM3 13l4-4 3.5 3.5 2.5-2.5 4 4",
  linework: "M3 14.5l3.5-7 3.5 5 6-8.5",
  reference_model: "M10 2.5l7 4v7l-7 4-7-4v-7zM3 6.5l7 4 7-4M10 10.5v7",
  pipe: "M3 7h7.5A3.5 3.5 0 0 1 14 10.5V17M3 11h7v6",
};

export const KindIcon = ({ kind }: { kind: Element["kind"] }) => <Svg d={KIND_PATH[kind]} size={14} />;
