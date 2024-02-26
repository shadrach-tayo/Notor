FROM lukemathwalker/cargo-chef:latest-rust-1.75.0 AS chef
WORKDIR /app
RUN apt update && apt install lld clang -y


FROM chef as builder
COPY . .

# Let's build our binary!
# We'll use the release profile to make it faaaast
RUN cargo build --release --bin notor

# Runtime stage
FROM debian:bookworm-slim AS runtime
WORKDIR /app
# Install OpenSSL - it is dynamically linked by some of our dependencies
# Install ca-certificates - it is needed to verify TLS certificates
# when establishing HTTPS connections
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl ca-certificates \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/* \

# Copy the compiled binary from the builder environment
# to our runtime environment
COPY --from=builder /app/target/release/notor notor

#COPY configuration configuration
COPY .env .
COPY configuration configuration
ENV APP_ENVIRONMENT=production

EXPOSE 4876
# When `docker run` is executed, launch the binary!
ENTRYPOINT ["./notor"]