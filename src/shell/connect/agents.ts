// Data and link builders for the "Connect an AI agent" dialog (DECISIONS D35).
// Pure: no React, no I/O. Every address comes from the app's MCP status, so a
// different port or host shows up here without a code change.

/** Used only before the status has loaded. */
export const FALLBACK_URL = "http://127.0.0.1:1450/mcp";
/** Where the app binary lives on macOS, shown when the app cannot tell us. */
export const MAC_BINARY = "/Applications/Guhit Studio.app/Contents/MacOS/guhit-studio";
export const DOCS_URL = "https://github.com/techuila/guhit-studio/blob/main/docs/MCP.md#setup";

export type AgentGroup = "Popular" | "More agents" | "Free and local models";
export const GROUPS: AgentGroup[] = ["Popular", "More agents", "Free and local models"];

export interface SetupContext {
  /** The MCP address, from the status. */
  url: string;
  /** The app binary that speaks stdio, or null when unknown. */
  stdioCommand: string | null;
  /** True when the app ships a Claude Desktop extension file. */
  claudeDesktopBundle: boolean;
  /**
   * The server's name in the agent's config, "guhit" by default. Another
   * agent on a labelled URL gets its own ("guhit-upper-floor"), so a second
   * entry in one client does not replace the first.
   */
  name?: string;
  /**
   * For clients that launch a command: what follows `--mcp-stdio` for
   * another agent, `--agent "Upper floor"` and `--toolsets ...`.
   */
  stdioArgs?: string[];
}

export const SERVER_NAME = "guhit";

export type Setup =
  | { kind: "link"; lead: string; button: string; app: string; link: string; steps: string[] }
  | { kind: "bundle"; lead: string; button: string; app: string; steps: string[] }
  | { kind: "snippet"; lead: string; where?: string; code: string; steps: string[] };

export interface AgentDef {
  id: string;
  name: string;
  mark: string;
  color: string;
  group: AgentGroup;
  free?: boolean;
  /** Lowercase letters and digits only. A reported client name containing one of these is this agent. */
  match: string[];
  setup: (ctx: SetupContext) => Setup;
}

const b64 = (o: unknown) => btoa(JSON.stringify(o));
const json = (o: unknown) => JSON.stringify(o, null, 2);

export function cursorLink(url: string, name = SERVER_NAME): string {
  return `cursor://anysphere.cursor-deeplink/mcp/install?name=${encodeURIComponent(name)}&config=` + encodeURIComponent(b64({ url }));
}
export function vscodeLink(url: string, name = SERVER_NAME): string {
  return "vscode:mcp/install?" + encodeURIComponent(JSON.stringify({ name, type: "http", url }));
}
export function lmstudioLink(url: string, name = SERVER_NAME): string {
  return `lmstudio://add_mcp?name=${encodeURIComponent(name)}&config=` + encodeURIComponent(b64({ url }));
}
export function gooseLink(url: string, name = SERVER_NAME): string {
  return (
    "goose://extension?url=" +
    encodeURIComponent(url) +
    `&type=streamable_http&id=${encodeURIComponent(name)}&name=Guhit%20Studio&description=Guhit%20Studio%20floor%20plans&timeout=300`
  );
}
export const claudeCodeCommand = (url: string, name = SERVER_NAME) => `claude mcp add --transport http ${name} ${url} --scope user`;
export const codexCommand = (url: string, name = SERVER_NAME) => `codex mcp add ${name} --url ${url}`;
export const windsurfConfig = (url: string, name = SERVER_NAME) => json({ mcpServers: { [name]: { serverUrl: url } } });
export const antigravityConfig = windsurfConfig;
export const zedConfig = (url: string, name = SERVER_NAME) => json({ context_servers: { [name]: { url } } });
export const clineConfig = (url: string, name = SERVER_NAME) => json({ mcpServers: { [name]: { type: "streamableHttp", url } } });
export const claudeDesktopConfig = (command: string, name = SERVER_NAME, extra: readonly string[] = []) =>
  json({ mcpServers: { [name]: { command, args: ["--mcp-stdio", ...extra] } } });
