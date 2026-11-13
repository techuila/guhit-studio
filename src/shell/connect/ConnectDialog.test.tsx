// The Connect agent dialog: one tab stop for the agent list, grouped under
// labelled groups (U44), the setup before who is working and the tools
// (U16), an error that says what to do (U35), and a top bar button that keeps
// its name when its words fold away (U1).
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it } from "vitest";
import type { McpStatus } from "../../contract/bindings";
import { ConnectButton } from "./ConnectButton";
import { ConnectDialog } from "./ConnectDialog";
import { useConnect } from "./connectStore";

const status = (more: Partial<McpStatus> = {}): McpStatus => ({
  enabled: true,
  listening: true,
  port: 1450,
  url: "http://127.0.0.1:1450/mcp",
  stdio_command: null,
  last_client: null,
  claude_desktop_bundle: false,
  agents: [],
  ...more,
});

// Server rendering reads a store's first state: set that, and put it back after.
const initial = useConnect.getInitialState();
const saved = { ...initial };
const show = (more: Partial<typeof initial>) => Object.assign(initial, more);
afterEach(() => Object.assign(initial, saved));

describe("connect dialog", () => {
  it("the agent list has one tab stop, inside labelled groups", () => {
    show({ status: status() });
    const html = renderToStaticMarkup(<ConnectDialog />);
    const options = html.match(/<button[^>]*role="option"[^>]*>/g) ?? [];
    expect(options.length).toBeGreaterThan(5);
    expect(options.filter((o) => o.includes('tabindex="0"'))).toHaveLength(1);
    expect(options.find((o) => o.includes('tabindex="0"'))).toContain('aria-selected="true"');
    expect(html).toContain('role="group" aria-labelledby="connect-group-popular"');
    expect(html).toContain('id="connect-group-free-and-local-models"');
  });

  it("shows the setup before the tools agents get", () => {
    show({ status: status(), toolsets: [] });
    const html = renderToStaticMarkup(<ConnectDialog />);
    expect(html.indexOf('role="listbox"')).toBeGreaterThan(-1);
    expect(html.indexOf('data-testid="connect-toolsets"')).toBeGreaterThan(html.indexOf('role="listbox"'));
  });

  it("a taken port says what to do about it", () => {
    show({ status: status({ listening: false }) });
    expect(renderToStaticMarkup(<ConnectDialog />)).toContain("Not listening: another app uses port 1450. Quit it, then turn Allow agents off and on.");
  });

  it("names Guhit Studio in sentences", () => {
    show({ status: status() });
    const html = renderToStaticMarkup(<ConnectDialog />);
    expect(html).toContain("Guhit Studio never sees a key");
    // The LLM is "the AI model", never "the model" (R7).
    expect(html).toContain("local AI model does the thinking");
  });
});

describe("connect button", () => {
  it("keeps its name and a link mark when its words fold away", () => {
    const html = renderToStaticMarkup(<ConnectButton variant="chrome" />);
    expect(html).toContain('aria-label="Connect agent"');
    expect(html).toContain("data-fold-label");
    expect(html).toContain("<svg");
  });
});
