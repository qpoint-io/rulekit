#!/bin/sh
# Fails if regenerating src/regex/unicode_names.rs changes it. Needs the Go
# version pinned in the repository's go.mod (Unicode tables follow Go).
set -eu
cd "$(dirname "$0")/unicode_names"
want="go$(sed -n 's/^go //p' ../../../go.mod)"
have="$(go env GOVERSION)"
[ "$have" = "$want" ] || { echo "need $want, have $have" >&2; exit 1; }
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
go run . > "$tmp"
cmp -s "$tmp" ../../src/regex/unicode_names.rs || { echo "unicode_names.rs is stale; regenerate it" >&2; exit 1; }
echo "unicode_names.rs is up to date ($have)"
