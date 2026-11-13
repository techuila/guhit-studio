// The agents' and live session's look and motion after the UI cleanup
// (R4 to R6; MOTION.md rules 4, 8, 11 and 12): no sparkle, nothing loops,
// one ring per new step on the plan cursor, no stagger, ease-out pops, purple
// only on the "AI" author tag, and no colored side rules.
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AgentAvatar } from "./AgentAvatar";
import { AgentCursorBody } from "./AgentCursors";

/**
 * Reads a stylesheet as text. Vitest hands CSS modules over as class maps
 * (even with ?raw), so this reads the file; through getBuiltinModule, since
 * the app's tsconfig has no Node types.
 */
const fs = (globalThis as unknown as { process: { getBuiltinModule(id: "node:fs"): { readFileSync(path: URL, encoding: "utf8"): string } } }).process.getBuiltinModule("node:fs");
const read = (rel: string): string => fs.readFileSync(new URL(rel, import.meta.url), "utf8");

const agentsCss = read("./agents.module.css");
const panelCss = read("./AgentsPanel.module.css");
const buttonCss = read("./ChatButton.module.css");
const chatCss = read("./ChatPanel.module.css");
const peekCss = read("./ChatPeek.module.css");
const followCss = read("./FollowFrame.module.css");
const liveCss = read("./live.module.css");
const layerCss = read("./LiveLayer.module.css");
const pointersCss = read("../viewer3d/live/LivePointers3D.module.css");

const SHEETS: Record<string, string> = {
  "agents.module.css": agentsCss,
  "AgentsPanel.module.css": panelCss,
  "ChatButton.module.css": buttonCss,
  "ChatPanel.module.css": chatCss,
  "ChatPeek.module.css": peekCss,
  "FollowFrame.module.css": followCss,
  "live.module.css": liveCss,
  "LiveLayer.module.css": layerCss,
  "LivePointers3D.module.css": pointersCss,
};

