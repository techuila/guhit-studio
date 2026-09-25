# Live session relay

DECISIONS D32. A live session's host keeps the project on its own computer
(D29). Guests on the same network or VPN connect to it directly. Everyone
else connects through the relay: a small server that pairs a guest with the
host and forwards their bytes, nothing more. It is the VS Code Live Share
model.

The session inside is still TLS 1.3 between the guest and the host, pinned to
the certificate in the invite, so the relay cannot read or change a plan, a
chat message or a file. It stores nothing and has no accounts.

What the relay does see: the addresses of the host and the guests, the room
id, and when and how much they send. It logs counts and errors, never
contents, room ids or addresses.

Code: `crates/guhit-relay` (the service) and `crates/guhit-app/src/live/relay.rs`
(the app's side).

## Protocol, version 1

The base URL is the app's relay setting, for example
`wss://relay.example.com`, or with a path prefix,
`wss://example.com/guhit`. The endpoints below are the base URL plus their
path. `ws://` is accepted only for a loopback host (tests and local runs).

Every connection is a WebSocket. Until a pair opens, the messages are text
frames holding one JSON object each. After it opens, only binary frames are
used, and the relay forwards them as they are.

Identifiers:

- `room`: 16 random bytes, base64url without padding (22 characters). The
  host picks it for the session and it goes into the invite.
- `key`: 32 random bytes, base64url without padding (43 characters). The
  host picks it and sends it to the relay only, never in the invite. It
  proves that a control or accept connection is the host's.
- `conn`: the relay's number for one guest connection, as a decimal string.

### Host control: `GET /v1/host`

1. Within 10 s the host sends `{"v":1,"room":"<room>","key":"<key>"}`.
2. The relay registers the room and answers `{"type":"ready"}`.
   - The room is already registered with the same key: the new connection
     takes over, and the relay closes the old one with 4001. A host whose
     connection half died can come back at once.
   - It is registered with another key: close 4409.
   - A malformed message, another `v`, or nothing within 10 s: close 4400.
   - Over a limit: close 4429.
3. For every guest that arrives, the relay sends `{"type":"guest","conn":"<conn>"}`.
4. The room exists as long as its control connection. When that closes,
   guests still waiting are closed with 4404. Pairs already open go on.

The host reconnects a lost control connection after 1, 2, 4, 8, 16 and then
every 30 s, with the same room and key, so invites keep working.

### Guest: `GET /v1/join/<room>`

1. No such room (malformed ids included): close 4404. Too many guests waiting
   or pairs open in the room, or another limit: close 4429.
2. The relay sends the host `{"type":"guest","conn":"<conn>"}`.
3. The host has 10 s to accept. Otherwise the guest is closed with 4408.
4. When the host accepts, the relay sends the guest `{"type":"open"}`, and
   binary frames flow. A binary frame from the guest before that: close 4400.

### Host accept: `GET /v1/accept`

1. Within 10 s the host sends `{"v":1,"room":"<room>","key":"<key>","conn":"<conn>"}`.
2. Another key: close 4403. No such room, or no guest `conn` waiting in it:
   close 4404. Malformed, or nothing within 10 s: close 4400.
3. The relay sends `{"type":"open"}` to both, and binary frames flow both
   ways. The guest speaks first (its TLS hello), so the host waits for
   `open` and then for the guest.

### An open pair

- Binary frames go to the other side unchanged and in order. A text frame
  closes both with 4400.
- When one side closes or fails, the relay closes the other with 1000.
- The relay pings every connection every 20 s. One that sends nothing for
  60 s, pongs included, is closed with 4000. WebSocket libraries answer
  pings on their own; the session inside also sends a ping every 5 s.
- A frame over 256 KiB closes the connection (1009). The app sends at most
  64 KiB per frame.
- Each direction of a pair may average `RELAY_RATE` bytes a second with
  bursts up to `RELAY_BURST`. Past that the relay reads more slowly; it does
  not close the pair.

### Close codes

| Code | Meaning |
|---|---|
| 1000 | Normal close, or the other side of the pair closed |
| 1009 | A frame over 256 KiB |
| 4000 | Nothing received for 60 s |
| 4001 | Replaced by a newer control connection for the same room |
| 4400 | Bad request: a malformed or unexpected message |
| 4403 | Wrong key |
| 4404 | No such room, or no such waiting guest |
| 4408 | The host did not accept in time |
| 4409 | The room is taken by another key |
| 4429 | Over a limit |

## Running a relay

Settings, from environment variables:

| Variable | Default | What |
|---|---|---|
| `PORT` | `8080` | Port to listen on, all interfaces, plain HTTP |
| `RELAY_MAX_ROOMS` | `5000` | Rooms at once |
| `RELAY_MAX_CONNECTIONS` | `20000` | WebSockets at once |
| `RELAY_MAX_PER_IP` | `64` | WebSockets at once from one client address |
| `RELAY_MAX_ROOMS_PER_IP` | `8` | Rooms at once from one client address |
| `RELAY_MAX_WAITING_PER_ROOM` | `8` | Guests waiting for the host's accept |
| `RELAY_MAX_PAIRS_PER_ROOM` | `32` | Open pairs in one room |
| `RELAY_RATE` | `4194304` | Bytes a second, each direction of a pair |
| `RELAY_BURST` | `16777216` | Burst bytes, each direction of a pair |
| `RELAY_CLIENT_IP_HEADER` | unset | Header with the client's address when behind a proxy, for example `fly-client-ip`, or `x-forwarded-for` (its first entry). Unset: the socket's address |

`GET /health` answers `200 ok`.

The relay speaks plain HTTP. TLS comes from whatever is in front of it: the
hosting platform, or a reverse proxy.

- Local: `PORT=1470 cargo run -p guhit-relay`, then the app setting
  `"live_relay": "ws://127.0.0.1:1470"`.
- Container: `docker build -f crates/guhit-relay/Dockerfile -t guhit-relay .`
  from the repository root, then `docker run -p 8080:8080 guhit-relay`.
- Fly.io: `crates/guhit-relay/fly.toml` runs one machine in Singapore
  (`sin`), close to the Philippines. `fly launch --no-deploy --copy-config
  --config crates/guhit-relay/fly.toml` once, then `fly deploy --config
  crates/guhit-relay/fly.toml` from the repository root. The URL is
  `wss://<app>.fly.dev`.
- Any server: run the container and put a TLS proxy in front, for example
  Caddy with `relay.example.com { reverse_proxy 127.0.0.1:8080 }`.

One small machine is enough to start: the relay keeps rooms in memory and
forwards bytes. The app sends the whole project to every guest on each
change, compressed; a house is tens of KB.

## Pointing the app at a relay

- Setting `live_relay` in the data folder's `settings.json`, for example
  `"live_relay": "wss://relay.example.com"`. An empty string switches the
  relay off.
- Without the setting, the build's `GUHIT_RELAY_URL` environment variable at
  compile time, if it was set. Release builds get the deployed relay this
  way.
- Setting `live_direct: false` makes a host use only the relay: it does not
  listen for direct connections, and its invites list no addresses.

Details of the invite and the app's side: `docs/CONTRACT.md`, "Live sessions".

## Library API

For tests, `guhit-relay` is also a library:

```rust
pub struct Config { /* every limit and timeout above, public fields */ }
impl Default for Config { /* the defaults above */ }
impl Config { pub fn from_env() -> Config }
pub fn router(config: Config) -> axum::Router;
pub async fn serve(listener: tokio::net::TcpListener, config: Config) -> std::io::Result<()>;
```

`serve` runs until the listener fails. Tests bind `127.0.0.1:0` and pass the
listener in.
