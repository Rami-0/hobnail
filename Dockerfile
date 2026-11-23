# syntax=docker/dockerfile:1
# Multi-stage build. The web stage type-checks, tests and builds the React
# client and checks its bundle budgets; the build stage runs fmt, clippy and
# tests and embeds the client, so the same checks gate every image. Final
# image: scratch + one static musl binary + a static busybox (sh for command
# widgets). See docs/measurements.md and docs/plans/react-port.md 4.4.

# scripts/web.sh reads the Node image from the next line; keep its form.
FROM --platform=$BUILDPLATFORM node:26-alpine@sha256:6bd9ab43c0ec0190ca7b2d193906cb8b7edf6e7f4afa43ddeccd6452710bffb5 AS web
WORKDIR /web
COPY web/package.json web/package-lock.json web/.npmrc ./
RUN --mount=type=cache,id=hobnail-npm,target=/root/.npm npm ci --ignore-scripts
COPY web/ ./
RUN npm run check && npm run test && npm run build && npm run size

FROM rust:1.100-alpine@sha256:de1e0058543a81ed7b046629167db587c67a94bdf37cce14eb27f0ec83baaffb AS build
RUN apk add --no-cache musl-dev busybox-static && rustup component add rustfmt clippy
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
# The API reference drift test reads the declared method/path/access rows.
COPY docs/api.md ./docs/api.md
# The Rust drift tests compare the generated TypeScript and the formatting
# fixtures against these.
COPY web/src/api/gen ./web/src/api/gen
COPY web/src/format/fixtures.json ./web/src/format/fixtures.json
COPY --from=web /web/dist ./web/dist
ARG RH_GIT_SHA=dev
ARG RH_BUILD_TIME=unknown
ENV RH_GIT_SHA=${RH_GIT_SHA} RH_BUILD_TIME=${RH_BUILD_TIME} CARGO_TARGET_DIR=/target RH_REQUIRE_WEB_DIST=1
# Hobnail's schemas and queries use neither full-text and spatial indexes nor
# the dbstat virtual table or extension loader. Keep SQLite JSON and STAT4.
ENV LIBSQLITE3_FLAGS="-USQLITE_ENABLE_FTS3 -USQLITE_ENABLE_FTS3_PARENTHESIS -USQLITE_ENABLE_FTS5 -USQLITE_ENABLE_RTREE -USQLITE_ENABLE_DBSTAT_VTAB -DSQLITE_OMIT_LOAD_EXTENSION"
# Stale-binary guard (plan 4.4): cargo decides freshness by mtime, the
# target cache is shared and every build is at /src, so a checkout whose
# files are older than the last build could ship the previous binary.
# RH_CONTENT_HASH (sha256 over every input's path and bytes) is declared by
# build.rs, so a content change always rebuilds; the binary prints it, and
# the build fails unless `hobnail version` shows this hash and RH_GIT_SHA.
# sharing=locked keeps two builds from interleaving between build and cp.
RUN --mount=type=cache,id=rh-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=rh-target,target=/target,sharing=locked \
    export RH_CONTENT_HASH="$(find src web/dist web/src/api/gen web/src/format/fixtures.json build.rs Cargo.toml Cargo.lock -type f \
        | LC_ALL=C sort | while IFS= read -r f; do printf '%s\n' "$f"; sha256sum < "$f"; done \
        | sha256sum | cut -c1-64)" \
 && echo "content hash $RH_CONTENT_HASH" \
 && cargo fmt --check \
 && cargo clippy --locked --all-targets -- -D warnings \
 && cargo test --locked \
 && cargo rustc --locked --release -- -Clink-arg=-Wl,-z,pack-relative-relocs \
 && v="$(/target/release/hobnail version)" && echo "$v" \
 && case "$v" in *"($RH_GIT_SHA "*"content $RH_CONTENT_HASH") ;; \
      *) echo "stale binary: expected sha $RH_GIT_SHA and content $RH_CONTENT_HASH" >&2; exit 1 ;; esac \
 && mkdir -p /rootfs/bin /rootfs/tmp \
 && cp /target/release/hobnail /rootfs/hobnail \
 && cp /bin/busybox.static /rootfs/bin/busybox \
 && ln -s busybox /rootfs/bin/sh

FROM scratch
LABEL org.opencontainers.image.title="Hobnail" \
      org.opencontainers.image.description="Hobnail: a tiny home for your homelab" \
      io.hobnail.build="1"
COPY --from=build /rootfs/ /
ENV PATH=/bin RH_DATA_DIR=/data RH_HOST_PROC=/host/proc RH_HOST_SYS=/host/sys
EXPOSE 3200
ENTRYPOINT ["/hobnail"]
CMD ["serve"]
