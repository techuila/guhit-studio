import { describe, expect, it } from "vitest";
import {
  AGENTS,
  CORE_TOOLS,
  CURSOR_TOOL_WARNING,
  MAC_BINARY,
  TOOLSETS,
  sameToolsets,
  stdioArgsFor,
  stdioLine,
  toolCount,
  agentForClient,
  agentSlug,
  clientLabel,
  clineConfig,
  codexCommand,
  claudeCodeCommand,
  cursorLink,
  gooseLink,
  labelledUrl,
  lmstudioLink,
  otherCode,
  serverNameFor,
  setupFor,
  vscodeLink,
  windsurfConfig,
  zedConfig,
  type SetupContext,
} from "./agents";

describe("several agents (DECISIONS D47)", () => {
  it("makes a label safe for a URL", () => {
    expect(agentSlug("Upper floor")).toBe("upper-floor");
    expect(agentSlug("  Ñino's  Kitchen!! ")).toBe("nino-s-kitchen");
    expect(agentSlug("***")).toBe("");
    expect(agentSlug("a".repeat(50))).toHaveLength(32);
  });

  it("gives each agent its own address; toolsets of its own override the defaults", () => {
    const base = "http://127.0.0.1:1450/mcp";
    // No list: it gets the defaults, and later changes to them.
    expect(labelledUrl(base, "Upper floor")).toBe("http://127.0.0.1:1450/mcp/agent/upper-floor");
    expect(labelledUrl(`${base}/`, "Services", ["visuals", "services"])).toBe("http://127.0.0.1:1450/mcp/agent/services?toolsets=services,visuals");
    expect(labelledUrl(base, "", ["live"])).toBe("http://127.0.0.1:1450/mcp?toolsets=live");
    // An empty list of its own: the core tools only, whatever the defaults.
    expect(labelledUrl(base, "Upper floor", [])).toBe("http://127.0.0.1:1450/mcp/agent/upper-floor?toolsets=none");
    expect(TOOLSETS.map((t) => t.id)).toEqual(["services", "visuals", "live", "review", "interop"]);
  });

  it("counts the tools an agent lists: 22 core, 33 with every toolset", () => {
    expect(toolCount([])).toBe(CORE_TOOLS);
    expect(toolCount(["services", "review"])).toBe(26);
    expect(toolCount(TOOLSETS.map((t) => t.id))).toBe(33);
    expect(toolCount(TOOLSETS.map((t) => t.id))).toBeLessThan(CURSOR_TOOL_WARNING);
    expect(sameToolsets(["visuals", "services"], ["services", "visuals"])).toBe(true);
    expect(sameToolsets(["visuals"], [])).toBe(false);
  });

  it("names the agent and its toolsets for clients that launch a command", () => {
    expect(stdioArgsFor("Ground floor")).toEqual(["--agent", "Ground floor"]);
    expect(stdioArgsFor(" Upper  floor ", ["live", "services"])).toEqual(["--agent", "Upper floor", "--toolsets", "services,live"]);
    expect(stdioLine("/opt/guhit", stdioArgsFor("Ground floor", []))).toBe(`"/opt/guhit" --mcp-stdio --agent "Ground floor" --toolsets none`);
    const desktop = setupFor("claude-desktop", { url, stdioCommand: "/opt/guhit", claudeDesktopBundle: true, name: "guhit-ground-floor", stdioArgs: stdioArgsFor("Ground floor") });
    // The extension takes no flags: another agent gets the config file.
    expect(desktop.kind).toBe("snippet");
    expect(desktop.kind === "snippet" && JSON.parse(desktop.code).mcpServers["guhit-ground-floor"].args).toEqual(["--mcp-stdio", "--agent", "Ground floor"]);
    expect(setupFor("claude-desktop", ctx).kind).toBe("bundle");
  });

  it("names its server apart, so a second entry in one client keeps the first", () => {
    const u = "http://127.0.0.1:1450/mcp/agent/upper-floor";
    expect(serverNameFor("Upper floor")).toBe("guhit-upper-floor");
    expect(serverNameFor("")).toBe("guhit");
    expect(claudeCodeCommand(u, "guhit-upper-floor")).toBe(`claude mcp add --transport http guhit-upper-floor ${u} --scope user`);
    expect(codexCommand(u, "guhit-upper-floor")).toBe(`codex mcp add guhit-upper-floor --url ${u}`);
    expect(JSON.parse(clineConfig(u, "guhit-upper-floor")).mcpServers["guhit-upper-floor"].url).toBe(u);
    expect(cursorLink(u, "guhit-upper-floor")).toContain("name=guhit-upper-floor&");
    // The setups pass the name through; without one they keep "guhit".
    const c = { url: u, stdioCommand: null, claudeDesktopBundle: false };
    const named = setupFor("claude-code", { ...c, name: "guhit-upper-floor" });
    expect(named.kind === "snippet" && named.code).toContain("guhit-upper-floor");
    const plain = setupFor("codex", c);
    expect(plain.kind === "snippet" && plain.code).toBe(`codex mcp add guhit --url ${u}`);
  });
});

