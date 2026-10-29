#!/bin/sh
# Build a HashWatcher .nar for Display Widgets → Install Widget on the Deck.
#
# Requires: nix-store (from Nix), and a built hashwatcher.wasm
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

command -v nix-store >/dev/null 2>&1 || {
	echo "nix-store is required. Install Nix, then re-run." >&2
	exit 1
}
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
# Dump the directory that holds lib/ — that is the package root Braiins expects.
nix-store --dump "$root" >"$OUT"

bytes=$(wc -c <"$OUT" | tr -d ' ')
echo "Wrote $OUT ($bytes bytes)"
echo "Install on the Deck: Display Widgets → Install Widget → choose this .nar"
