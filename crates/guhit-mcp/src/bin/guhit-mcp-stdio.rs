//! Dev-only stdio proxy: newline-delimited JSON-RPC on stdin and stdout,
//! forwarded to the MCP endpoint on 127.0.0.1. Use it against the dev bridge:
//!
//! ```text
//! cargo run -p guhit-devbridge -- --port 1631 --data .devdata/mcp
//! cargo run -p guhit-mcp --bin guhit-mcp-stdio -- --port 1631
//! ```
//!
//! It never starts anything: with nothing listening, a tool call gets the
//! "not open" error. The desktop app's `--mcp-stdio` mode starts the app.

const USAGE: &str = "usage: guhit-mcp-stdio [--port 1450]";

fn parse_port() -> Result<u16, String> {
    let mut port = guhit_mcp::DEFAULT_PORT;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                let v = args.next().ok_or("--port needs a value")?;
                port = v.parse().ok().filter(|p| *p > 0).ok_or(format!("bad port `{v}`"))?;
            }
            "-h" | "--help" => {
                eprintln!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(port)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let port = match parse_port() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("guhit-mcp-stdio: {e}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = guhit_mcp::stdio::run_stdio_proxy(port, None).await {
        eprintln!("guhit-mcp-stdio: {e}");
        std::process::exit(1);
    }
}
