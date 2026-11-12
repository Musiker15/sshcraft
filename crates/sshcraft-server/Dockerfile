# syntax=docker/dockerfile:1
# Build from the repository root:
#   docker build -f crates/sshcraft-server/Dockerfile -t sshcraft-server .
# The versions are pinned for reproducible builds; raise them when newer ones are out.
ARG RUST_VERSION=1.99.1
ARG DEBIAN_VERSION=13.7

FROM rust:${RUST_VERSION}-slim-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p sshcraft-server \
    && cp target/release/sshcraft-server /usr/local/bin/sshcraft-server

FROM debian:${DEBIAN_VERSION}-slim
RUN useradd --system --uid 10001 --home-dir /data sshcraft \
    && mkdir /data \
    && chown sshcraft /data
COPY --from=build /usr/local/bin/sshcraft-server /usr/local/bin/sshcraft-server
USER sshcraft
VOLUME /data
ENV SSHCRAFT_DATABASE_URL=sqlite:///data/sshcraft.db \
    SSHCRAFT_LISTEN=0.0.0.0:8470
EXPOSE 8470
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD ["sshcraft-server", "health"]
ENTRYPOINT ["sshcraft-server"]
CMD ["serve"]