const url = "http://127.0.0.1:1450/mcp";
const ctx: SetupContext = { url, stdioCommand: "/opt/guhit/guhit-studio", claudeDesktopBundle: true };

describe("deep links", () => {
  it("cursor config decodes back to the url", () => {
    const link = cursorLink(url);
    const prefix = "cursor://anysphere.cursor-deeplink/mcp/install?name=guhit&config=";
    expect(link.startsWith(prefix)).toBe(true);
    expect(JSON.parse(atob(decodeURIComponent(link.slice(prefix.length))))).toEqual({ url });
  });
  it("lm studio config decodes back to the url", () => {
    const link = lmstudioLink(url);
    const prefix = "lmstudio://add_mcp?name=guhit&config=";
    expect(link.startsWith(prefix)).toBe(true);
    expect(JSON.parse(atob(decodeURIComponent(link.slice(prefix.length))))).toEqual({ url });
  });
  it("vscode carries name, type and url", () => {
    const link = vscodeLink(url);
    expect(link.startsWith("vscode:mcp/install?")).toBe(true);
    expect(JSON.parse(decodeURIComponent(link.slice("vscode:mcp/install?".length)))).toEqual({ name: "guhit", type: "http", url });
  });
  it("goose carries the encoded url and fixed fields", () => {
    expect(gooseLink(url)).toBe(
      `goose://extension?url=${encodeURIComponent(url)}&type=streamable_http&id=guhit&name=Guhit%20Studio&description=Guhit%20Studio%20floor%20plans&timeout=300`,
    );
  });
  it("follows a different port", () => {
    expect(gooseLink("http://127.0.0.1:1451/mcp")).toContain(encodeURIComponent("http://127.0.0.1:1451/mcp"));
  });
});

describe("snippets", () => {
  it("builds the commands", () => {
    expect(claudeCodeCommand(url)).toBe(`claude mcp add --transport http guhit ${url} --scope user`);
    expect(codexCommand(url)).toBe(`codex mcp add guhit --url ${url}`);
  });
  it("builds the config files", () => {
    expect(JSON.parse(windsurfConfig(url))).toEqual({ mcpServers: { guhit: { serverUrl: url } } });
    expect(JSON.parse(zedConfig(url))).toEqual({ context_servers: { guhit: { url } } });
    expect(JSON.parse(clineConfig(url))).toEqual({ mcpServers: { guhit: { type: "streamableHttp", url } } });
  });
  it("other agent shows the url and the stdio command", () => {
    expect(otherCode(url, "/x/guhit")).toBe(`URL      ${url}\nCommand  "/x/guhit" --mcp-stdio`);
    expect(otherCode(url, null)).toContain(`"${MAC_BINARY}" --mcp-stdio`);
  });
});

describe("setupFor", () => {
  it("every agent has a setup, and the url comes from the context", () => {
    const other = "http://127.0.0.1:1999/mcp";
    for (const a of AGENTS) {
      const s = setupFor(a.id, { ...ctx, url: other });
      expect(s.lead.length).toBeGreaterThan(0);
      if (s.kind === "link") expect(decodeURIComponent(s.link) + atob(decodeURIComponent(s.link).split("config=")[1] ?? "e30=")).toContain("1999");
      if (s.kind === "snippet" && a.id !== "claude-desktop") expect(s.code).toContain(other);
    }
  });
  it("claude desktop uses the bundle when shipped, else a stdio config", () => {
    expect(setupFor("claude-desktop", ctx).kind).toBe("bundle");
    const s = setupFor("claude-desktop", { ...ctx, claudeDesktopBundle: false });
    expect(s.kind).toBe("snippet");
    if (s.kind === "snippet") expect(JSON.parse(s.code)).toEqual({ mcpServers: { guhit: { command: "/opt/guhit/guhit-studio", args: ["--mcp-stdio"] } } });
  });
  it("chatgpt shows the bare url", () => {
    const s = setupFor("chatgpt", ctx);
    expect(s.kind === "snippet" && s.code).toBe(url);
  });
});

describe("agentForClient", () => {
  it("matches reported names loosely", () => {
    expect(agentForClient("claude-code")).toBe("claude-code");
    expect(agentForClient("Claude Code")).toBe("claude-code");
    expect(agentForClient("Cursor")).toBe("cursor");
    expect(agentForClient("claude-ai")).toBe("claude-desktop");
    expect(agentForClient("Visual Studio Code")).toBe("vscode");
    expect(agentForClient("codex-mcp-client")).toBe("codex");
    expect(agentForClient("LM Studio")).toBe("lmstudio");
  });
  it("returns null for unknown or empty", () => {
    expect(agentForClient("my-custom-thing")).toBeNull();
    expect(agentForClient(null)).toBeNull();
    expect(agentForClient("")).toBeNull();
  });
});

describe("clientLabel", () => {
  it("names a known client by its agent and keeps an unknown one", () => {
    expect(clientLabel("claude-code")).toBe("Claude Code");
    expect(clientLabel("Some New Agent")).toBe("Some New Agent");
  });
});
