# Multi-stage build for AIMP node
FROM rust:1.85-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY aimp_node/ aimp_node/

RUN cargo build --release --manifest-path aimp_node/Cargo.toml

# Runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates iproute2 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/aimp_node /usr/local/bin/aimp_node

EXPOSE 1337/udp 9090/tcp

ENTRYPOINT ["aimp_node"]
CMD ["--headless", "--port", "1337"]
