#!/bin/sh
# Assemble a HashWatcher third-party widget package tree for Deck upload or
# SSH registration. Layout matches BraiinsForge/bmc-main docs/uploading-wasm-widgets.md:
#
#   lib/bmc-widgets/hashwatcher/hashwatcher/
#     manifest.json
#     bin/hashwatcher          # official WASM launcher (SDK major 0)
#     lib/wasm/hashwatcher.wasm
#     assets/icon.svg
#
# Usage: assemble-package.sh <dest-root> <wasm-path> [manifest-path] [icon-path]
# dest-root receives the lib/ tree (it is created if missing).
set -eu

DEST_ROOT=${1:?dest-root required}
WASM=${2:?wasm path required}
MANIFEST=${3:-}
ICON=${4:-}

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

MAINTAINER=hashwatcher
WIDGET=hashwatcher
DEST="$DEST_ROOT/lib/bmc-widgets/$MAINTAINER/$WIDGET"

rm -rf "$DEST_ROOT/lib"
mkdir -p "$DEST/bin" "$DEST/lib/wasm" "$DEST/assets" "$DEST/lib/assets"

cp "$MANIFEST" "$DEST/manifest.json"
cp "$WASM" "$DEST/lib/wasm/$WIDGET.wasm"
cp "$ICON" "$DEST/assets/icon.svg"
# Official wrapper from bmc-upgrade/assets/wasm-widget-wrapper.sh (SDK major 0).
# Deck .nar validation requires a byte-for-byte match.
cp "$SCRIPT_DIR/hashwatcher-launcher.sh" "$DEST/bin/$WIDGET"
chmod 755 "$DEST/bin/$WIDGET"

find "$DEST" -name '._*' -exec rm -f {} + 2>/dev/null || true
find "$DEST" -name '*.wasm.bak*' -exec rm -f {} + 2>/dev/null || true

echo "$DEST"
