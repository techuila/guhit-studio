// Inline icon set. Thin geometric linework on a 20 x 20 grid, 1.5 px strokes.
import type { CSSProperties } from "react";

const PATHS = {
  select: "M5 3.2l9.8 6.6-4.4 1L8 15.4z",
  pan: "M10 2.5v15M2.5 10h15M10 2.5L8 4.5M10 2.5l2 2M10 17.5l-2-2M10 17.5l2-2M2.5 10l2-2M2.5 10l2 2M17.5 10l-2-2M17.5 10l-2 2",
  wall: "M2.5 6.5h15v7h-15zM5.5 13.5l5-7M10.5 13.5l5-7",
  room: "M3 3h14v14H3zM5.5 5.5h9v9h-9z",
  door: "M2 15.5h3.5M16.5 15.5H18M5.5 15.5v-11M5.5 4.5a11 11 0 0 1 11 11",
  window: "M2 10h3M15 10h3M5 7h10v6H5zM5 10h10",
  column: "M5 5h10v10H5zM5 5l10 10M15 5L5 15",
  stair: "M3 16.5h3.5V13H10V9.5h3.5V6H17",
  asset: "M5 9V6.5A1.5 1.5 0 0 1 6.5 5h7A1.5 1.5 0 0 1 15 6.5V9M3 9.5h14v5H3zM5 14.5v2M15 14.5v2",
  dimension: "M3.5 5v10M16.5 5v10M3.5 10h13M2.3 11.2l2.4-2.4M15.3 11.2l2.4-2.4",
  text: "M4.5 6V4.5h11V6M10 4.5v11M8 15.5h4",
  camera: "M2.5 6.5h10v7h-10zM12.5 9l5-2.2v6.4l-5-2.2",
  undo: "M7 4.5L3.5 8 7 11.5M3.5 8h8.2a4.3 4.3 0 0 1 0 8.6H9",
  redo: "M13 4.5L16.5 8 13 11.5M16.5 8H8.3a4.3 4.3 0 0 0 0 8.6H11",
  back: "M12 4.5L6.5 10l5.5 5.5",
  chevronDown: "M5.5 8l4.5 4.5L14.5 8",
  chevronRight: "M8 5.5l4.5 4.5L8 14.5",
  chevronUp: "M5.5 12L10 7.5l4.5 4.5",
  export: "M10 12.5V3M6.5 6.5L10 3l3.5 3.5M4 12v4.5h12V12",
  versions: "M10 6v4l2.8 1.8M3.2 10a6.8 6.8 0 1 0 2-4.8M3 3.5v2.2h2.2",
  check: "M4.5 10.5l3.5 3.5 7.5-8",
  close: "M5 5l10 10M15 5L5 15",
  plus: "M10 4v12M4 10h12",
  more: "M4.2 10a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M9.2 10a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M14.2 10a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0",
  eye: "M2.5 10s2.8-5 7.5-5 7.5 5 7.5 5-2.8 5-7.5 5-7.5-5-7.5-5zM10 12.2a2.2 2.2 0 1 0 0-4.4 2.2 2.2 0 0 0 0 4.4z",
  eyeOff: "M3.5 3.5l13 13M8.2 5.3A7.6 7.6 0 0 1 10 5c4.7 0 7.5 5 7.5 5a12.6 12.6 0 0 1-2.2 2.8M12.6 14.5A7.3 7.3 0 0 1 10 15c-4.7 0-7.5-5-7.5-5a12.5 12.5 0 0 1 3-3.4",
  lock: "M5 9h10v7.5H5zM7 9V6.5a3 3 0 0 1 6 0V9",
  unlock: "M5 9h10v7.5H5zM7 9V6.5a3 3 0 0 1 5.6-1.4",
  search: "M8.8 14.1a5.3 5.3 0 1 0 0-10.6 5.3 5.3 0 0 0 0 10.6zM12.7 12.7l4.3 4.3",
  trash: "M4 6h12M8 6V4h4v2M5.5 6l.7 10.5h7.6L14.5 6M8.5 9v4.5M11.5 9v4.5",
  copy: "M7 7h9.5v9.5H7zM13 7V3.5H3.5V13H7",
  pencil: "M4 16l.8-3.3 8.7-8.7 2.5 2.5-8.7 8.7zM11.8 5.7l2.5 2.5",
  grid: "M3 3h14v14H3zM3 7.7h14M3 12.3h14M7.7 3v14M12.3 3v14",
  snap: "M5 3.5v6.5a5 5 0 0 0 10 0V3.5M5 6.5h3M12 6.5h3M8 3.5v6.5a2 2 0 0 0 4 0V3.5",
  ortho: "M4 3.5v12.5h12.5M4 12h4v4",
  fit: "M3.5 7V3.5H7M13 3.5h3.5V7M16.5 13v3.5H13M7 16.5H3.5V13M7 7h6v6H7z",
  view2d: "M3 4h14v12H3zM3 9.5h7.5V16M10.5 4v5.5",
  view3d: "M10 2.8l6.5 3.6v7.2L10 17.2l-6.5-3.6V6.4zM3.5 6.4L10 10l6.5-3.6M10 10v7.2",
  split: "M3 4h14v12H3zM10 4v12",
  copilot: "M3.5 4.5h13v9h-7l-3.5 3v-3h-2.5zM6.5 8h7M6.5 10.8h4.5",
  visuals: "M3 4h14v12H3zM3 13l4-4 3.5 3.5 2.5-2.5 4 4M13.2 7.8h.01",
  layers: "M10 3l7.5 4L10 11 2.5 7zM2.5 10.5L10 14.5l7.5-4M2.5 13.5L10 17.5l7.5-4",
  info: "M10 17a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM10 9.2v4.3M10 6.6h.01",
  warning: "M10 3.2l7.3 13H2.7zM10 8.2v4M10 14.3h.01",
  north: "M10 2.5l4.5 14.5L10 14l-4.5 3z",
  panelRight: "M3 4h14v12H3zM12.5 4v12",
  home: "M3 9.5L10 3.5l7 6M5 8.2V16.5h10V8.2M8.5 16.5v-4.5h3v4.5",
  roof: "M2.5 11L10 4.5 17.5 11M4.5 9.5V16h11V9.5",
  level: "M3 15.5h14M3 10.5h14M3 5.5h14M6 15.5v-5M14 10.5v-5",
  folder: "M3 5h5l1.5 2H17v9H3z",
  keyboard: "M2.5 5.5h15v9h-15zM5.5 8.5h.01M8.5 8.5h.01M11.5 8.5h.01M14.5 8.5h.01M6.5 11.7h7",
  flip: "M10 3v14M7.5 6L3.5 10l4 4zM12.5 6l4 4-4 4z",
  rotate: "M16 6.5A6.5 6.5 0 1 0 17 10.8M16.8 3v4.2h-4.2",
  linework: "M3 14.5l3.5-7 3.5 5 6-8.5",
  model: "M10 2.5l7 4v7l-7 4-7-4v-7zM3 6.5l7 4 7-4M10 10.5v7",
  import: "M10 3v9.5M6.5 9L10 12.5 13.5 9M4 16.5h12",
  settings: "M10 12.7a2.7 2.7 0 1 0 0-5.4 2.7 2.7 0 0 0 0 5.4zM10 3v2.1M10 14.9V17M17 10h-2.1M5.1 10H3M15.1 4.9l-1.5 1.5M6.4 13.6l-1.5 1.5M15.1 15.1l-1.5-1.5M6.4 6.4L4.9 4.9",
  /** A pipe run with a 90 degree bend and a flange at each end. */
  pipe: "M3 5.5h7.5a6 6 0 0 1 6 6v5.5M3 10h7.5a1.5 1.5 0 0 1 1.5 1.5v5.5M3 4v7.5M10.5 17h7.5",
  walk: "M12.9 3.7a1.6 1.6 0 1 1-3.2 0 1.6 1.6 0 0 1 3.2 0zM10.8 6.9l-1.3 5-2.4 5.6M9.5 11.9l2.6 2.2.9 3.4M6.6 10.2l1.9-2.8 2.3-.5 2.2 2.6 2.3.7",
  fly: "M17.5 3L2.5 9l7 2.6 3.9 5.4zM9.5 11.6L17.5 3",
  /** A box with its hidden edges dashed: the shell drawn see-through. */
  xray: "M10 2.8l6.5 3.6v7.2L10 17.2l-6.5-3.6V6.4zM3.5 6.4L10 10l6.5-3.6M10 10v7.2M10 7.4V5.6M7.6 11.3l-1.6.9M12.4 11.3l1.6.9",
  takeoff: "M5 3.5h10v13H5zM7.5 7h5M7.5 10h5M7.5 13h2.5",
  /** Two chain links: a switch linked to its lights. */
  link: "M8.2 11.8l3.6-3.6M9.4 6.9l1.3-1.3a3 3 0 0 1 4.2 4.2l-1.3 1.3M10.6 13.1l-1.3 1.3a3 3 0 0 1-4.2-4.2l1.3-1.3",
  sun: "M10 13.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4zM10 2.5v1.8M10 15.7v1.8M2.5 10h1.8M15.7 10h1.8M4.7 4.7l1.3 1.3M14 14l1.3 1.3M15.3 4.7L14 6M6 14l-1.3 1.3",
  moon: "M15.8 12.3A6.6 6.6 0 0 1 7.7 4.2a6.6 6.6 0 1 0 8.1 8.1z",
  /** The day's arc over the horizon with the sun on it. */
  sunPath: "M2.5 16h15M4.5 16a5.5 5.5 0 0 1 11 0M13.9 9.4a1.6 1.6 0 1 0-.01 0z",
  /** A lens aperture: the Render button. */
  render: "M10 17a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM10 3l2.4 5.2M17 10l-5.7.5M13.5 16.1l-3.3-4.7M6.5 16.1l2.4-5.2M3 10l5.7-.5M6.5 3.9l3.3 4.7",
  /** Crosshair: refine the resting view. */
  refine: "M10 3v2.2M10 14.8V17M3 10h2.2M14.8 10H17M10 13.3a3.3 3.3 0 1 0 0-6.6 3.3 3.3 0 0 0 0 6.6z",
  clock: "M10 17a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM10 6.2V10l2.8 1.8",
  bulb: "M7.8 14.2h4.4M8.3 16.7h3.4M10 3a4.8 4.8 0 0 0-2.9 8.6c.6.5.9 1.1.9 1.9v.7h4v-.7c0-.8.3-1.4.9-1.9A4.8 4.8 0 0 0 10 3z",
  plug: "M7.2 3v4M12.8 3v4M5 7h10v2.8a5 5 0 0 1-10 0zM10 14.8v2.7",
  /** A split aircon indoor unit blowing air down. */
  aircon: "M2.5 4.5h15v6h-15zM5 8h10M6.5 13.5c0 1.2-1 1.6-1 2.8M10 13.5c0 1.2-1 1.6-1 2.8M13.5 13.5c0 1.2-1 1.6-1 2.8",
  /** A table: schedules. */
  schedule: "M3 4h14v12H3zM3 8h14M3 12h14M8 4v12",
  /** A map pin: the site. */
  pin: "M10 17.5s5-4.6 5-8.7a5 5 0 0 0-10 0c0 4.1 5 8.7 5 8.7zM10 10.6a1.8 1.8 0 1 0 0-3.6 1.8 1.8 0 0 0 0 3.6z",
  /** A box with a lid: set aside. */
  aside: "M3 4.5h14v3H3zM4.5 7.5v8h11v-8M8 10.5h4",
  reopen: "M7 4.5L3.5 8 7 11.5M3.5 8h8.2a4.3 4.3 0 0 1 0 8.6H9",
  resolved: "M10 17a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM6.8 10.2l2.2 2.2 4.2-4.6",
  /** Two people: a live session, Share. */
  people: "M7.5 9.3a2.7 2.7 0 1 0 0-5.4 2.7 2.7 0 0 0 0 5.4zM2.5 16.5c0-2.9 2.2-4.9 5-4.9s5 2 5 4.9M13.2 9.1a2.4 2.4 0 1 0-.6-4.7M14.5 11.8c1.8.5 3 2.2 3 4.7",
  /** Two speech bubbles: the live session chat. */
  chat: "M2.5 3.5h10v7H6.5l-3 2.5v-2.5h-1zM12.5 7h5v7h-1v2.5l-3-2.5H8.5v-3.5",
  /** An arrow going in: join a live session. */
  join: "M11.5 3.5h4v13h-4M3 10h9M9 6.8l3.2 3.2L9 13.2",
  /** An arrow up: send. */
  send: "M10 16V4.5M5.5 9L10 4.5 14.5 9",
  /** The plan pointer: show on the plan. */
  pointer: "M5 3.2l9.8 6.6-4.4 1L8 15.4z",
} as const;

export type IconName = keyof typeof PATHS;

interface IconProps {
  name: IconName;
  size?: number;
  className?: string;
  style?: CSSProperties;
  title?: string;
}

export function Icon({ name, size = 18, className, style, title }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={{ flex: "none", display: "block", ...style }}
      aria-hidden={title ? undefined : true}
      role={title ? "img" : undefined}
    >
      {title ? <title>{title}</title> : null}
      <path d={PATHS[name]} />
    </svg>
  );
}

/** The GUHIT mark (assets/brand/logo-mark.svg): a plan frame with a door swing forming a G. */
export function BrandMark({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="200 200 624 624" fill="none" aria-hidden style={{ flex: "none", display: "block" }}>
      <g stroke="currentColor" strokeWidth={34} strokeLinecap="square">
        <path d="M220 220H740V290" />
        <path d="M220 220V804H804V540" />
        <path d="M220 540H500M500 540V804" />
        <path d="M804 540H640" />
        <path d="M660 380V470" />
      </g>
      <path d="M500 540A160 160 0 0 1 660 380" stroke="#8fc9cf" strokeWidth={34} strokeLinecap="square" />
    </svg>
  );
}
