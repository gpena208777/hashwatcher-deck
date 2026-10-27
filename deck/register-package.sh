#!/bin/sh
# Register HashWatcher in the Deck package profile.
# The profile is rebuilt from this list when any other widget updates.
# A symlink added under the live profile generation is not on that list, so the next update removes it.
set -eu

PATH="/run/current-profile/bin:$PATH"
WIDGET=${1:-/mnt/data/bmc/widgets/hashwatcher}
VERSION=${2:-}

if [ -z "$VERSION" ] && [ -f "$WIDGET/version" ]; then
	VERSION=$(tr -d '\n' < "$WIDGET/version")
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
dest="$work/bmc-widget-hashwatcher/lib/bmc-widgets/hashwatcher"
mkdir -p "$dest/lib/wasm" "$dest/bin" "$dest/assets" "$dest/lib/assets"
cp "$WIDGET/manifest.json" "$dest/manifest.json"
cp "$WIDGET/lib/wasm/hashwatcher.wasm" "$dest/lib/wasm/hashwatcher.wasm"
if [ -d "$WIDGET/assets" ]; then
	cp -a "$WIDGET/assets/." "$dest/assets/"
fi
if [ -d "$WIDGET/lib/assets" ]; then
	cp -a "$WIDGET/lib/assets/." "$dest/lib/assets/"
fi
# The launcher has to follow this package. A path under /mnt/data is not part of
# the profile, and nix-store --add cannot embed its own output path beforehand.
cat > "$dest/bin/hashwatcher" << 'EOF'
#!/bin/sh
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
exec /run/current-profile/bin/bmc-wasm-thin-v0 \
	--wasm "$root/lib/wasm/hashwatcher.wasm" \
	--asset-root "$root/lib/assets" \
	"$@"
EOF
chmod 755 "$dest/bin/hashwatcher"
find "$dest" -name '._*' -exec rm -f {} + 2>/dev/null || true
find "$dest" -name '*.wasm.bak*' -exec rm -f {} + 2>/dev/null || true

store=$(nix-store --add "$work/bmc-widget-hashwatcher")
bmc-nix-cli add-packages \
	--name widget-hashwatcher \
	--version "$VERSION" \
	--store-path "$store"
echo "Registered widget-hashwatcher $VERSION at $store"
