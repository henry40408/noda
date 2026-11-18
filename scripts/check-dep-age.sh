#!/bin/sh
# Fails when any Go module the build depends on (indirect ones included) was
# published less than 168 hours ago: a fresh release is the supply-chain attack
# window. Run it in each Go module: the repository root and e2e/.
#
#   scripts/check-dep-age.sh
set -eu

# GNU date first, then BSD date (macOS).
cutoff="$(date -u -d '168 hours ago' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null ||
  date -u -v-168H +%Y-%m-%dT%H:%M:%SZ)"

# RFC 3339 timestamps in one zone sort as text. The main module and replaced
# modules have no published time worth checking.
too_new="$(go list -m -mod=mod -f \
  '{{if and (not .Main) (not .Replace)}}{{.Time}} {{.Path}} {{.Version}}{{end}}' all |
  awk -v cutoff="$cutoff" 'NF == 3 && $1 > cutoff')"

if [ -n "$too_new" ]; then
  echo "modules published after $cutoff (less than 168 hours ago):" >&2
  echo "$too_new" >&2
  exit 1
fi
