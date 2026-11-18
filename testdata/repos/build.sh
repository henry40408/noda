#!/bin/sh
# Builds one deterministic notebook scenario with nothing but git.
#
#   build.sh <out-dir> <scenario>
#
# Everything lives under <out-dir>: HOME, the four XDG roots, and for the remote
# scenarios a bare repository and a second clone. Dates, authors and note ids are
# fixed, so every object id is the same on every machine; the Rust and Go
# implementations are compared on the same bytes.
#
# `sign = false` in noda's config and `commit.gpgsign = false` in git's: the
# fixture must not read a developer's signing setup.
set -eu

out=${1:?usage: build.sh <out-dir> <scenario>}
scenario=${2:?usage: build.sh <out-dir> <scenario>}

# This script writes $out/home/.gitconfig. Pointing it at the real home directory
# would replace the developer's git configuration.
real_home=$(cd "${HOME:-/nonexistent}" 2>/dev/null && pwd -P) || real_home=
if [ -n "$real_home" ] && [ "$(mkdir -p "$out" && cd "$out" && pwd -P)" = "$real_home" ]; then
	echo "build.sh: <out-dir> must not be the real home directory" >&2
	exit 1
fi

mkdir -p "$out/home" "$out/config/noda" "$out/data/noda/notebooks" "$out/state/noda" "$out/cache/noda"
out=$(cd "$out" && pwd)

export HOME="$out/home"
export XDG_CONFIG_HOME="$out/config" XDG_DATA_HOME="$out/data" XDG_STATE_HOME="$out/state" XDG_CACHE_HOME="$out/cache"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="$out/home/.gitconfig" TZ=UTC LC_ALL=C
export GIT_TERMINAL_PROMPT=0

printf '[user]\n\tname = Fixture Author\n\temail = fixture@example.com\n[init]\n\tdefaultBranch = main\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n' >"$HOME/.gitconfig"
printf 'sign = false\n' >"$out/config/noda/config.toml"

nb="$out/data/noda/notebooks/default"
clock=0

# Every commit and tag is an hour after the last.
tick() {
	clock=$((clock + 1))
	stamp="$((1700000000 + clock * 3600)) +0000"
	export GIT_AUTHOR_DATE="$stamp" GIT_COMMITTER_DATE="$stamp"
}

# commit <dir> <message>: everything in the working tree.
commit() {
	tick
	git -C "$1" add --all
	git -C "$1" commit --quiet --no-verify -m "$2"
}

# note <dir> <file> <title> <tags> <created> <updated> <body>
note() {
	{
		printf -- '---\ntitle: %s\n' "$3"
		[ -z "$4" ] || printf 'tags: [%s]\n' "$4"
		printf 'created: %s\nupdated: %s\n---\n\n%s\n' "$5" "$6" "$7"
	} >"$1/$2"
}

new_notebook() {
	git init --quiet "$1"
	tick
	git -C "$1" commit --quiet --allow-empty --no-verify -m 'chore: initialize notebook'
}

N1=01abc234-budget-review.md
N1R=01abc234-q3-budget-review.md
N2=02def567-meeting-notes.md
N3=03ghj890-reading-list.md
N4=04kmn123-fresh-idea.md

body_budget='the q3 budget is late

- first line
- second line
- third line'

body_budget_edited='the q3 budget is late again

- first line
- second line changed
- third line
- fourth line'

body_meeting='# Agenda

the budget, again

see [the budget](01abc234-budget-review.md)'

body_reading='a book
another book'

linear() {
	new_notebook "$nb"
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-01T00:00:00Z "$body_budget"
	commit "$nb" 'add: Budget review'
	note "$nb" $N2 'Meeting notes' 'work, q3' 2024-01-02T00:00:00Z 2024-01-02T00:00:00Z "$body_meeting"
	commit "$nb" 'add: Meeting notes'
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-03T00:00:00Z "$body_budget_edited"
	commit "$nb" 'edit: Budget review'
	git -C "$nb" mv $N1 $N1R
	note "$nb" $N1R 'Q3 budget review' 'work' 2024-01-01T00:00:00Z 2024-01-04T00:00:00Z "$body_budget_edited"
	commit "$nb" 'mv: Q3 budget review'
	git -C "$nb" rm --quiet $N2
	commit "$nb" 'rm: Meeting notes'
	note "$nb" $N2 'Meeting notes' 'work, q3' 2024-01-02T00:00:00Z 2024-01-02T00:00:00Z "$body_meeting"
	commit "$nb" 'restore: Meeting notes'
	note "$nb" $N3 'Reading list' '' 2024-01-05T00:00:00Z 2024-01-05T00:00:00Z "$body_reading"
	commit "$nb" 'add: Reading list'
	note "$nb" $N1R 'Q3 budget review' 'work, urgent' 2024-01-01T00:00:00Z 2024-01-06T00:00:00Z "$body_budget_edited"
	commit "$nb" 'tag: Q3 budget review'
	git -C "$nb" rm --quiet $N3
	commit "$nb" 'rm: Reading list'
}

merge() {
	new_notebook "$nb"
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-01T00:00:00Z "$body_budget"
	note "$nb" $N2 'Meeting notes' 'work' 2024-01-02T00:00:00Z 2024-01-02T00:00:00Z "$body_meeting"
	commit "$nb" 'add: two notes'
	git -C "$nb" checkout --quiet -b side
	note "$nb" $N2 'Meeting notes' 'work' 2024-01-02T00:00:00Z 2024-01-03T00:00:00Z "$body_meeting
- a line from the side branch"
	commit "$nb" 'edit: Meeting notes on side'
	git -C "$nb" checkout --quiet main
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-04T00:00:00Z "$body_budget_edited"
	commit "$nb" 'edit: Budget review on main'
	tick
	git -C "$nb" merge --quiet --no-ff --no-verify -m 'merge: side' side
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-05T00:00:00Z "$body_budget_edited
- after the merge"
	commit "$nb" 'edit: Budget review after merge'
}

