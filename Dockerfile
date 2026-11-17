# syntax=docker/dockerfile:1

# ---- build: cross-compile a static Go binary --------------------------------
# Runs on the native build platform; Go cross-compiles to the target with
# CGO off, so no qemu and no C toolchain.
# No Go version tag: the image's Go is used when it is at least go.mod's
# `toolchain` line; otherwise GOTOOLCHAIN=auto fetches that line (auto only
# moves up). Keep the `bookworm` suffix: a bare tag resolves to trixie, a
# silent Debian major bump.
FROM --platform=$BUILDPLATFORM golang:bookworm AS build

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
# openssh-client is what git runs for ssh remotes, and ca-certificates is what
# it verifies HTTPS remotes against. gnupg is left out: as before, commits made
# in the container are not signed.
#
# The digest pins the image; dependabot's docker ecosystem moves it.
#
# Set NODA_LOG_LEVEL (debug, info, warn or error; default info) with
# `docker run -e`. Do not set RUST_LOG: noda refuses to start while it exists.
FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
RUN apt-get update \
    && apt-get install -y --no-install-recommends git openssh-client ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/noda /noda

# noda honours XDG on every platform, so HOME on the volume puts notebooks,
# config and state under one mount.
ENV HOME=/data
VOLUME /data
WORKDIR /data

# Exec form, so `noda` is PID 1 and receives `docker stop`'s SIGTERM: `noda web`
# then waits for a running sync instead of being SIGKILLed with git's
# `index.lock` left behind. git's child processes are reaped by git itself.
ENTRYPOINT ["/noda"]
