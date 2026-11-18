# syntax=docker/dockerfile:1

# ---- build: cross-compile a static Go binary --------------------------------
# Runs on the native build platform; Go cross-compiles to the target with
# CGO off, so no qemu and no C toolchain.
# Pinned by version and digest (pushed 2026-11-10, past the 168-hour cooldown);
# dependabot's docker ecosystem moves both. GOTOOLCHAIN=auto below fetches
# go.mod's `toolchain` line when it is newer than this image's Go. Keep the
# `bookworm` suffix: a bare tag resolves to trixie, a silent Debian major bump.
FROM --platform=$BUILDPLATFORM golang:1.26.11-bookworm@sha256:dc40a13b61304647ed7e5d581d246b86413dfc29d63c6d018310feb93cd98fc0 AS build

WORKDIR /app

# The official image sets GOTOOLCHAIN=local, which ignores go.mod's toolchain
# line. Fetch a newer toolchain in a layer keyed on go.mod/go.sum alone, so
# source edits do not re-download it.
ENV GOTOOLCHAIN=auto
COPY go.mod go.sum ./
RUN go version

COPY . .

# `vendor/` ships in the repository, so the build needs no network. The
# workflow passes the version in, because `.dockerignore` excludes `.git`.
ARG TARGETOS
ARG TARGETARCH
ARG NODA_VERSION=dev
RUN --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS="$TARGETOS" GOARCH="$TARGETARCH" \
    go build -mod=vendor -trimpath \
      -ldflags "-s -w -X github.com/henry40408/noda/internal/version.version=${NODA_VERSION}" \
      -o /out/noda ./cmd/noda

# ---- runtime: Debian slim with git and ssh ----------------------------------
# Not distroless: noda runs the system `git` for every repository operation, so
# git is a runtime dependency (bookworm ships 2.39.5, noda's minimum).
# openssh-client is what git runs for ssh remotes, ca-certificates is what
# it verifies HTTPS remotes against, and tini is PID 1. gnupg is left out: as before, commits made
# in the container are not signed.
#
# The digest pins the image; dependabot's docker ecosystem moves it.
#
# Set NODA_LOG_LEVEL (debug, info, warn or error; default info) with
# `docker run -e`. The Rust build's logging variable is retired; noda refuses to
# start while it is set.
FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
RUN apt-get update \
    && apt-get install -y --no-install-recommends git openssh-client ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/noda /noda

# noda honours XDG on every platform, so HOME on the volume puts notebooks,
# config and state under one mount.
ENV HOME=/data
VOLUME /data
WORKDIR /data

# tini is PID 1: it forwards `docker stop`'s SIGTERM to noda, so `noda web`
# waits for a running sync instead of being SIGKILLed with git's `index.lock`
# left behind, and it reaps the grandchildren (ssh, credential helpers) that
# are re-parented to PID 1 when their git exits or is killed. noda only waits
# for its direct children.
ENTRYPOINT ["/usr/bin/tini", "--", "/noda"]