/** A shell word: quoted when it has a space or a quote. */
const word = (w: string) => (/^[A-Za-z0-9_.,:/=@+-]+$/.test(w) ? w : `"${w.replace(/(["\\$`])/g, "\\$1")}"`);
export const stdioLine = (command: string, extra: readonly string[] = []) => [`"${command}"`, "--mcp-stdio", ...extra.map(word)].join(" ");
export function otherCode(url: string, stdioCommand: string | null, extra: readonly string[] = []): string {
  return `URL      ${url}\nCommand  ${stdioLine(stdioCommand ?? MAC_BINARY, extra)}`;
}

export const AGENTS: AgentDef[] = [
  {
    id: "claude-code", name: "Claude Code", mark: "CC", color: "#c1633f", group: "Popular", match: ["claudecode"],
    setup: ({ url, name }) => ({
      kind: "snippet",
      lead: "Run this once in a terminal. Claude Code remembers it for every project.",
      code: claudeCodeCommand(url, name),
      steps: ["Keep Guhit Studio open.", "Ask Claude Code to draw or change the plan. The window updates as it works."],
    }),
  },
  {
    id: "claude-desktop", name: "Claude Desktop", mark: "CD", color: "#a8553a", group: "Popular", match: ["claudedesktop", "claudeai"],
    // An agent of its own (`--agent`, `--toolsets`) needs the config file: the extension takes no flags.
    setup: ({ stdioCommand, claudeDesktopBundle, name, stdioArgs }) =>
      claudeDesktopBundle && !stdioArgs?.length
        ? {
            kind: "bundle",
            lead: "Installs Guhit as a Claude Desktop extension. Claude Desktop asks you to confirm.",
            button: "Install in Claude Desktop",
            app: "Claude Desktop",
            steps: ["Click Install in the Claude Desktop window that opens.", "Guhit opens by itself the first time Claude uses it."],
          }
        : {
            kind: "snippet",
            lead: "Add this to Claude Desktop's config file, then restart Claude Desktop.",
            where: "macOS: ~/Library/Application Support/Claude/claude_desktop_config.json. Windows: %APPDATA%\\Claude\\claude_desktop_config.json",
            code: claudeDesktopConfig(stdioCommand ?? MAC_BINARY, name, stdioArgs),
            steps: stdioCommand ? [] : ["The command shown is the default macOS location of the app."],
          },
  },
  {
    id: "cursor", name: "Cursor", mark: "Cu", color: "#1f1f1f", group: "Popular", match: ["cursor"],
    setup: ({ url, name }) => ({
      kind: "link",
      lead: "Adds Guhit to Cursor in one click. Cursor shows the setting before saving it.",
      button: "Add to Cursor",
      app: "Cursor",
      link: cursorLink(url, name),
      steps: ["Confirm in Cursor.", "Use Agent mode and ask for a change."],
    }),
  },
  {
    id: "vscode", name: "VS Code (Copilot)", mark: "VS", color: "#1f6fb5", group: "Popular", match: ["vscode", "visualstudiocode", "copilot"],
    setup: ({ url, name }) => ({
      kind: "link",
      lead: "Adds Guhit to VS Code for GitHub Copilot agent mode.",
      button: "Add to VS Code",
      app: "VS Code",
      link: vscodeLink(url, name),
      steps: ["Click Install in VS Code.", "Open Copilot Chat in Agent mode."],
    }),
  },
  {
    id: "codex", name: "Codex", mark: "Cx", color: "#2b2b2b", group: "Popular", match: ["codex"],
    setup: ({ url, name }) => ({
      kind: "snippet",
      lead: "Run this once in a terminal. The ChatGPT desktop app shares this setting.",
      code: codexCommand(url, name),
      steps: ["Keep Guhit Studio open.", "Ask Codex for a change."],
    }),
  },
  {
    id: "chatgpt", name: "ChatGPT desktop", mark: "GP", color: "#10a37f", group: "Popular", match: ["chatgpt", "openai"],
    setup: ({ url }) => ({
      kind: "snippet",
      lead: "In ChatGPT: Settings, MCP servers, Add server, Streamable HTTP, then paste this URL and restart.",
      code: url,
      steps: ["ChatGPT on the web cannot reach an app on your computer."],
    }),
  },
  {
    id: "windsurf", name: "Windsurf / Devin", mark: "Ws", color: "#0b7a6b", group: "More agents", match: ["windsurf", "devin"],
    setup: ({ url, name }) => ({ kind: "snippet", lead: "Add this to mcp_config.json.", where: "~/.config/devin/mcp_config.json", code: windsurfConfig(url, name), steps: [] }),
  },
  {
    id: "antigravity", name: "Antigravity", mark: "AG", color: "#4285f4", group: "More agents", match: ["antigravity"],
    setup: ({ url, name }) => ({
      kind: "snippet",
      lead: "Add this to Antigravity's MCP config. It must be serverUrl.",
      where: "~/.gemini/config/mcp_config.json",
      code: antigravityConfig(url, name),
      steps: [],
    }),
  },
  {
    id: "zed", name: "Zed", mark: "Z", color: "#3b3b3b", group: "More agents", match: ["zed"],
    setup: ({ url, name }) => ({ kind: "snippet", lead: "Add this to Zed's settings.json.", where: "Zed: Settings, Open Settings File", code: zedConfig(url, name), steps: [] }),
  },
  {
    id: "cline", name: "Cline", mark: "Cl", color: "#5a5a5a", group: "Free and local models", free: true, match: ["cline"],
    setup: ({ url, name }) => ({
      kind: "snippet",
      lead: "Cline, MCP Servers, Configure. Add this entry.",
      where: "cline_mcp_settings.json",
      code: clineConfig(url, name),
      steps: ["Works with Ollama and LM Studio models."],
    }),
  },
  {
    id: "lmstudio", name: "LM Studio", mark: "LM", color: "#6b4fd1", group: "Free and local models", free: true, match: ["lmstudio"],
    setup: ({ url, name }) => ({
      kind: "link",
      lead: "Adds Guhit to LM Studio in one click. Use an AI model with 32k context or more.",
      button: "Add to LM Studio",
      app: "LM Studio",
      link: lmstudioLink(url, name),
      steps: ["Confirm in LM Studio.", "LM Studio does not pass plan pictures to its AI model yet."],
    }),
  },
  {
    id: "goose", name: "Goose", mark: "Go", color: "#222222", group: "Free and local models", free: true, match: ["goose"],
    setup: ({ url, name }) => ({
      kind: "link",
      lead: "Adds Guhit as a Goose extension.",
      button: "Add to Goose",
      app: "Goose",
      link: gooseLink(url, name),
      steps: ["Confirm in Goose."],
    }),
  },
  {
    id: "other", name: "Other agent", mark: "+", color: "var(--ink-3)", group: "Free and local models", match: [],
    setup: ({ url, stdioCommand, stdioArgs }) => ({
      kind: "snippet",
      lead: "Agents that take a URL: use this one. Agents that only launch a command: use the app with --mcp-stdio.",
      code: otherCode(url, stdioCommand, stdioArgs),
      steps: [
        ...(stdioCommand ? [] : ["The command shown is the default macOS location of the app."]),
        "Exact steps for 30 agents are in the setup notes below.",
      ],
    }),
  },
];

