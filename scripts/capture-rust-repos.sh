#!/bin/zsh
# Records what the Rust noda prints on each scenario of testdata/repos/build.sh
# into testdata/golden/repos/<scenario>/<case>.txt.
#
#   scripts/capture-rust-repos.sh <path-to-rust-noda-binary>
#
# Run it twice and `git diff --exit-code testdata/golden/repos` must stay empty:
# that is what shows the capture is deterministic. The Go tests compare against
# these files and must never edit them.
#
# Every process (the fixture build and noda) runs under `env -i`, so a
# developer's GIT_*, TZ or locale cannot leak into the recording. stdout goes to
# a file, not to `$(...)`, which would eat trailing newlines and hide a
# difference in them.
set -eu

BIN=${1:?usage: capture-rust-repos.sh <path-to-rust-noda-binary>}
BIN=$(cd "$(dirname "$BIN")" && pwd -P)/$(basename "$BIN")
W=$(cd "$(dirname "$0")/.." && pwd -P)
SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/noda-capture.XXXXXX")
trap 'rm -rf "$SCRATCH"' EXIT
OUTROOT=$W/testdata/golden/repos
rm -rf "$OUTROOT"
mkdir -p "$OUTROOT"

# scenario|case-name|noda args. A second command follows after ' ;; '.
# `@sha:<subject>` is the id of the commit with that subject, underscores
# standing for spaces; `restore` needs one.
CASES=(
'linear|log|log'
'linear|log-2|log -n 2'
'linear|log-note|log budget-review'
'linear|log-note-id|log 01abc234'
'linear|log-deleted-note|log reading-list'
'linear|blame|blame meeting-notes'
'linear|blame-renamed|blame 01abc234'
'linear|diff|diff'
'linear|diff-note|diff meeting-notes'
'linear|deleted|deleted'
'linear|deleted-json|deleted --json'
'linear|status|status'
'linear|ls|ls'
'linear|ls-json|ls --json'
'linear|todo|todo'
'linear|backlinks|backlinks 01abc234'
'linear|backlinks-json|backlinks 01abc234 --json'
'linear|search|search budget'
'linear|doctor|doctor'
'linear|doctor-links|doctor --links'
'linear|snapshot-list|snapshot'
'linear|snapshot-make|snapshot 2024-q1 ;; snapshot'
'linear|restore-deleted|restore reading-list @sha:add:_Reading_list ;; status'
'linear|tag-add|tag meeting-notes +new -q3 --no-touch ;; show meeting-notes'
'linear|rm|rm meeting-notes ;; status'
'linear|pin|pin meeting-notes ;; unpin meeting-notes'
'merge|log|log'
'merge|log-2|log -n 3'
'merge|blame|blame budget-review'
'merge|diff|diff'
'merge|status|status'
'dirty|status|status'
'dirty|diff|diff'
'dirty|diff-note|diff budget-review'
'dirty|blame-uncommitted|blame budget-review'
'dirty|ls|ls'
'dirty|doctor|doctor'
'dirty|file-ls|file ls'
'dirty|sync|sync ;; status'
'rename-rewrite|log-note|log budget-review'
'rename-rewrite|log-id|log 01abc234'
'rename-rewrite|blame|blame 01abc234'
'rename-rewrite|diff|diff'
'diverged-clean|status|status'
'diverged-clean|diff-remote|diff --remote'
'diverged-clean|pull|pull ;; status'
'diverged-clean|sync|sync ;; status'
'diverged-clean|push|push'
'diverged-conflict|status|status'
'diverged-conflict|pull|pull'
'diverged-conflict|sync|sync'
'diverged-conflict|diff-remote|diff --remote'
'ahead|status|status'
'ahead|diff-remote|diff --remote'
'ahead|push|push ;; status'
'ahead|sync|sync ;; status'
'ahead|snapshot-list|snapshot'
'behind|status|status'
'behind|diff-remote|diff --remote'
'behind|pull|pull ;; log'
'behind|sync|sync ;; log'
'tags|status|status'
'tags|snapshot-list|snapshot'
'tags|push|push'
'tags|sync|sync'
)

# noda_in <dir> <args...>: noda with nothing but the fixture's own environment.
noda_in() {
  local D=$1; shift
  env -i HOME=$D/home PATH=$PATH TZ=UTC LC_ALL=C \
    XDG_CONFIG_HOME=$D/config XDG_DATA_HOME=$D/data XDG_STATE_HOME=$D/state XDG_CACHE_HOME=$D/cache \
    GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=$D/home/.gitconfig "$BIN" "$@"
}

# resolve_shas <dir> <args>: replaces each @sha:<subject> by that commit's id.
resolve_shas() {
  local D=$1 args=$2 token subject
  local -a words
  for token in ${=args}; do
    if [[ $token != @sha:* ]]; then
      words+=("$token")
      continue
    fi
    subject=${${token#@sha:}//_/ }
    words+=("$(env -i PATH=$PATH GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
      git -C "$D/data/noda/notebooks/default" log --format=%H --fixed-strings --grep="$subject" -n 1)")
  done
  echo "${words[*]}"
}

# Object ids become <OID:n> by first appearance and the scratch root becomes
# <ROOT>. An all-zero id is not an object: blame prints it for an uncommitted line.
normalize() {
  python3 -I - "$1" "$2" <<'PY'
import re, sys
path, root = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read().replace(root, "<ROOT>")
seen = {}
def sub(m):
    key = m.group(0)
    if set(key) == {"0"}:
        return key
    for k in seen:
        if k.startswith(key) or key.startswith(k):
            return "<OID:%d>" % seen[k]
    seen[key] = len(seen) + 1
    return "<OID:%d>" % seen[key]
text = re.sub(r"(?<![0-9A-Za-z-])(?:[0-9a-f]{40}|[0-9a-f]{7})(?![0-9A-Za-z-])", sub, text)
open(path, "w", encoding="utf-8").write(text)
PY
}

record() {
  local scenario=$1 name=$2 args=$3 D one command code
  D=$SCRATCH/run
  rm -rf "$D"
  env -i PATH=$PATH sh "$W/testdata/repos/build.sh" "$D" "$scenario" >/dev/null 2>&1 || { echo "build failed: $scenario" >&2; return 1; }
  D=$(cd "$D" && pwd -P)
  mkdir -p "$OUTROOT/$scenario"
  local file=$OUTROOT/$scenario/$name.txt
  : > "$file"
  for one in "${(@s: ;; :)args}"; do
    command=$(resolve_shas "$D" "$one")
    code=0
    noda_in "$D" ${=command} >"$D/stdout" 2>"$D/stderr" || code=$?
    {
      echo "\$ noda $command"
      cat "$D/stdout"
      if [[ -s "$D/stdout" && $(tail -c 1 "$D/stdout" | wc -l) -eq 0 ]]; then echo; fi
      echo "--- stderr"
      cat "$D/stderr"
      echo "--- exit $code"
    } >> "$file"
  done
  normalize "$file" "$D"
}

for line in "${CASES[@]}"; do
  scenario=${line%%|*}; rest=${line#*|}
  record "$scenario" "${rest%%|*}" "${rest#*|}"
done
echo done
