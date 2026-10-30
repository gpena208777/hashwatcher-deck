#!/bin/sh
# The package ships the Tailscale installer. Copy it into place and start the
# download when Tailscale is not installed yet, then start the widget.
# The exec below stays the official SDK major 0 wrapper.
share="${0%/*}/../share"
if [ -d "$share" ]; then
	mkdir -p /mnt/data/hashwatcher /www/cgi-bin
	for name in install-tailscale.sh scan-leds.sh supervise.sh coin.wav action update-widget; do
		if [ -f "$share/$name" ]; then
			cp "$share/$name" "/mnt/data/hashwatcher/$name" || true
		fi
	done
	chmod 755 /mnt/data/hashwatcher/install-tailscale.sh /mnt/data/hashwatcher/scan-leds.sh \
		/mnt/data/hashwatcher/supervise.sh /mnt/data/hashwatcher/action \
		/mnt/data/hashwatcher/update-widget 2>/dev/null || true
	for name in hashwatcher-install-tailscale hashwatcher-install-status hashwatcher-play-sound \
		hashwatcher-led hashwatcher-settings hashwatcher-update-status; do
		if [ -f "$share/$name" ]; then
			cp "$share/$name" "/www/cgi-bin/$name" || true
			chmod 755 "/www/cgi-bin/$name" || true
		fi
	done
	if [ ! -x /mnt/data/tailscale/tailscaled ] && [ -x /mnt/data/hashwatcher/install-tailscale.sh ]; then
		if [ ! -d /tmp/hashwatcher-tailscale-install.lock ]; then
			start-stop-daemon -S -b -x /mnt/data/hashwatcher/install-tailscale.sh || true
		fi
	fi
fi
exec /run/current-profile/bin/bmc-wasm-thin-v0 \
    --wasm "${0%/*}/../lib/wasm/hashwatcher.wasm" \
    --asset-root "${0%/*}/../lib/assets" \
    "$@"
