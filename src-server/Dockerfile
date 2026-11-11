FROM rust:1.85.0-bookworm AS builder
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev lld clang \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --locked --bin notor

# Runtime stage
FROM debian:bookworm-slim AS runtime
WORKDIR /app
# Install OpenSSL - it is dynamically linked by some of our dependencies
# Install ca-certificates - it is needed to verify TLS certificates
# when establishing HTTPS connections
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl ca-certificates \
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/notor notor
COPY configuration configuration
ENV APP_ENVIRONMENT=production

ENTRYPOINT ["./notor"]
