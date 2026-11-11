// The gallery label for how a model view image was made (`RenderRecord::info`).
// A record without info shows as a plain capture: never claiming more than
// we know.

import type { RenderInfo, VideoInfo } from "../../contract/bindings";

/** "Render", "Enhanced capture", "Shadow study", "Video" or "Capture". */
export function kindLabel(info: RenderInfo | null): string {
  switch (info?.kind) {
    case "path_traced":
      return "Render";
    case "enhanced":
      return "Enhanced capture";
    case "shadow_study":
      return "Shadow study";
    case "video":
      return "Video";
    default:
      return "Capture";
  }
}

/** What Visuals says when this version of the app has no AI rendering. */
export const NO_AI_RENDER = "AI rendering is not in this version yet. Captures and comparing still work.";

/**
 * "Nov 11, 10:42 AM": when a visual was saved. Lists and captions name a
 * capture by its view and this time, never by the model's revision number.
 */
export function savedLabel(rfc3339: string): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/**
 * A capture's name in the Source picker, short enough for its column: the
 * view, then "5:21 AM" for today or "Nov 11, 5:21 AM" for another day.
 */
export function captureLabel(viewName: string, rfc3339: string, now: Date = new Date()): string {
  const name = viewName || "View";
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return name;
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const sameDay = d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth() && d.getDate() === now.getDate();
  return sameDay ? `${name}, ${time}` : `${name}, ${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}, ${time}`;
}

/** "0:30" or "1:05:09", a video's length. */
export function lengthLabel(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const rest = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${rest}` : `${m}:${rest}`;
}

/** "Video 0:30, 1920 x 1080", or "Frames 0:30, ..." for a folder of PNG frames. */
export function videoLabel(v: VideoInfo): string {
  return `${v.codec === "png" ? "Frames" : "Video"} ${lengthLabel(v.seconds)}, ${v.width} x ${v.height}`;
}
