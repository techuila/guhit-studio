// A project card is a real button with the menu beside it, not inside it
// (U45), and a project never opened shows a blank sheet, not a made-up plan
// (V22).
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ProjectMeta } from "../contract/bindings";
import { ProjectCard, type CardProps } from "./ProjectHub";

const project = {
  id: "p1",
  name: "Reyes Residence",
  thumbnail: null,
  floor_area_m2: 0,
  room_count: 0,
  updated_at: "2026-11-10T03:00:00Z",
} as unknown as ProjectMeta;

const noop = () => undefined;
const props = (more: Partial<CardProps> = {}): CardProps => ({
  project,
  index: 0,
  renaming: false,
  menuOpen: false,
  onOpen: noop,
  onMenu: noop,
  onStartRename: noop,
  onRename: noop,
  onCancelRename: noop,
  onDuplicate: noop,
  onDelete: noop,
  ...more,
});

describe("project card", () => {
  it("opens through a real button named for the project, with no button inside it", () => {
    const html = renderToStaticMarkup(<ProjectCard {...props()} />);
    expect(html).not.toContain('role="button"');
    const open = html.match(/<button[^>]*aria-label="Open Reyes Residence"[^>]*>([\s\S]*?)<\/button>/);
    expect(open).not.toBeNull();
    expect(open![1]).not.toContain("<button");
    // The actions menu is its own button, beside the open button.
    expect(html).toContain('aria-label="Actions for Reyes Residence"');
    expect(html).toContain('data-tip="Rename, duplicate, move to trash"');
  });

  it("shows the blank sheet for a project with no picture yet", () => {
    const html = renderToStaticMarkup(<ProjectCard {...props()} />);
    expect(html).toContain("g-blank");
    expect(html).not.toContain("<img");
  });

  it("shows the saved picture when there is one", () => {
    const html = renderToStaticMarkup(<ProjectCard {...props({ project: { ...project, thumbnail: "data:image/png;base64,AAAA" } })} />);
    expect(html).toContain('<img src="data:image/png;base64,AAAA"');
    expect(html).not.toContain("g-blank");
  });
});
