# syntax=docker/dockerfile:1
# Build from the repository root:
#   docker build -f crates/sshcraft-server/Dockerfile -t sshcraft-server .

FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p sshcraft-server \
    && cp target/release/sshcraft-server /usr/local/bin/sshcraft-server

FROM debian:trixie-slim
RUN useradd --system --uid 10001 --home-dir /data sshcraft \
    && mkdir /data \
    && chown sshcraft /data
COPY --from=build /usr/local/bin/sshcraft-server /usr/local/bin/sshcraft-server
USER sshcraft
VOLUME /data
ENV SSHCRAFT_DATABASE_URL=sqlite:///data/sshcraft.db \
    SSHCRAFT_LISTEN=0.0.0.0:8470
EXPOSE 8470
ENTRYPOINT ["sshcraft-server"]
CMD ["serve"]
