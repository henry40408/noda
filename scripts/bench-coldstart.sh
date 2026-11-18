#!/bin/sh
# Cold-start benchmark: a quick `noda ls` is mostly process startup, so this
# times whole processes.
#
#   go build -mod=vendor -o bin/noda ./cmd/noda
#   scripts/bench-coldstart.sh [binary ...]      (default: bin/noda)
#   RUNS=500 scripts/bench-coldstart.sh
#
# Subtract the `/usr/bin/true` floor to read the binary's own startup cost.
set -eu

RUNS=${RUNS:-200}
WARMUP=${WARMUP:-20}

now() {
    perl -MTime::HiRes=time -e 'printf "%.6f\n", time'
}

# bench <label> <command> [args...]
bench() {
    label=$1
    shift
    i=0
    while [ "$i" -lt "$WARMUP" ]; do
        "$@" >/dev/null 2>&1 || true
        i=$((i + 1))
    done
    start=$(now)
    i=0
    while [ "$i" -lt "$RUNS" ]; do
        "$@" >/dev/null 2>&1 || true
        i=$((i + 1))
    done
    end=$(now)
    # `--` keeps a label like "--version" from being read as a perl switch.
    perl -e 'printf "  %-24s %7.3f ms\n", $ARGV[0], ($ARGV[2] - $ARGV[1]) * 1000 / $ARGV[3]' \
        -- "$label" "$start" "$end" "$RUNS"
}

root=$(mktemp -d "${TMPDIR:-/tmp}/noda-bench.XXXXXX")
trap 'rm -rf "$root"' EXIT
# Nothing here may read or write the developer's own home or git config.
HOME="$root/home"
XDG_CONFIG_HOME="$root/config"
XDG_DATA_HOME="$root/data"
XDG_STATE_HOME="$root/state"
XDG_CACHE_HOME="$root/cache"
GIT_CONFIG_GLOBAL="$root/gitconfig"
GIT_CONFIG_NOSYSTEM=1
mkdir -p "$HOME"
printf '[user]\n\tname = Bench\n\temail = bench@example.com\n[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n' >"$GIT_CONFIG_GLOBAL"
export HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME XDG_CACHE_HOME GIT_CONFIG_GLOBAL GIT_CONFIG_NOSYSTEM

echo "runs: $RUNS (after $WARMUP warmup)"
echo
bench "/usr/bin/true (floor)" /usr/bin/true

for bin in "${@:-bin/noda}"; do
    size=$(wc -c <"$bin" | tr -d ' ')
    echo
    echo "$bin ($((size / 1024)) KiB)"
    bench "--version" "$bin" --version

    rm -rf "$root"/config "$root"/data "$root"/state "$root"/cache
    if "$bin" init >/dev/null 2>&1; then
        "$bin" add "Bench note" -c "body" >/dev/null 2>&1 || true
        bench "ls (1 note)" "$bin" ls
    fi
done
