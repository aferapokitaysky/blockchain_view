FROM rust:1.93-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY apps/api ./apps/api
RUN cargo build --release --locked -p tracefold-api --bin tracefold-api --bin sync_worker

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home tracefold
COPY --from=builder /app/target/release/tracefold-api /usr/local/bin/tracefold-api
COPY --from=builder /app/target/release/sync_worker /usr/local/bin/sync_worker
USER tracefold
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/tracefold-api"]