/** Comments out, then every innermost rule as [selector, body]. */
function rules(css: string): Array<[string, string]> {
  const bare = css.replace(/\/\*[\s\S]*?\*\//g, "");
  return [...bare.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => [m[1].trim(), m[2]]);
}

/** The selectors inside the reduced-motion block. */
function reducedMotion(css: string): string {
  const bare = css.replace(/\/\*[\s\S]*?\*\//g, "");
  return [...bare.matchAll(/@media \(prefers-reduced-motion: reduce\) \{([\s\S]*?)\n\}/g)].map((m) => m[1]).join("\n");
}

/** The element with `data-ring`, found by walking a view's element tree. */
function ringOf(node: ReactNode): ReactElement | null {
  if (Array.isArray(node)) {
    for (const child of node) {
      const hit = ringOf(child);
      if (hit) return hit;
    }
    return null;
  }
  if (!isValidElement(node)) return null;
  const props = node.props as { children?: ReactNode; "data-ring"?: string };
  if (props["data-ring"] !== undefined) return node;
  return ringOf(props.children);
}

describe("agent cursor on the plan", () => {
  it("a working agent's diamond carries one ring, keyed by its status line", () => {
    const first = ringOf(AgentCursorBody({ name: "Ground floor", activity: "working", line: "Placing doors" }));
    const next = ringOf(AgentCursorBody({ name: "Ground floor", activity: "working", line: "Checking room sizes" }));
    expect(first?.key).toBe("Placing doors");
    // A new step is a new key: React mounts a new ring, so its one-shot ring plays again once.
    expect(next?.key).toBe("Checking room sizes");
  });

  it("no ring while idle, looking, paused or stopped", () => {
    for (const activity of ["idle", "looking", "paused", "stopped", null] as const) {
      expect(ringOf(AgentCursorBody({ name: "Services", activity, line: "Reading the plan" }))).toBeNull();
    }
  });

  it("shows the name, the AI tag and the line, and no sparkle", () => {
    const html = renderToStaticMarkup(<AgentCursorBody name="Services" activity="working" line="Reading the plan" />);
    expect(html).toContain("Services");
    expect(html).toContain(">AI<");
    expect(html).toContain("Reading the plan");
    expect(html).not.toContain("<svg");
  });

  it("the ring runs once, is invisible at rest, and stops under reduced motion", () => {
    const ring = rules(agentsCss).find(([sel]) => sel === ".glyphRing");
    expect(ring?.[1]).toMatch(/animation:\s*glyph-ring var\(--dur-scene\) var\(--ease-out\);/);
    expect(ring?.[1]).toMatch(/opacity:\s*0;/);
    expect(reducedMotion(agentsCss)).toContain(".glyphRing");
  });
});

describe("agent avatar", () => {
  it("is the rounded square and its status dot, with no corner mark", () => {
    const html = renderToStaticMarkup(<AgentAvatar name="Ground floor" color={3} activity="working" />);
    expect(html).toContain("GF");
    expect(html).toContain('data-activity="working"');
    // The sparkle was an svg in the corner; only the paused dot draws one now.
    expect(html).not.toContain("<svg");
  });

  it("working is a steady ring in the teal on color, not a loop and not purple", () => {
    const working = rules(agentsCss).filter(([sel]) => sel.includes('[data-activity="working"]'));
    expect(working.length).toBeGreaterThan(0);
    for (const [, body] of working) {
      expect(body).not.toMatch(/animation/);
      expect(body).not.toMatch(/var\(--ai/);
    }
    expect(agentsCss).toMatch(/--agent-on: var\(--accent\)/);
  });

  it("an idle or stopped avatar keeps its color and initials: the state shows on the dot only", () => {
    for (const [sel, body] of rules(agentsCss)) {
      if (/^\.avatar\b/.test(sel)) expect(body, sel).not.toMatch(/filter|opacity/);
    }
    expect(rules(agentsCss).some(([sel]) => sel === '.status[data-activity="idle"]')).toBe(false);
  });

  it("a working dot's ring stays inside the dot, and bar avatars sit apart, so nothing covers the next avatar", () => {
    const working = rules(agentsCss).find(([sel]) => sel === '.status[data-activity="working"]')?.[1] ?? "";
    // Only the 1.5 px separator in the surface color reaches outside; the teal ring is inset.
    expect(working).toMatch(/inset 0 0 0 1\.5px/);
    expect(working).not.toMatch(/0 0 0 3px/);
    const gap = Number(rules(agentsCss).find(([sel]) => sel === ".agentButton")?.[1].match(/margin-left:\s*(-?[\d.]+)px/)?.[1]);
    // 26 px buttons around 22 px avatars: the dot ends 3 px past the corner, so the next avatar starts 3 px on.
    expect(gap).toBeGreaterThanOrEqual(-1);
  });

  it("the Agents button's mark does not turn on hover", () => {
    expect(rules(agentsCss).some(([sel, body]) => sel.includes(".panelButton") && /rotate/.test(body))).toBe(false);
  });
});

describe("live and agents stylesheets", () => {
  it("nothing loops forever", () => {
    for (const [name, css] of Object.entries(SHEETS)) expect(css, name).not.toMatch(/\binfinite\b/);
  });

  it("no colored side rule on any card, message or row", () => {
    for (const [name, css] of Object.entries(SHEETS)) expect(css, name).not.toMatch(/border-left\s*:/);
  });

  it("pops ease out from 0.9 (0.8 for dots): no spring on pills, avatars, counts or chips", () => {
    for (const [name, css] of Object.entries(SHEETS)) expect(css, name).not.toMatch(/--ease-spring/);
    const from = (css: string, name: string) => css.match(new RegExp(`@keyframes ${name} \\{\\s*from \\{\\s*opacity: 0;\\s*transform: scale\\(([\\d.]+)\\)`))?.[1];
    expect(from(agentsCss, "agent-in")).toBe("0.9");
    expect(from(agentsCss, "dot-in")).toBe("0.8");
    expect(from(liveCss, "avatar-in")).toBe("0.9");
    expect(from(liveCss, "dot-in")).toBe("0.8");
    expect(from(buttonCss, "chat-in")).toBe("0.9");
    expect(from(panelCss, "chip-in")).toBe("0.9");
    expect(from(peekCss, "count-in")).toBe("0.9");
    // A new message bumps the unread count from 1.12, not 1.35.
    expect(liveCss).toMatch(/@keyframes bump \{\s*from \{\s*transform: scale\(1\.12\);/);
  });

  it("the Agents panel shows its rows as one block: no stagger", () => {
    expect(panelCss).not.toMatch(/animation-delay/);
    expect(panelCss).not.toMatch(/--i\b/);
  });

  it("a tab body cross-fades in place, opacity only", () => {
    const body = rules(panelCss).find(([sel]) => sel === ".body");
    expect(body?.[1]).toMatch(/animation:\s*global\(ui-fade-in\)/);
  });

  it("purple stays on the AI author tag", () => {
    for (const [name, css] of Object.entries(SHEETS)) {
      for (const [sel, body] of rules(css)) {
        if (/var\(--ai\b/.test(body) || /var\(--ai-soft\)/.test(body)) expect(sel, name).toMatch(/aiTag/);
      }
    }
  });

  it("one AI tag everywhere people and AI mix: the plan cursor, the 3D marker, Chat, the chat card, the Agents panel", () => {
    const keys = ["border-radius", "background", "color", "font-size", "font-weight", "line-height"];
    const look = (css: string) => {
      const body = rules(css).find(([sel]) => sel === ".aiTag")?.[1] ?? "";
      return keys.map((k) => body.match(new RegExp(`(?:^|[;\\s])${k}:\\s*([^;]+);`))?.[1].trim() ?? `no ${k}`);
    };
    const tagged = [agentsCss, pointersCss, chatCss, peekCss, panelCss];
    for (const css of tagged) expect(look(css)).toEqual(["var(--radius-pill)", "var(--ai-soft)", "var(--ai)", "var(--text-xs)", "650", "16px"]);
    // The 3D marker draws it too (it had none).
    expect(read("../viewer3d/live/LivePointers3D.tsx")).toMatch(/className=\{s\.aiTag\}>AI</);
  });

  it("no margin or padding transitions: only transforms, opacity and grid tracks move (MOTION.md rule 5)", () => {
    for (const [name, css] of Object.entries(SHEETS)) {
      for (const [sel, body] of rules(css)) {
        const transition = body.match(/transition:([^;]+);/)?.[1] ?? "";
        expect(transition, `${name} ${sel}`).not.toMatch(/\b(margin|padding)(-\w+)?\b/);
      }
    }
  });

  it("counts compose the shared count style (R14)", () => {
    expect(rules(panelCss).find(([sel]) => sel === ".reviewCount")?.[1]).toMatch(/composes: count from "\.\.\/ui\/controls\.module\.css"/);
    expect(rules(liveCss).find(([sel]) => sel === ".count")?.[1]).toMatch(/composes: count from "\.\.\/ui\/controls\.module\.css"/);
  });

  it("one unread badge style: the top bar keeps UnreadBadge's fill, only adds the ring", () => {
    const badge = rules(buttonCss).find(([sel]) => sel === ".badge > span")?.[1] ?? "";
    expect(badge).not.toMatch(/background|color:/);
    expect(badge).toMatch(/box-shadow/);
  });

  it("type stays at or above the 11 px floor", () => {
    for (const [name, css] of Object.entries(SHEETS)) expect(css, name).not.toMatch(/font(-size)?:[^;]*\b(9|9\.5|10|10\.5)px/);
  });
});
