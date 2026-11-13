// The review list's look after the UI cleanup (D58): the triage keys live in
// a header tooltip, not a line under the list, and the trust disclaimer stays.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { DocState, Issue } from "../contract/bindings";
import fixture from "../../fixtures/sample-bungalow.docstate.json";
import { ReviewSection } from "./ReviewSection";

function docWith(issues: Issue[]): DocState {
  const d = structuredClone(fixture as unknown as DocState);
  d.derived = { ...d.derived, issues };
  return d;
}

function issue(d: DocState): Issue {
  const room = d.project.elements.find((e) => e.kind === "room")!;
  return { id: "i1", severity: "warning", code: "room_no_window", message: "The room has no window.", element_ids: [room.id], location: null, status: "open", note: "" };
}

describe("review section", () => {
  it("puts the triage keys in the header's keyboard tooltip, not under the list", () => {
    const base = docWith([]);
    const html = renderToStaticMarkup(<ReviewSection doc={docWith([issue(base)])} />);
    expect(html).toContain('data-testid="review-keys"');
    expect(html).toContain('data-tip="↑ ↓ move, S sets aside, O reopens, Enter shows"');
    expect(html).not.toContain("With the list focused");
    expect(html).not.toContain("<kbd>");
    // The list still names the keys for screen readers.
    expect(html).toContain('aria-label="Review items. ↑ ↓ move, S sets aside, O reopens, Enter shows."');
  });

  it("shows no keys tooltip when there is nothing to triage", () => {
    const html = renderToStaticMarkup(<ReviewSection doc={docWith([])} />);
    expect(html).not.toContain('data-testid="review-keys"');
    expect(html).toContain("Nothing to flag right now.");
  });

  it("keeps the trust disclaimer visible (R9)", () => {
    const html = renderToStaticMarkup(<ReviewSection doc={docWith([])} />);
    expect(html).toContain("This is not a permit, structural, plumbing, electrical or building code review.");
  });
});
