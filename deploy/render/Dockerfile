# Stage 1: Build
FROM rust:1.85-slim AS builder

WORKDIR /build

# Copy source
COPY . .

# Build the binary
RUN cargo build --release --bin mpr

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN groupadd --gid 1000 mempalace \
    && useradd --uid 1000 --gid mempalace --shell /bin/false --create-home mempalace

WORKDIR /app

# Copy built binary
COPY --from=builder /build/target/release/mpr /usr/local/bin/mpr

# Ensure binary is executable
RUN chmod +x /usr/local/bin/mpr

# Create data directory
RUN mkdir -p /data && chown mempalace:mempalace /data

# Copy entrypoint script
COPY --chmod=0755 entrypoint.sh /usr/local/bin/entrypoint.sh

USER mempalace

EXPOSE 3111

# Volume for SQLite DB
VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]