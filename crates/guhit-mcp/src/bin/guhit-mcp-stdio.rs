//! Dev-only stdio proxy: newline-delimited JSON-RPC on stdin and stdout,
//! forwarded to the MCP endpoint on 127.0.0.1. Use it against the dev bridge:
//!
//! ```text
//! cargo run -p guhit-devbridge -- --port 1631 --data .devdata/mcp
//! cargo run -p guhit-mcp --bin guhit-mcp-stdio -- --port 1631 --agent kitchen --toolsets visuals
//! ```
//!
//! It never starts anything: with nothing listening, a tool call gets the
//! "not open" error. The desktop app's `--mcp-stdio` mode starts the app.

const USAGE: &str = "usage: guhit-mcp-stdio [--port 1450] [--agent <label>] [--toolsets services,visuals,live,review,interop]";

struct Args {
    port: u16,
    agent: Option<String>,
    toolsets: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut out = Args { port: guhit_mcp::DEFAULT_PORT, agent: None, toolsets: None };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                let v = args.next().ok_or("--port needs a value")?;
                out.port = v.parse().ok().filter(|p| *p > 0).ok_or(format!("bad port `{v}`"))?;
            }
            "--agent" => out.agent = Some(args.next().ok_or("--agent needs a label")?),
            "--toolsets" => out.toolsets = Some(args.next().ok_or("--toolsets needs a list")?),
            "-h" | "--help" => {
                eprintln!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(out)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("guhit-mcp-stdio: {e}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = guhit_mcp::stdio::run_stdio_proxy_as(args.port, None, args.agent, args.toolsets).await {
        eprintln!("guhit-mcp-stdio: {e}");
        std::process::exit(1);
    }
}
