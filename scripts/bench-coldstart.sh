#!/bin/sh
# Cold-start benchmark. For a note-taking CLI the dominant cost of a quick
# `noda ls` is process startup, not the work itself, so we measure whole
# processes rather than in-process code.
#
#   scripts/bench-coldstart.sh [binary ...]      (default: target/release/noda)
#   RUNS=500 scripts/bench-coldstart.sh
#
# `/usr/bin/true` is measured too: that is the floor this shell loop can reach,
# so subtract it to read the binary's own startup cost.
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
XDG_CONFIG_HOME="$root/config"
XDG_DATA_HOME="$root/data"
XDG_STATE_HOME="$root/state"
XDG_CACHE_HOME="$root/cache"
export XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME XDG_CACHE_HOME

echo "runs: $RUNS (after $WARMUP warmup)"
echo
bench "/usr/bin/true (floor)" /usr/bin/true

for bin in "${@:-target/release/noda}"; do
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
