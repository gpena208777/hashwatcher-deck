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
#     bin/hashwatcher          # official WASM launcher (SDK major 0)
#     lib/wasm/hashwatcher.wasm
#     assets/icon.svg
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
# Helpers the launcher copies onto the Deck. Install Widget does not run
# register-package.sh, so these have to travel inside the package.
mkdir -p "$DEST/share"
for name in install-tailscale.sh scan-leds.sh supervise.sh coin.wav; do
	if [ -f "$SCRIPT_DIR/$name" ]; then
		cp "$SCRIPT_DIR/$name" "$DEST/share/$name"
	fi
done
for name in hashwatcher-install-tailscale hashwatcher-install-status hashwatcher-play-sound \
	hashwatcher-led hashwatcher-settings hashwatcher-update-status; do
	if [ -f "$SCRIPT_DIR/cgi-bin/$name" ]; then
		cp "$SCRIPT_DIR/cgi-bin/$name" "$DEST/share/$name"
	fi
done
if [ -f "$SCRIPT_DIR/cgi-bin/action" ]; then
	cp "$SCRIPT_DIR/cgi-bin/action" "$DEST/share/action"
fi
if [ -f "$SCRIPT_DIR/cgi-bin/update-widget" ]; then
	cp "$SCRIPT_DIR/cgi-bin/update-widget" "$DEST/share/update-widget"
fi
chmod 755 "$DEST/share/"* 2>/dev/null || true
# The wasm references compiled images under lib/assets. An empty directory
# makes the widget crash on startup and the Deck stays on Loading Scene.
if [ -n "$ASSET_SRC" ] && [ -d "$ASSET_SRC" ]; then
	cp -a "$ASSET_SRC"/. "$DEST/lib/assets"/
fi

find "$DEST" -name '._*' -exec rm -f {} + 2>/dev/null || true
find "$DEST" -name '*.wasm.bak*' -exec rm -f {} + 2>/dev/null || true

echo "$DEST"