dirty() {
	new_notebook "$nb"
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-01T00:00:00Z "$body_budget"
	commit "$nb" 'add: Budget review'
	note "$nb" $N2 'Meeting notes' 'work' 2024-01-02T00:00:00Z 2024-01-02T00:00:00Z "$body_meeting"
	commit "$nb" 'add: Meeting notes'
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-09T00:00:00Z "$body_budget_edited"
	note "$nb" $N4 'Fresh idea' '' 2024-01-09T00:00:00Z 2024-01-09T00:00:00Z 'not committed yet'
	printf 'not really a png\n' >"$nb/diagram.png"
}

# A rename that rewrites almost everything: where following by id and following
# by content part ways.
rename_rewrite() {
	new_notebook "$nb"
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-01T00:00:00Z "alpha
bravo
charlie
delta
echo
foxtrot
golf
hotel
india
juliet
kilo
lima"
	commit "$nb" 'add: Budget review'
	git -C "$nb" mv $N1 $N1R
	note "$nb" $N1R 'Q3 budget review' 'work' 2024-01-01T00:00:00Z 2024-01-02T00:00:00Z "one
two
three
four
five
six
golf
seven
eight
nine
ten
lima"
	commit "$nb" 'mv: Q3 budget review, rewritten'
	note "$nb" $N1R 'Q3 budget review' 'work' 2024-01-01T00:00:00Z 2024-01-03T00:00:00Z "one
two
three
four
five
six
golf
seven
eight
nine
ten
lima
eleven"
	commit "$nb" 'edit: Q3 budget review'
}

# The notebook is a clone of a bare remote, and a second clone writes to the
# same remote, so the remote can be ahead of what the notebook has seen.
remote_pair() {
	git init --quiet --bare "$out/remote.git"
	url="file://$out/remote.git"
	new_notebook "$nb"
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-01T00:00:00Z "$body_budget"
	note "$nb" $N2 'Meeting notes' 'work' 2024-01-02T00:00:00Z 2024-01-02T00:00:00Z "$body_meeting"
	commit "$nb" 'add: two notes'
	git -C "$nb" remote add origin "$url"
	git -C "$nb" push --quiet --no-verify origin main
	git clone --quiet "$url" "$out/other"
}

other_commit() { # <message>: the second clone pushes whatever is in its tree
	commit "$out/other" "$1"
	git -C "$out/other" push --quiet --no-verify origin main
}

diverged_clean() {
	remote_pair
	note "$out/other" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-07T00:00:00Z "$body_budget_edited"
	other_commit 'edit: Budget review elsewhere'
	note "$nb" $N2 'Meeting notes' 'work' 2024-01-02T00:00:00Z 2024-01-08T00:00:00Z "$body_meeting
- a local line"
	commit "$nb" 'edit: Meeting notes here'
}

diverged_conflict() {
	remote_pair
	note "$out/other" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-07T00:00:00Z "the q3 budget is on time

- first line
- second line
- third line"
	other_commit 'edit: Budget review elsewhere'
	note "$nb" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-08T00:00:00Z "the q3 budget is cancelled

- first line
- second line
- third line"
	commit "$nb" 'edit: Budget review here'
}

ahead() {
	remote_pair
	note "$nb" $N3 'Reading list' '' 2024-01-05T00:00:00Z 2024-01-05T00:00:00Z "$body_reading"
	commit "$nb" 'add: Reading list'
	note "$nb" $N3 'Reading list' 'books' 2024-01-05T00:00:00Z 2024-01-06T00:00:00Z "$body_reading"
	commit "$nb" 'tag: Reading list'
	tick
	git -C "$nb" tag -a -m 'before the books' before-books HEAD~1
}

behind() {
	remote_pair
	note "$out/other" $N1 'Budget review' 'work' 2024-01-01T00:00:00Z 2024-01-07T00:00:00Z "$body_budget_edited"
	other_commit 'edit: Budget review elsewhere'
	note "$out/other" $N1 'Budget review' 'work, urgent' 2024-01-01T00:00:00Z 2024-01-08T00:00:00Z "$body_budget_edited"
	other_commit 'tag: Budget review elsewhere'
}

# A local tag the remote lacks, one it already has, and one it has at another commit.
tags() {
	remote_pair
	note "$nb" $N3 'Reading list' '' 2024-01-05T00:00:00Z 2024-01-05T00:00:00Z "$body_reading"
	commit "$nb" 'add: Reading list'
	tick
	git -C "$nb" tag -a -m 'only here' local-only HEAD
	tick
	git -C "$nb" tag -a -m 'on both' shared HEAD~1
	git -C "$nb" push --quiet --no-verify origin shared
	tick
	git -C "$nb" tag -a -m 'here it is the new commit' clash HEAD
	tick
	git -C "$out/other" fetch --quiet origin
	git -C "$out/other" tag -a -m 'there it is the old commit' clash HEAD
	git -C "$out/other" push --quiet --no-verify origin clash
}

case "$scenario" in
linear) linear ;;
merge) merge ;;
dirty) dirty ;;
rename-rewrite) rename_rewrite ;;
diverged-clean) diverged_clean ;;
diverged-conflict) diverged_conflict ;;
ahead) ahead ;;
behind) behind ;;
tags) tags ;;
*)
	echo "unknown scenario: $scenario" >&2
	exit 2
	;;
esac

printf 'default\n' >"$out/state/noda/active"
