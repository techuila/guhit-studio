// The gallery label for how a model view image was made (`RenderRecord::info`).
// A record without info shows as a plain capture: never claiming more than
// we know.

import type { RenderInfo } from "../../contract/bindings";

/** "Render", "Enhanced capture", "Shadow study" or "Capture". */
export function kindLabel(info: RenderInfo | null): string {
  switch (info?.kind) {
    case "path_traced":
      return "Render";
    case "enhanced":
      return "Enhanced capture";
    case "shadow_study":
      return "Shadow study";
    default:
      return "Capture";
  }
}
