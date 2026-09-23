# syntax=docker/dockerfile:1

# ---- build: cross-compile a static musl binary with cargo-zigbuild ----------
# The builder runs on the native platform and zig cross-compiles, so arm64
# builds at native speed instead of under qemu.
# No Rust version here: rust-toolchain.toml pins it. Not `rust:1.97` either —
# the un-suffixed tag resolves to trixie, a silent Debian major bump.
FROM --platform=$BUILDPLATFORM rust:bookworm AS build

# cmake for libgit2-sys, perl + make for openssl-src, curl + xz for zig.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake curl xz-utils perl make \
    && rm -rf /var/lib/apt/lists/*

# 0.15+ requires libc++-19 for bindgen.
ARG ZIG_VERSION=0.14.1
# 0.23.0+ filters `-Wl,--fix-cortex-a53-843419`, which rustc 1.98 added as a
# default aarch64-unknown-linux-musl linker arg and zig's linker rejects.
# https://github.com/rust-cross/cargo-zigbuild/pull/452
ARG ZIGBUILD_VERSION=0.23.2
RUN cargo install cargo-zigbuild --version "${ZIGBUILD_VERSION}" --locked
RUN set -eux; \
    case "$(uname -m)" in \
      x86_64) zarch=x86_64 ;; \
      aarch64) zarch=aarch64 ;; \
      *) echo "unsupported build arch $(uname -m)" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-${zarch}-linux-${ZIG_VERSION}.tar.xz" \
      | tar -xJ -C /opt; \
    ln -s "/opt/zig-${zarch}-linux-${ZIG_VERSION}/zig" /usr/local/bin/zig

WORKDIR /app

# A layer keyed on rust-toolchain.toml alone, so editing source does not
# re-download the compiler; any rustup proxy call installs it.
COPY rust-toolchain.toml .
RUN cargo --version

COPY . .

ARG TARGETARCH
# build.rs cannot `git describe` here (`.dockerignore` excludes `.git`), so the
# workflow passes the version in; unset falls back to the manifest version.
ARG NODA_VERSION
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target,sharing=locked \
    set -eux; \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported target arch $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    rustup target add "$target"; \
    cargo zigbuild --release --target "$target"; \
    install -Dm755 "target/${target}/release/noda" /out/noda

# ---- runtime: minimal static image (CA certs, no shell) --------------------
# The CA bundle is what the vendored OpenSSL verifies HTTPS `noda sync` against.
FROM gcr.io/distroless/static-debian12
COPY --from=build /out/noda /noda

# noda honours XDG on every platform, so HOME on the volume puts notebooks,
# config and state under one mount.
ENV HOME=/data
VOLUME /data
WORKDIR /data

# Exec form, so `noda` is PID 1 and receives `docker stop`'s SIGTERM: `noda web`
# then waits for a running sync instead of being SIGKILLed with git's
# `index.lock` left behind.
ENTRYPOINT ["/noda"]
