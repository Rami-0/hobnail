# Dev/test image used by scripts/cargo.sh (no Rust toolchain on the host).
FROM rust:1.100-alpine
RUN apk add --no-cache musl-dev git && rustup component add rustfmt clippy
WORKDIR /src
