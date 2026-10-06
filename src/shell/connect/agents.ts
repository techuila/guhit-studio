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
}

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

export function cursorLink(url: string): string {
  return "cursor://anysphere.cursor-deeplink/mcp/install?name=guhit&config=" + encodeURIComponent(b64({ url }));
}
export function vscodeLink(url: string): string {
  return "vscode:mcp/install?" + encodeURIComponent(JSON.stringify({ name: "guhit", type: "http", url }));
}
export function lmstudioLink(url: string): string {
  return "lmstudio://add_mcp?name=guhit&config=" + encodeURIComponent(b64({ url }));
}
export function gooseLink(url: string): string {
  return (
    "goose://extension?url=" +
    encodeURIComponent(url) +
    "&type=streamable_http&id=guhit&name=Guhit%20Studio&description=Guhit%20Studio%20floor%20plans&timeout=300"
  );
}
export const claudeCodeCommand = (url: string) => `claude mcp add --transport http guhit ${url} --scope user`;
export const codexCommand = (url: string) => `codex mcp add guhit --url ${url}`;
export const windsurfConfig = (url: string) => json({ mcpServers: { guhit: { serverUrl: url } } });
export const antigravityConfig = windsurfConfig;
export const zedConfig = (url: string) => json({ context_servers: { guhit: { url } } });
export const clineConfig = (url: string) => json({ mcpServers: { guhit: { type: "streamableHttp", url } } });
export const claudeDesktopConfig = (command: string) => json({ mcpServers: { guhit: { command, args: ["--mcp-stdio"] } } });
export const stdioLine = (command: string) => `"${command}" --mcp-stdio`;
export function otherCode(url: string, stdioCommand: string | null): string {
  return `URL      ${url}\nCommand  ${stdioLine(stdioCommand ?? MAC_BINARY)}`;
}

export const AGENTS: AgentDef[] = [
  {
    id: "claude-code", name: "Claude Code", mark: "CC", color: "#c1633f", group: "Popular", match: ["claudecode"],
    setup: ({ url }) => ({
      kind: "snippet",
      lead: "Run this once in a terminal. Claude Code remembers it for every project.",
      code: claudeCodeCommand(url),
      steps: ["Keep Guhit open.", "Ask Claude Code to draw or change the plan. The window updates as it works."],
    }),
  },
  {
    id: "claude-desktop", name: "Claude Desktop", mark: "CD", color: "#a8553a", group: "Popular", match: ["claudedesktop", "claudeai"],
    setup: ({ stdioCommand, claudeDesktopBundle }) =>
      claudeDesktopBundle
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
            code: claudeDesktopConfig(stdioCommand ?? MAC_BINARY),
            steps: stdioCommand ? [] : ["The command shown is the default macOS location of the app."],
          },
  },
  {
    id: "cursor", name: "Cursor", mark: "Cu", color: "#1f1f1f", group: "Popular", match: ["cursor"],
    setup: ({ url }) => ({
      kind: "link",
      lead: "Adds Guhit to Cursor in one click. Cursor shows the setting before saving it.",
      button: "Add to Cursor",
      app: "Cursor",
      link: cursorLink(url),
      steps: ["Confirm in Cursor.", "Use Agent mode and ask for a change."],
    }),
  },
  {
    id: "vscode", name: "VS Code (Copilot)", mark: "VS", color: "#1f6fb5", group: "Popular", match: ["vscode", "visualstudiocode", "copilot"],
    setup: ({ url }) => ({
      kind: "link",
      lead: "Adds Guhit to VS Code for GitHub Copilot agent mode.",
      button: "Add to VS Code",
      app: "VS Code",
      link: vscodeLink(url),
      steps: ["Click Install in VS Code.", "Open Copilot Chat in Agent mode."],
    }),
  },
  {
    id: "codex", name: "Codex", mark: "Cx", color: "#2b2b2b", group: "Popular", match: ["codex"],
    setup: ({ url }) => ({
      kind: "snippet",
      lead: "Run this once in a terminal. The ChatGPT desktop app shares this setting.",
      code: codexCommand(url),
      steps: ["Keep Guhit open.", "Ask Codex for a change."],
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
    setup: ({ url }) => ({ kind: "snippet", lead: "Add this to mcp_config.json.", where: "~/.config/devin/mcp_config.json", code: windsurfConfig(url), steps: [] }),
  },
  {
    id: "antigravity", name: "Antigravity", mark: "AG", color: "#4285f4", group: "More agents", match: ["antigravity"],
    setup: ({ url }) => ({
      kind: "snippet",
      lead: "Add this to Antigravity's MCP config. It must be serverUrl.",
      where: "~/.gemini/config/mcp_config.json",
      code: antigravityConfig(url),
      steps: [],
    }),
  },
  {
    id: "zed", name: "Zed", mark: "Z", color: "#3b3b3b", group: "More agents", match: ["zed"],
    setup: ({ url }) => ({ kind: "snippet", lead: "Add this to Zed's settings.json.", where: "Zed: Settings, Open Settings File", code: zedConfig(url), steps: [] }),
  },
  {
    id: "cline", name: "Cline", mark: "Cl", color: "#5a5a5a", group: "Free and local models", free: true, match: ["cline"],
    setup: ({ url }) => ({
      kind: "snippet",
      lead: "Cline, MCP Servers, Configure. Add this entry.",
      where: "cline_mcp_settings.json",
      code: clineConfig(url),
      steps: ["Works with Ollama and LM Studio models."],
    }),
  },
  {
    id: "lmstudio", name: "LM Studio", mark: "LM", color: "#6b4fd1", group: "Free and local models", free: true, match: ["lmstudio"],
    setup: ({ url }) => ({
      kind: "link",
      lead: "Adds Guhit to LM Studio in one click. Use a model with 32k context or more.",
      button: "Add to LM Studio",
      app: "LM Studio",
      link: lmstudioLink(url),
      steps: ["Confirm in LM Studio.", "Plan pictures do not reach the model in LM Studio yet."],
    }),
  },
  {
    id: "goose", name: "Goose", mark: "Go", color: "#222222", group: "Free and local models", free: true, match: ["goose"],
    setup: ({ url }) => ({
      kind: "link",
      lead: "Adds Guhit as a Goose extension.",
      button: "Add to Goose",
      app: "Goose",
      link: gooseLink(url),
      steps: ["Confirm in Goose."],
    }),
  },
  {
    id: "other", name: "Other agent", mark: "+", color: "#7b8896", group: "Free and local models", match: [],
    setup: ({ url, stdioCommand }) => ({
      kind: "snippet",
      lead: "Agents that take a URL: use this one. Agents that only launch a command: use the app with --mcp-stdio.",
      code: otherCode(url, stdioCommand),
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
