# syntax=docker/dockerfile:1
#
# Container image of rupta: the web app is built with Node, embedded into
# the Rust binary, and the binary runs on slim Debian as an unprivileged user.
#
#   docker build -t rupta .
#   docker run -p 8080:8080 -v rupta-data:/data -e RUPTA_SECRET_KEY=$(openssl rand -hex 32) rupta

# 1. Web app.
FROM node:26-bookworm-slim AS web
WORKDIR /web
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml web/.pnpmfile.cjs ./
RUN --mount=type=cache,target=/root/.local/share/pnpm/store pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# 2. Server. clang is needed by the embedded database (its SQL parser is
#    compiled from C).
FROM rust:1.99.1-slim-bookworm AS server
RUN apt-get update \
    && apt-get install -y --no-install-recommends clang libclang-dev git pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
COPY --from=web /web/dist web/dist
ARG JOBS
ENV CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_NET_GIT_FETCH_WITH_CLI=true
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p rupta-server --bin rupta ${JOBS:+--jobs "$JOBS"} \
    && cp target/release/rupta /usr/local/bin/rupta \
    && strip /usr/local/bin/rupta

# 3. Runtime.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 rupta \
    && useradd --system --uid 10001 --gid rupta --home-dir /data --shell /usr/sbin/nologin rupta \
    && mkdir /data && chown rupta:rupta /data
COPY --from=server /usr/local/bin/rupta /usr/local/bin/rupta
USER rupta:rupta
ENV RUPTA_DATA_DIR=/data RUPTA_LISTEN=0.0.0.0:8080 RUPTA_LOG_FORMAT=json
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=5s --start-period=60s --retries=3 CMD ["rupta", "healthcheck"]
ENTRYPOINT ["rupta"]
CMD ["serve"]

LABEL org.opencontainers.image.title="rupta" \
      org.opencontainers.image.description="Self-hosted error tracking compatible with the official open-source SDKs" \
      org.opencontainers.image.source="https://github.com/michaeldoehler/rupta" \
      org.opencontainers.image.licenses="MIT"
