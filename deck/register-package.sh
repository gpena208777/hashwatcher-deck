#!/bin/sh
# Register HashWatcher in the Deck package profile.
# The profile is rebuilt from this list when any other widget updates.
# A symlink added under the live profile generation is not on that list, so the next update removes it.
#
# Package layout is the third-party maintainer form used by .nar upload:
#   lib/bmc-widgets/hashwatcher/hashwatcher/
# Package name: widget-hashwatcher_hashwatcher
set -eu

PATH="/run/current-profile/bin:$PATH"
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
WIDGET=${1:-/mnt/data/bmc/widgets/hashwatcher}
VERSION=${2:-}

if [ -z "$VERSION" ] && [ -f "$WIDGET/version" ]; then
	VERSION=$(tr -d '\n' < "$WIDGET/version")
fi
if [ -z "$VERSION" ] && [ -f "$WIDGET/manifest.json" ]; then
	VERSION=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$WIDGET/manifest.json" | head -n 1)
fi
if [ -z "$VERSION" ]; then
	VERSION=0.1.0
fi

[ -f "$WIDGET/manifest.json" ] || {
	echo "HashWatcher manifest is missing at $WIDGET/manifest.json" >&2
	exit 1
}
[ -f "$WIDGET/lib/wasm/hashwatcher.wasm" ] || {
	echo "HashWatcher wasm is missing at $WIDGET/lib/wasm/hashwatcher.wasm" >&2
	exit 1
}
command -v nix-store >/dev/null 2>&1 || {
	echo "nix-store is not on this Deck." >&2
	exit 1
}
command -v bmc-nix-cli >/dev/null 2>&1 || {
	echo "bmc-nix-cli is not on this Deck." >&2
	exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
root="$work/bmc-widget-hashwatcher"
mkdir -p "$root"

ICON="$WIDGET/assets/icon.svg"
if [ ! -f "$ICON" ]; then
	ICON="$SCRIPT_DIR/../assets/icon.svg"
fi

"$SCRIPT_DIR/assemble-package.sh" "$root" "$WIDGET/lib/wasm/hashwatcher.wasm" "$WIDGET/manifest.json" "$ICON" >/dev/null

store=$(nix-store --add "$root")
bmc-nix-cli add-packages \
	--name widget-hashwatcher_hashwatcher \
	--version "$VERSION" \
	--store-path "$store"
echo "Registered widget-hashwatcher_hashwatcher $VERSION at $store"
