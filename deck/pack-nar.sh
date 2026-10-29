#!/bin/sh
# Build a HashWatcher .nar for Display Widgets → Install Widget on the Deck.
#
# Requires a built hashwatcher.wasm. Uses nix-store --dump when available,
# otherwise deck/dump-nar.py (Python 3).
#
# Usage:
#   deck/pack-nar.sh path/to/hashwatcher.wasm [output.nar]
#
# Default output: dist/hashwatcher.nar
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

WASM=${1:?usage: pack-nar.sh <wasm-path> [output.nar]}
OUT=${2:-"$REPO_ROOT/dist/hashwatcher.nar"}

[ -f "$WASM" ] || {
	echo "wasm missing: $WASM" >&2
	exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
root="$work/package-root"
mkdir -p "$root"

"$SCRIPT_DIR/assemble-package.sh" "$root" "$WASM" >/dev/null

mkdir -p "$(dirname "$OUT")"
if command -v nix-store >/dev/null 2>&1; then
	nix-store --dump "$root" >"$OUT"
else
	python3 "$SCRIPT_DIR/dump-nar.py" "$root" "$OUT"
fi

bytes=$(wc -c <"$OUT" | tr -d ' ')
echo "Wrote $OUT ($bytes bytes)"
echo "Install on the Deck: Display Widgets → Install Widget → choose this .nar"
