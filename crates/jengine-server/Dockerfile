# The stock jengine-server (relay rooms) as a container.
#
#   docker build -f crates/jengine-server/Dockerfile -t jengine-server .
#   docker run -p 9001:9001 -v jengine-data:/data jengine-server
#
# Built from the repo root, because the crate is a workspace member and cargo
# reads every member's manifest. A game's own server is the same file with
# its crate's name in place of `jengine-server`.
#
# Built and run with Podman on 2026-10-06 (91 MB image, five minutes to
# build; `podman stop` ends it in three seconds - SIGTERM, saving everyone).

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p jengine-server --bin jengine-server

FROM debian:bookworm-slim
COPY --from=build /src/target/release/jengine-server /usr/local/bin/jengine-server
VOLUME /data
EXPOSE 9001
# SIGTERM is what an orchestrator sends, and what the server saves everyone on.
STOPSIGNAL SIGTERM
ENTRYPOINT ["jengine-server", "0.0.0.0:9001", "--db", "/data/server.db"]
