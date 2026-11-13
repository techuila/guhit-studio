// G4 problem 10: idle agents kept their labels on the plan and in 3D, and
// one covered the door tool's "Narrowed to 1450 mm to fit". An idle agent's
// label now folds away in both, and comes back when it works again; its mark
// stays, dimmed. PH titles keep the whole name: format.test.ts.

import { describe, expect, it } from "vitest";

// Read as text: vitest hands CSS modules over as class maps. The app has no
// Node types, so the tests' runtime module is loaded by name.
const fs: { readFileSync(path: URL, encoding: "utf8"): string } = await import(/* @vite-ignore */ ["node", "fs"].join(":"));
const plan = fs.readFileSync(new URL("./agents.module.css", import.meta.url), "utf8");
const model = fs.readFileSync(new URL("../viewer3d/live/LivePointers3D.module.css", import.meta.url), "utf8");

/** The declarations of the rule whose selector is exactly `selector`. */
function rule(css: string, selector: string): string | null {
  const at = css.indexOf(`${selector} {`);
  if (at < 0) return null;
  const open = css.indexOf("{", at);
  return css.slice(open + 1, css.indexOf("}", open));
}

describe("an idle agent's label", () => {
  it("folds away on the plan, with a motion token, and only for idle", () => {
    const idle = rule(plan, '.cursor[data-activity="idle"] .label');
    expect(idle).toMatch(/opacity:\s*0;/);
    expect(idle).toMatch(/transition-timing-function:\s*var\(--ease-in\)/);
    expect(rule(plan, ".label")).toMatch(/opacity var\(--dur-panel\)/);
    expect(rule(plan, '.cursor[data-activity="working"] .label')).toBeNull();
  });

  it("folds away in 3D", () => {
    expect(rule(model, '.marker[data-activity="idle"] .agentLabel')).toMatch(/opacity:\s*0;/);
  });

  it("keeps still under reduced motion, fading without the shrink", () => {
    const reduced = plan.slice(plan.indexOf("@media (prefers-reduced-motion: reduce)"));
    expect(rule(reduced, '  .cursor[data-activity="idle"] .label')).toMatch(/transform:\s*none;/);
  });
});
