# syntax=docker/dockerfile:1
# linux/arm64 image, cross-compiled on the build machine: scripts/build-arm64.sh.
# The web stage, release profile, link flags, SQLite flags, labels and final
# layout match the production Dockerfile; only the Rust step differs (clang +
# lld against the target's musl sysroot instead of an emulated rustc, which
# takes hours under QEMU). fmt, clippy and the tests run in the amd64
# production build of the same commit, so they are not repeated here. The
# production Dockerfile is not used for this and is not changed by it.

FROM --platform=$BUILDPLATFORM node:26-alpine@sha256:6bd9ab43c0ec0190ca7b2d193906cb8b7edf6e7f4afa43ddeccd6452710bffb5 AS web
WORKDIR /web
COPY web/package.json web/package-lock.json web/.npmrc ./
RUN --mount=type=cache,id=hobnail-npm,target=/root/.npm npm ci --ignore-scripts
COPY web/ ./
RUN npm run check && npm run test && npm run build && npm run size

# The target's own musl headers and libraries and its static busybox (one
# short emulated step: apk only).
FROM rust:1.100-alpine@sha256:de1e0058543a81ed7b046629167db587c67a94bdf37cce14eb27f0ec83baaffb AS arm-packages
RUN apk add --no-cache musl-dev busybox-static

FROM --platform=$BUILDPLATFORM rust:1.100-alpine@sha256:de1e0058543a81ed7b046629167db587c67a94bdf37cce14eb27f0ec83baaffb AS cross
RUN apk add --no-cache clang lld musl-dev
RUN rustup target add aarch64-unknown-linux-musl \
 && printf '#!/bin/sh\nexec clang --target=aarch64-linux-musl --sysroot=/arm64 -fuse-ld=lld "$@"\n' > /usr/local/bin/arm64-link \
 && chmod +x /usr/local/bin/arm64-link
COPY --from=arm-packages /usr/include /arm64/usr/include
COPY --from=arm-packages /usr/lib /arm64/usr/lib
COPY --from=arm-packages /lib /arm64/lib
COPY --from=arm-packages /bin/busybox.static /arm64/bin/busybox.static
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY docs/api.md ./docs/api.md
COPY web/src/api/gen ./web/src/api/gen
COPY web/src/format/fixtures.json ./web/src/format/fixtures.json
COPY --from=web /web/dist ./web/dist
ARG RH_GIT_SHA=dev
ARG RH_BUILD_TIME=unknown
ENV RH_GIT_SHA=${RH_GIT_SHA} RH_BUILD_TIME=${RH_BUILD_TIME} RH_REQUIRE_WEB_DIST=1 CARGO_TARGET_DIR=/target
ENV LIBSQLITE3_FLAGS="-USQLITE_ENABLE_FTS3 -USQLITE_ENABLE_FTS3_PARENTHESIS -USQLITE_ENABLE_FTS5 -USQLITE_ENABLE_RTREE -USQLITE_ENABLE_DBSTAT_VTAB -DSQLITE_OMIT_LOAD_EXTENSION"
ENV CC_aarch64_unknown_linux_musl=clang \
    CFLAGS_aarch64_unknown_linux_musl="--target=aarch64-linux-musl --sysroot=/arm64" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=/usr/local/bin/arm64-link
# Its own cache ids, so it never shares a target directory with the amd64 build.
RUN --mount=type=cache,id=rh-arm64-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=rh-arm64-target,target=/target,sharing=locked \
    export RH_CONTENT_HASH="$(find src web/dist web/src/api/gen web/src/format/fixtures.json build.rs Cargo.toml Cargo.lock -type f \
        | LC_ALL=C sort | while IFS= read -r f; do printf '%s\n' "$f"; sha256sum < "$f"; done \
        | sha256sum | cut -c1-64)" \
 && echo "content hash $RH_CONTENT_HASH" && echo "$RH_CONTENT_HASH" > /content-hash \
 && cargo rustc --locked --release --target aarch64-unknown-linux-musl -- -Clink-arg=-Wl,-z,pack-relative-relocs \
 && mkdir -p /rootfs/bin /rootfs/tmp \
 && cp /target/aarch64-unknown-linux-musl/release/hobnail /rootfs/hobnail \
 && cp /arm64/bin/busybox.static /rootfs/bin/busybox \
 && ln -s busybox /rootfs/bin/sh

# The stale-binary guard of the production build, run on the target platform
# (emulated, a fraction of a second): the binary must report this commit and
# content hash.
FROM scratch AS verified
COPY --from=cross /rootfs/ /
COPY --from=cross /content-hash /content-hash
ARG RH_GIT_SHA=dev
RUN ["/bin/sh", "-c", "read -r h < /content-hash && v=\"$(/hobnail version)\" && echo \"$v\" && case \"$v\" in *\"($RH_GIT_SHA \"*\"content $h\") ;; *) echo \"stale binary: expected sha $RH_GIT_SHA and content $h\" >&2; exit 1 ;; esac"]

FROM scratch
LABEL org.opencontainers.image.title="Hobnail" \
      org.opencontainers.image.description="Hobnail: a tiny home for your homelab" \
      io.hobnail.build="1"
COPY --from=verified /hobnail /hobnail
COPY --from=verified /bin /bin
COPY --from=verified /tmp /tmp
ENV PATH=/bin RH_DATA_DIR=/data RH_HOST_PROC=/host/proc RH_HOST_SYS=/host/sys
EXPOSE 3200
ENTRYPOINT ["/hobnail"]
CMD ["serve"]