const squash = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

/** The agent a reported client name belongs to ("claude-code", "Claude Code"), or null. */
export function agentForClient(name: string | null | undefined): string | null {
  if (!name) return null;
  const n = squash(name);
  if (n === "") return null;
  for (const a of AGENTS) if (a.match.some((m) => n.includes(m))) return a.id;
  return null;
}

/** The name to show for a client: the agent's own name when it is one we
 *  list ("claude-code" reads "Claude Code"), else what the client sent. */
export function clientLabel(name: string): string {
  const id = agentForClient(name);
  return id ? agentById(id).name : name;
}

export function agentById(id: string): AgentDef {
  return AGENTS.find((a) => a.id === id) ?? AGENTS[0];
}

export function setupFor(id: string, ctx: SetupContext): Setup {
  return agentById(id).setup(ctx);
}

// ---------------------------------------------------------------- several agents (DECISIONS D47)

/**
 * The extended toolsets of MCP v2 (`guhit_mcp::toolsets`), beside the core
 * tools every agent gets: what each adds and how many tools it lists.
 * Which ones agents get by default is a setting (`mcp_toolsets`); an
 * agent's own URL (`?toolsets=`) or command (`--toolsets`) overrides it.
 */
export const TOOLSETS: ReadonlyArray<{ id: string; label: string; hint: string; tools: number }> = [
  { id: "services", label: "Services", hint: "Draw and edit pipes, wiring and device links", tools: 3 },
  { id: "visuals", label: "Visuals", hint: "Path traced renders and AI visualizations", tools: 2 },
  { id: "live", label: "Live", hint: "The live session, chat and a status line", tools: 3 },
  { id: "review", label: "Review", hint: "Mark review items done or set aside", tools: 1 },
  { id: "interop", label: "Import and export", hint: "Bring in DXF files and underlays", tools: 2 },
];

