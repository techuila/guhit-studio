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
