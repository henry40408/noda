#!/bin/sh
# History benchmark: builds a notebook of NOTES notes and about COMMITS
# commits with the given binary, then times the commands that walk history.
# Every call to git costs a process, so these are the commands where a
# per-note or per-commit call would show.
#
#   scripts/bench-history.sh [binary]            (default: bin/noda)
#   NOTES=500 COMMITS=2000 RUNS=3 scripts/bench-history.sh
set -eu

NOTES=${NOTES:-500}
COMMITS=${COMMITS:-2000}
RUNS=${RUNS:-3}
bin=${1:-bin/noda}
case $bin in /*) ;; *) bin=$PWD/$bin ;; esac

now() {
    perl -MTime::HiRes=time -e 'printf "%.6f\n", time'
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
printf '[user]\n\tname = Bench\n\temail = bench@example.com\n[init]\n\tdefaultBranch = main\n' >"$GIT_CONFIG_GLOBAL"
export HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME XDG_CACHE_HOME GIT_CONFIG_GLOBAL GIT_CONFIG_NOSYSTEM

"$bin" init >/dev/null
"$bin" config sign false >/dev/null

echo "building $NOTES notes and about $COMMITS commits"
first=
i=0
while [ "$i" -lt "$NOTES" ]; do
    out=$("$bin" add "Note $i" -c "body of note $i")
    [ -n "$first" ] || first=${out%% *}
    i=$((i + 1))
done
extra=$((COMMITS - NOTES))
i=0
while [ "$i" -lt "$extra" ]; do
    "$bin" tag "$first" "+t$i" >/dev/null
    "$bin" tag "$first" "-t$i" >/dev/null
    i=$((i + 2))
done
# Deleted notes give `deleted` something to find.
for n in 1 2 3 4 5; do
    id=$("$bin" add "Gone $n" -c "x")
    "$bin" rm "${id%% *}" >/dev/null
done

# bench <label> <command> [args...]
bench() {
    label=$1
    shift
    start=$(now)
    i=0
    while [ "$i" -lt "$RUNS" ]; do
        "$@" >/dev/null 2>&1 || true
        i=$((i + 1))
    done
    end=$(now)
    perl -e 'printf "  %-24s %9.1f ms\n", $ARGV[0], ($ARGV[2] - $ARGV[1]) * 1000 / $ARGV[3]' \
        -- "$label" "$start" "$end" "$RUNS"
}

echo "runs: $RUNS"
bench "ls" "$bin" ls
bench "status" "$bin" status
bench "log" "$bin" log
bench "log <note>" "$bin" log "$first"
bench "blame <note>" "$bin" blame "$first"
bench "deleted" "$bin" deleted
