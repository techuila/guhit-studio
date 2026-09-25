//! The relay program: settings from the environment (docs/RELAY.md,
//! "Running a relay"), then serve until stopped.

use std::net::SocketAddr;
use std::process::ExitCode;

use guhit_relay::Config;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> ExitCode {
    let config = Config::from_env();
    let addr = SocketAddr::new(config.bind, config.port);
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("guhit-relay: cannot listen on {addr}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let addr = listener.local_addr().unwrap_or(addr);
    println!("guhit-relay {} listening on {addr}", env!("CARGO_PKG_VERSION"));
    tokio::select! {
        result = guhit_relay::serve(listener, config) => match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("guhit-relay: {e}");
                ExitCode::FAILURE
            }
        },
        () = stop_signal() => ExitCode::SUCCESS,
    }
}

/// Ctrl-C, or SIGTERM from a container runtime. In a container the relay is
/// process 1, which ignores a signal it has no handler for.
async fn stop_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut term) => {
                term.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