/** The core toolset (drafting and checking the plan), always listed. */
export const CORE_TOOLS = 22;

/** Cursor warns when an agent lists more tools than this (DECISIONS D34). */
export const CURSOR_TOOL_WARNING = 40;

/** How many tools an agent lists with these toolsets on: 22 core, 33 with all. */
export function toolCount(toolsets: readonly string[]): number {
  return CORE_TOOLS + TOOLSETS.filter((t) => toolsets.includes(t.id)).reduce((n, t) => n + t.tools, 0);
}

/** The same toolsets, in the order the server lists them. */
export function sameToolsets(a: readonly string[], b: readonly string[]): boolean {
  const on = (list: readonly string[]) => TOOLSETS.filter((t) => list.includes(t.id)).map((t) => t.id).join(",");
  return on(a) === on(b);
}

/** Toolsets as a URL or `--toolsets` takes them: "services,visuals", or "none" for core only. */
export function toolsetList(toolsets: readonly string[]): string {
  const on = TOOLSETS.filter((t) => toolsets.includes(t.id)).map((t) => t.id);
  return on.length > 0 ? on.join(",") : "none";
}

/** A label as it goes in a URL: "Upper floor" is "upper-floor". Empty when nothing usable is left. */
export function agentSlug(raw: string): string {
  return raw
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 32)
    .replace(/-+$/, "");
}

/**
 * The address for one more agent: `http://127.0.0.1:1450/mcp/agent/upper-floor`.
 * `toolsets` null: it gets the default toolsets, and later changes to them.
 * A list overrides them: `?toolsets=services,visuals`, `?toolsets=none` for
 * the core tools only. Each labelled URL is its own agent in the app: its
 * own name, cursor and limits.
 */
export function labelledUrl(base: string, label: string, toolsets: readonly string[] | null = null): string {
  const root = base.replace(/\/+$/, "");
  const slug = agentSlug(label);
  const path = slug ? `${root}/agent/${slug}` : root;
  return toolsets === null ? path : `${path}?toolsets=${toolsetList(toolsets)}`;
}

/**
 * The same agent for clients that launch a command (`guhit-studio
 * --mcp-stdio`): `--agent "Upper floor"`, and `--toolsets` when it
 * overrides the defaults.
 */
export function stdioArgsFor(label: string, toolsets: readonly string[] | null = null): string[] {
  const name = label.replace(/\s+/g, " ").trim();
  const out = name ? ["--agent", name] : [];
  return toolsets === null ? out : [...out, "--toolsets", toolsetList(toolsets)];
}

/** The server's name in the agent's config for a labelled agent: "guhit-upper-floor". */
export function serverNameFor(label: string): string {
  const slug = agentSlug(label);
  return slug ? `${SERVER_NAME}-${slug}` : SERVER_NAME;
}
