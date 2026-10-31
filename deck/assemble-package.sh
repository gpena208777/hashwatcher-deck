#!/bin/sh
# Assemble a HashWatcher widget package tree for Deck upload or registration.
#
# Field Decks already have this widget as a flat package:
#   lib/bmc-widgets/hashwatcher/
#   package name widget-hashwatcher
#
# A maintainer folder also named hashwatcher
# (lib/bmc-widgets/hashwatcher/hashwatcher/) is refused on those Decks:
# "hashwatcher/hashwatcher cannot be installed next to the installed widget
# hashwatcher: only one of them would show". The same uid under any other
# folder is refused too. Keep this flat so Install Widget replaces the
# installed package in place.
#
#   lib/bmc-widgets/hashwatcher/
#     manifest.json
#     bin/hashwatcher          # official WASM launcher (SDK major 0), byte for byte
#     lib/wasm/hashwatcher.wasm
#     assets/icon.svg
#     lib/assets/                # wasm images, plus the Tailscale installer as data
#
# Usage: assemble-package.sh <dest-root> <wasm-path> [manifest-path] [icon-path] [asset-root]
# dest-root receives the lib/ tree (it is created if missing).
set -eu

DEST_ROOT=${1:?dest-root required}
WASM=${2:?wasm path required}
MANIFEST=${3:-}
ICON=${4:-}
ASSET_SRC=${5:-}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

if [ -z "$MANIFEST" ]; then
	MANIFEST="$REPO_ROOT/manifest.json"
fi
if [ -z "$ICON" ]; then
	ICON="$REPO_ROOT/assets/icon.svg"
fi

[ -f "$WASM" ] || {
	echo "wasm missing: $WASM" >&2
	exit 1
}
[ -f "$MANIFEST" ] || {
	echo "manifest missing: $MANIFEST" >&2
	exit 1
}
[ -f "$ICON" ] || {
	echo "icon missing: $ICON" >&2
	exit 1
}
[ -f "$SCRIPT_DIR/hashwatcher-launcher.sh" ] || {
	echo "launcher missing: $SCRIPT_DIR/hashwatcher-launcher.sh" >&2
	exit 1
}

WIDGET=hashwatcher
DEST="$DEST_ROOT/lib/bmc-widgets/$WIDGET"

rm -rf "$DEST_ROOT/lib"
mkdir -p "$DEST/bin" "$DEST/lib/wasm" "$DEST/assets" "$DEST/lib/assets"

cp "$MANIFEST" "$DEST/manifest.json"
cp "$WASM" "$DEST/lib/wasm/$WIDGET.wasm"
cp "$ICON" "$DEST/assets/icon.svg"
# Official wrapper from bmc-upgrade/assets/wasm-widget-wrapper.sh (SDK major 0).
# Deck .nar validation requires a byte-for-byte match.
cp "$SCRIPT_DIR/hashwatcher-launcher.sh" "$DEST/bin/$WIDGET"
chmod 755 "$DEST/bin/$WIDGET"
# The wasm references compiled images under lib/assets. An empty directory
# makes the widget crash on startup and the Deck stays on Loading Scene.
if [ -n "$ASSET_SRC" ] && [ -d "$ASSET_SRC" ]; then
	cp -a "$ASSET_SRC"/. "$DEST/lib/assets"/
fi
# Install Widget rejects anything outside assets/ and lib/assets/, and it
# rejects an executable file in those directories. The Tailscale installer
# travels here as data. Deck updates run it from register-package.sh.
# A changed launcher cannot: the Deck requires Braiins' wrapper byte for byte.
BOOT="$DEST/lib/assets/boot"
mkdir -p "$BOOT"
place_boot() {
	dest=$1
	localpath=$2
	url=$3
	if [ -f "$localpath" ]; then
		cp "$localpath" "$dest"
	else
		wget -q -O "$dest" -T 25 -U HashWatcher-Deck "$url"
	fi
	chmod 644 "$dest"
}
SHARE_BASE=https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck
place_boot "$BOOT/install-tailscale.sh" "$SCRIPT_DIR/install-tailscale.sh" "$SHARE_BASE/install-tailscale.sh"
place_boot "$BOOT/scan-leds.sh" "$SCRIPT_DIR/scan-leds.sh" "$SHARE_BASE/scan-leds.sh"
place_boot "$BOOT/supervise.sh" "$SCRIPT_DIR/supervise.sh" "$SHARE_BASE/supervise.sh"
place_boot "$BOOT/coin.wav" "$SCRIPT_DIR/coin.wav" "$SHARE_BASE/coin.wav"
place_boot "$BOOT/action" "$SCRIPT_DIR/cgi-bin/action" "$SHARE_BASE/cgi-bin/action"
place_boot "$BOOT/update-widget" "$SCRIPT_DIR/cgi-bin/update-widget" "$SHARE_BASE/cgi-bin/update-widget"
for name in hashwatcher-install-tailscale hashwatcher-install-status hashwatcher-play-sound \
	hashwatcher-led hashwatcher-settings hashwatcher-update-status hashwatcher-update; do
	place_boot "$BOOT/$name" "$SCRIPT_DIR/cgi-bin/$name" "$SHARE_BASE/cgi-bin/$name"
done

find "$DEST" -name '._*' -exec rm -f {} + 2>/dev/null || true
find "$DEST" -name '*.wasm.bak*' -exec rm -f {} + 2>/dev/null || true

echo "$DEST"
