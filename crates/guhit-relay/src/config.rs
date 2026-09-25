//! The relay's settings (docs/RELAY.md, "Running a relay").

use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::time::Duration;

use axum::http::HeaderName;

/// Every setting of a relay. `Default` holds the values docs/RELAY.md lists.
/// The environment sets the ones in its table; the timeouts and the frame
/// size are fixed by the protocol, and are fields only so tests can shorten
/// them.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Address the bin listens on (`RELAY_BIND`).
    pub bind: IpAddr,
    /// Port the bin listens on (`PORT`).
    pub port: u16,
    /// Rooms at once (`RELAY_MAX_ROOMS`).
    pub max_rooms: usize,
    /// WebSockets at once (`RELAY_MAX_CONNECTIONS`).
    pub max_connections: usize,
    /// WebSockets at once from one client address (`RELAY_MAX_PER_IP`).
    pub max_per_ip: usize,
    /// Rooms at once from one client address (`RELAY_MAX_ROOMS_PER_IP`).
    pub max_rooms_per_ip: usize,
    /// Guests of one room waiting for the host's accept
    /// (`RELAY_MAX_WAITING_PER_ROOM`).
    pub max_waiting_per_room: usize,
    /// Open pairs in one room (`RELAY_MAX_PAIRS_PER_ROOM`).
    pub max_pairs_per_room: usize,
    /// Bytes a second each direction of a pair may average (`RELAY_RATE`).
    /// 0 turns the limit off; the environment cannot set 0.
    pub rate: u64,
    /// Bytes each direction of a pair may send at once before the rate
    /// applies (`RELAY_BURST`).
    pub burst: u64,
    /// Header with the client's address, for a relay behind a proxy
    /// (`RELAY_CLIENT_IP_HEADER`). None: the socket's address.
    pub client_ip_header: Option<String>,
    /// Time a host has for its first message on `/v1/host` and `/v1/accept`.
    pub hello_timeout: Duration,
    /// Time a host has to accept a guest.
    pub accept_timeout: Duration,
    /// Time between the relay's pings, on every connection.
    pub ping_interval: Duration,
    /// A connection that sends nothing for this long, pongs included, is
    /// closed with 4000.
    pub idle_timeout: Duration,
    /// Largest frame, and largest message, a client may send. Larger closes
    /// the connection with 1009.
    pub max_frame: usize,
    /// Time a connection the relay closes gets to answer the close frame.
    /// Not in docs/RELAY.md: it only decides when the socket goes away.
    pub close_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 8080,
            max_rooms: 5000,
            max_connections: 20_000,
            max_per_ip: 64,
            max_rooms_per_ip: 8,
            max_waiting_per_room: 8,
            max_pairs_per_room: 32,
            rate: 4 * 1024 * 1024,
            burst: 16 * 1024 * 1024,
            client_ip_header: None,
            hello_timeout: Duration::from_secs(10),
            accept_timeout: Duration::from_secs(10),
            ping_interval: Duration::from_secs(20),
            idle_timeout: Duration::from_secs(60),
            max_frame: 256 * 1024,
            close_timeout: Duration::from_secs(5),
        }
    }
}

impl Config {
    /// The settings from the environment. A value that does not parse is
    /// fatal: this prints what is wrong and exits with status 2, so a relay
    /// never runs on a setting it misread.
    pub fn from_env() -> Config {
        let lookup = |name: &str| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned());
        match Config::from_lookup(lookup) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("guhit-relay: {e}");
                std::process::exit(2);
            }
        }
    }

    /// The settings from `lookup`, which answers a variable's value, so
    /// parsing is testable without the process environment. A variable that
    /// is unset or empty keeps its default.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let get = |name: &str| lookup(name).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let mut c = Config::default();
        if let Some(v) = get("RELAY_BIND") {
            c.bind = v
                .parse()
                .map_err(|_| format!("RELAY_BIND must be an IP address such as 0.0.0.0 or 127.0.0.1, not `{v}`"))?;
        }
        if let Some(v) = get("PORT") {
            c.port = v.parse().map_err(|_| format!("PORT must be a port number up to 65535, not `{v}`"))?;
        }
        if let Some(v) = get("RELAY_MAX_ROOMS") {
            c.max_rooms = whole("RELAY_MAX_ROOMS", &v)?;
        }
        if let Some(v) = get("RELAY_MAX_CONNECTIONS") {
            c.max_connections = whole("RELAY_MAX_CONNECTIONS", &v)?;
        }
        if let Some(v) = get("RELAY_MAX_PER_IP") {
            c.max_per_ip = whole("RELAY_MAX_PER_IP", &v)?;
        }
        if let Some(v) = get("RELAY_MAX_ROOMS_PER_IP") {
            c.max_rooms_per_ip = whole("RELAY_MAX_ROOMS_PER_IP", &v)?;
        }
        if let Some(v) = get("RELAY_MAX_WAITING_PER_ROOM") {
            c.max_waiting_per_room = whole("RELAY_MAX_WAITING_PER_ROOM", &v)?;
        }
        if let Some(v) = get("RELAY_MAX_PAIRS_PER_ROOM") {
            c.max_pairs_per_room = whole("RELAY_MAX_PAIRS_PER_ROOM", &v)?;
        }
        if let Some(v) = get("RELAY_RATE") {
            c.rate = whole("RELAY_RATE", &v)?;
            if c.rate == 0 {
                return Err("RELAY_RATE must be at least 1: 0 would stop every pair".into());
            }
        }
        if let Some(v) = get("RELAY_BURST") {
            c.burst = whole("RELAY_BURST", &v)?;
        }
        if let Some(v) = get("RELAY_CLIENT_IP_HEADER") {
            let name = HeaderName::from_str(&v).map_err(|_| {
                format!("RELAY_CLIENT_IP_HEADER must be a header name such as fly-client-ip, not `{v}`")
            })?;
            c.client_ip_header = Some(name.as_str().to_string());
        }
        Ok(c)
    }
}

fn whole<T: FromStr>(name: &str, value: &str) -> Result<T, String> {
    value.parse().map_err(|_| format!("{name} must be a whole number, not `{value}`"))
}
