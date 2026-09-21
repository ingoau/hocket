# Headless Hocket coordinator. Build from the workspace root:
#   docker build -f crates/hocket-coordinator/Dockerfile -t hocket-coordinator .
# Run:
#   docker run -p 7373:7373 -v hocket-data:/data hocket-coordinator
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# The coordinator needs neither audio nor mDNS; the workspace's other members
# are built only as far as their manifests require.
RUN cargo build --release -p hocket-coordinator --locked \
    && strip target/release/hocket-coordinator

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --home /data --create-home hocket
COPY --from=build /src/target/release/hocket-coordinator /usr/local/bin/hocket-coordinator
USER hocket
VOLUME ["/data"]
EXPOSE 7373
ENV RUST_LOG=info
HEALTHCHECK --interval=30s --timeout=3s CMD ["/bin/sh", "-c", "exec 3<>/dev/tcp/127.0.0.1/7373 && printf 'GET /health HTTP/1.0\\r\\n\\r\\n' >&3 && head -c 200 <&3 | grep -q '\"status\":\"ok\"'"]
ENTRYPOINT ["/usr/local/bin/hocket-coordinator"]
CMD ["--listen", "0.0.0.0:7373", "--data-dir", "/data"]
