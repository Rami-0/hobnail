# syntax=docker/dockerfile:1
# Multi-stage build. The build stage runs fmt, clippy and tests, so the same
# checks gate every image. Final image: scratch + one static musl binary
# + a static busybox (sh for command widgets). See docs/measurements.md.
FROM rust:1.100-alpine AS build
RUN apk add --no-cache musl-dev busybox-static && rustup component add rustfmt clippy
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
ARG RH_GIT_SHA=dev
ARG RH_BUILD_TIME=unknown
ENV RH_GIT_SHA=${RH_GIT_SHA} RH_BUILD_TIME=${RH_BUILD_TIME} CARGO_TARGET_DIR=/target
RUN --mount=type=cache,id=rh-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=rh-target,target=/target \
    cargo fmt --check \
 && cargo clippy --locked --all-targets -- -D warnings \
 && cargo test --locked \
 && cargo build --locked --release \
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
