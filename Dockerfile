# syntax=docker/dockerfile:1
#
# Container image of rupta: the web app is built with Node, embedded into
# the Rust binary, and the binary runs on distroless Debian (glibc, CA
# certificates and tzdata, no shell or package manager) as an unprivileged user.
#
#   docker build -t rupta .
#   docker run -p 8080:8080 -v rupta-data:/data -e RUPTA_SECRET_KEY=$(openssl rand -hex 32) rupta

# 1. Web app.
FROM node:26-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml web/.pnpmfile.cjs ./
# Node 25 and later no longer ship corepack; install the pnpm version that
# package.json pins in `packageManager`.
RUN npm install -g --no-fund --no-audit "$(node -p "require('./package.json').packageManager")"
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
    && strip /usr/local/bin/rupta \
    && mkdir /data-skel

# 3. Runtime. distroless instead of debian-slim: rupta needs glibc, libgcc,
#    CA certificates and tzdata (TLS is rustls), and debian-slim also carried
#    util-linux, PAM, perl, ncurses and gzip, with unfixed CVEs a scanner
#    reports against the image. uid 10001 as before, so existing /data
#    volumes stay readable.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:532c7a94f9e96ba9db75eb9388f4227751131c7cb0a411d1a8521af4096135de
COPY --from=server --chown=10001:10001 /data-skel /data
COPY --from=server /usr/local/bin/rupta /usr/local/bin/rupta
USER 10001:10001
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
