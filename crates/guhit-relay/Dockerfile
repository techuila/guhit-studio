# The live session relay (docs/RELAY.md). Build from the repository root:
#   docker build -f crates/guhit-relay/Dockerfile -t guhit-relay .
#   docker run -p 8080:8080 guhit-relay
# The context is the workspace, since cargo reads every member's manifest
# even to build one package. The root .dockerignore keeps it to the Rust
# sources.

FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p guhit-relay

FROM debian:trixie-slim
RUN useradd --uid 10001 --no-create-home --shell /usr/sbin/nologin relay
COPY --from=build /src/target/release/guhit-relay /usr/local/bin/guhit-relay
USER relay
EXPOSE 8080
CMD ["guhit-relay"]
