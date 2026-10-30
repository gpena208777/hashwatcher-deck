#!/bin/sh
# Download Tailscale when the user asks. The widget package does not contain it.
# Safe to run again: an existing install is started, not downloaded twice.
trap '' HUP
LOCK=/tmp/hashwatcher-tailscale-install.lock
if ! mkdir "$LOCK" 2>/dev/null; then
	exit 0
fi
trap 'rm -rf "$WORK"; rmdir "$LOCK"' EXIT

DIR=/mnt/data/tailscale
BASE=https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck
mkdir -p "$DIR/state" "$DIR/www/cgi-bin" /mnt/data/hashwatcher

lan_route() {
	ip -4 addr show 2>/dev/null | awk '
		$1 == "inet" {
			split($2, a, "/")
			split(a[1], o, ".")
			bits = a[2]
			if (o[1] == "127" || o[1] == "100" || bits == "") next
			if (o[1] == "192" && o[2] == "168") {
				printf "192.168.%s.0/%s\n", o[3], bits
				exit
			}
			if (o[1] == "10") {
				printf "10.%s.%s.0/%s\n", o[2], o[3], bits
				exit
			}
		}
	'
}

route=$(lan_route)
case "$route" in
""|*[!0-9./]*) route=192.168.0.0/24 ;;
esac
printf '%s\n' "$route" > "$DIR/route"

if [ ! -x "$DIR/tailscale" ] || [ ! -x "$DIR/tailscaled" ]; then
	WORK=$(mktemp -d)
	index=$(wget -q -O - -T 25 -U HashWatcher-Deck "https://pkgs.tailscale.com/stable/?mode=json") || exit 1
	ver=$(printf '%s' "$index" | sed -n 's/.*"Version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
	case "$ver" in
	""|*[!0-9.]*) exit 1 ;;
	esac
	url="https://pkgs.tailscale.com/stable/tailscale_${ver}_arm.tgz"
	wget -q -O "$WORK/tailscale.tgz" -T 180 -U HashWatcher-Deck "$url" || exit 1
	tar -xzf "$WORK/tailscale.tgz" -C "$WORK" || exit 1
	bin=$(echo "$WORK"/tailscale_*_arm)
	[ -f "$bin/tailscale" ] && [ -f "$bin/tailscaled" ] || exit 1
	cp "$bin/tailscale" "$bin/tailscaled" "$DIR/"
	chmod 755 "$DIR/tailscale" "$DIR/tailscaled"
	rm -rf "$WORK"
	WORK=
fi

wget -q -O "$DIR/supervise.sh" -T 25 -U HashWatcher-Deck "$BASE/supervise.sh" || exit 1
wget -q -O "$DIR/www/cgi-bin/action" -T 25 -U HashWatcher-Deck "$BASE/cgi-bin/action" || exit 1
wget -q -O "$DIR/www/cgi-bin/update-widget" -T 25 -U HashWatcher-Deck "$BASE/cgi-bin/update-widget" || exit 1
chmod 755 "$DIR/supervise.sh" "$DIR/www/cgi-bin/action" "$DIR/www/cgi-bin/update-widget"

if ! grep -q '/mnt/data/tailscale/supervise.sh' /etc/rc.local 2>/dev/null; then
	tmp=/tmp/rc.local.hashwatcher
	: > "$tmp"
	while IFS= read -r row; do
		case "$row" in
		"exit 0")
			printf '%s\n\n' '/mnt/data/tailscale/supervise.sh >>/mnt/data/tailscale/supervise.log 2>&1 &' >> "$tmp"
			;;
		esac
		printf '%s\n' "$row" >> "$tmp"
	done < /etc/rc.local
	mv "$tmp" /etc/rc.local
fi

printf '%s\n' up > "$DIR/want"
if [ -f "$DIR/supervise.pid" ] && kill -0 "$(cat "$DIR/supervise.pid")" 2>/dev/null; then
	exit 0
fi
"$DIR/supervise.sh" >>"$DIR/supervise.log" 2>&1 &
