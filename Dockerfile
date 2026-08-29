FROM node:24-bookworm-slim AS web-builder

ARG PNPM_VERSION=10.34.1

WORKDIR /build
RUN corepack enable && corepack prepare "pnpm@${PNPM_VERSION}" --activate

COPY web/package.json web/pnpm-lock.yaml ./web/
RUN pnpm --dir web install --frozen-lockfile

COPY web/ ./web/
RUN pnpm --dir web build


FROM rust:bookworm AS rust-builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY --from=web-builder /build/web/dist ./web/dist
RUN cargo build --locked --release -p nwm


FROM debian:trixie-slim AS runtime

ARG VERSION=dev
ARG REVISION=unknown

LABEL org.opencontainers.image.title="NUT Web Manager" \
      org.opencontainers.image.description="LAN NUT Server and Client management for Debian, PVE and PBS" \
      org.opencontainers.image.source="https://github.com/guowenju/nut-web-manager" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"

RUN apt-get update \
    && apt-get install -y --no-install-recommends openssh-client \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rust-builder /build/target/release/nwm /usr/local/bin/nwm
COPY --chmod=0755 entrypoint.sh /usr/local/bin/nwm-entrypoint

ENV NWM_BIND_ADDRESS=0.0.0.0:8080 \
    NWM_DATA_DIR=/data \
    RUST_LOG=nwm=info,tower_http=info

VOLUME ["/data"]
EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/nwm-entrypoint"]
CMD ["/usr/local/bin/nwm"]
