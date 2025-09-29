# Multi-stage build for Rust application
FROM rust:1.70 as builder

WORKDIR /app

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm src/main.rs

# Copy source code and build the application
COPY src ./src
COPY config.json.example ./config.json

# Build the application with release optimizations
RUN cargo build --release

# Runtime stage with minimal image
FROM debian:bookworm-slim

# Install CA certificates for HTTPS requests
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN groupadd -r keycycle && useradd -r -g keycycle keycycle

# Copy the compiled binary from builder stage
COPY --from=builder /app/target/release/key-cycle-proxy /usr/local/bin/key-cycle-proxy

# Set ownership and permissions
RUN chown keycycle:keycycle /usr/local/bin/key-cycle-proxy && \
    chmod +x /usr/local/bin/key-cycle-proxy

# Switch to non-root user
USER keycycle

# Expose port (default Rust server port)
EXPOSE 8080

# Set environment variables
ENV RUST_LOG=info

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the application
CMD ["key-cycle-proxy"]
