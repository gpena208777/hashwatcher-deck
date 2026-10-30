#!/bin/sh
# Register HashWatcher in the Deck package profile.
# The profile is rebuilt from this list when any other widget updates.
# A symlink added under the live profile generation is not on that list, so the next update removes it.
#
# Flat package, same identity as the widget already on field Decks:
#   lib/bmc-widgets/hashwatcher/
# Package name: widget-hashwatcher
# Do not nest a hashwatcher/ maintainer folder. The Deck refuses that beside
# the installed flat widget, and a second folder with the same uid.
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

"$SCRIPT_DIR/assemble-package.sh" "$root" "$WIDGET/lib/wasm/hashwatcher.wasm" "$WIDGET/manifest.json" "$ICON" "$WIDGET/lib/assets" >/dev/null

store=$(nix-store --add "$root")
bmc-nix-cli add-packages \
	--name widget-hashwatcher \
	--version "$VERSION" \
	--store-path "$store"
# The installer script is not Tailscale. The download happens only after the user taps Install.
plant_tailscale_installer() {
	base=https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck
	mkdir -p /mnt/data/hashwatcher
	wget -q -O /mnt/data/hashwatcher/install-tailscale.sh -T 25 -U HashWatcher-Deck \
		"$base/install-tailscale.sh" || return 1
	chmod 755 /mnt/data/hashwatcher/install-tailscale.sh
	wget -q -O /mnt/data/hashwatcher/scan-leds.sh -T 25 -U HashWatcher-Deck \
		"$base/scan-leds.sh" || return 1
	chmod 755 /mnt/data/hashwatcher/scan-leds.sh
	wget -q -O /mnt/data/hashwatcher/update-widget -T 25 -U HashWatcher-Deck \
		"$base/cgi-bin/update-widget" || return 1
	chmod 755 /mnt/data/hashwatcher/update-widget
	wget -q -O /mnt/data/hashwatcher/supervise.sh -T 25 -U HashWatcher-Deck \
		"$base/supervise.sh" || return 1
	chmod 755 /mnt/data/hashwatcher/supervise.sh
	if [ -d /www/cgi-bin ]; then
		wget -q -O /www/cgi-bin/hashwatcher-install-tailscale -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/hashwatcher-install-tailscale" || return 1
		wget -q -O /www/cgi-bin/hashwatcher-install-status -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/hashwatcher-install-status" || return 1
		wget -q -O /www/cgi-bin/hashwatcher-play-sound -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/hashwatcher-play-sound" || return 1
		wget -q -O /www/cgi-bin/hashwatcher-led -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/hashwatcher-led" || return 1
		wget -q -O /www/cgi-bin/hashwatcher-settings -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/hashwatcher-settings" || return 1
		wget -q -O /www/cgi-bin/hashwatcher-update-status -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/hashwatcher-update-status" || return 1
		wget -q -O /www/cgi-bin/hashwatcher-update -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/hashwatcher-update" || return 1
		chmod 755 /www/cgi-bin/hashwatcher-install-tailscale /www/cgi-bin/hashwatcher-install-status \
			/www/cgi-bin/hashwatcher-play-sound /www/cgi-bin/hashwatcher-led \
			/www/cgi-bin/hashwatcher-settings /www/cgi-bin/hashwatcher-update-status \
			/www/cgi-bin/hashwatcher-update
	fi
	if [ -d /mnt/data/tailscale/www/cgi-bin ]; then
		wget -q -O /mnt/data/tailscale/www/cgi-bin/update-widget -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/update-widget" || return 1
		wget -q -O /mnt/data/tailscale/www/cgi-bin/action -T 25 -U HashWatcher-Deck \
			"$base/cgi-bin/action" || return 1
		chmod 755 /mnt/data/tailscale/www/cgi-bin/update-widget /mnt/data/tailscale/www/cgi-bin/action
	fi
	wget -q -O /mnt/data/hashwatcher/coin.wav -T 25 -U HashWatcher-Deck \
		"$base/coin.wav" || true
}

echo "Registered widget-hashwatcher $VERSION at $store"
plant_tailscale_installer || true
