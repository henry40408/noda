# syntax=docker/dockerfile:1

# ---- build: cross-compile a static musl binary with cargo-zigbuild ----------
# The builder stays on the native build platform and zig cross-compiles to the
# target's musl triple, so the arm64 image builds at native speed instead of
# crawling through qemu.
# No Rust version here: rust-toolchain.toml is the single source of truth and
# rustup installs it below. Do not "simplify" this to `rust:1.97` — the
# un-suffixed tag resolves to trixie, which would be a silent Debian major bump.
FROM --platform=$BUILDPLATFORM rust:bookworm AS build

# libgit2-sys drives its C sources through CMake, and openssl-src through
# perl + make. curl fetches zig; xz unpacks it.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake curl xz-utils perl make \
    && rm -rf /var/lib/apt/lists/*

# Zig 0.14.1 avoids the libc++-19 bindgen requirement that 0.15+ introduces.
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

# Install the pinned toolchain in a layer keyed on rust-toolchain.toml alone, so
# editing source does not re-download the compiler. Any rustup proxy invocation
# triggers the install, and the musl targets declared in the file come with it.
COPY rust-toolchain.toml .
RUN cargo --version

COPY . .

# Map Docker's TARGETARCH onto the Rust musl triple and build. `rustup target
# add` runs after the source is in place so it resolves against the toolchain
# rust-toolchain.toml pins, not the base image's default.
ARG TARGETARCH
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
# The CA bundle is not decoration: `noda sync` over HTTPS goes through the
# vendored OpenSSL, which still needs somewhere to verify a certificate chain.
FROM gcr.io/distroless/static-debian12
COPY --from=build /out/noda /noda

# noda honours the XDG variables on every platform, so pointing HOME at the
# volume puts notebooks, config and state under one mount rather than four.
ENV HOME=/data
VOLUME /data
WORKDIR /data

# Exec form, so `noda` is PID 1 and `docker stop` sends it the `SIGTERM` itself
# rather than to a shell that would keep it. That matters for exactly one
# subcommand: `noda web` answers what is in flight and waits for a running
# `sync` before it goes, and an interrupted `sync` is a repository left holding
# git's `index.lock`. Under the shell form the signal would go nowhere, the ten
# seconds would run out, and `SIGKILL` is precisely the ending that leaves it.
ENTRYPOINT ["/noda"]
