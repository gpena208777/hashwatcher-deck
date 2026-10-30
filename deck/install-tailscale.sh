#!/bin/sh
# Download Tailscale when the user asks. The widget package does not contain it.
# The download stays on /mnt/data. /tmp is a small RAM disk and cannot hold the archive.
trap '' HUP
mkdir -p /mnt/data/hashwatcher
: > /mnt/data/hashwatcher/install-tailscale.log
exec >>/mnt/data/hashwatcher/install-tailscale.log 2>&1
LOCK=/tmp/hashwatcher-tailscale-install.lock
if ! mkdir "$LOCK" 2>/dev/null; then
	exit 0
fi
WORK=
trap 'if [ -n "$WORK" ]; then rm -rf "$WORK"; fi; rmdir "$LOCK" 2>/dev/null' EXIT

DIR=/mnt/data/tailscale
BASE=https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck
STATUS=/mnt/data/hashwatcher/install-status.json
mkdir -p "$DIR/state" "$DIR/www/cgi-bin" /mnt/data/hashwatcher

say() {
	printf '{"state":"%s","progress":%s,"error":"%s"}\n' "$1" "$2" "$3" > "$STATUS.tmp"
	mv "$STATUS.tmp" "$STATUS"
}

fail() {
	say failed 0 "$1"
	exit 1
}

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

say downloading 1 ""
avail=$(df -k /mnt/data 2>/dev/null | awk 'NR==2 {print $4}')
case "$avail" in
''|*[!0-9]*) ;;
*)
	if [ "$avail" -lt 200000 ]; then
		fail "Not enough space to install Tailscale."
	fi
	;;
esac

route=$(lan_route)
case "$route" in
""|*[!0-9./]*) route=192.168.0.0/24 ;;
esac
printf '%s\n' "$route" > "$DIR/route"

if [ ! -x "$DIR/tailscale" ] || [ ! -x "$DIR/tailscaled" ]; then
	WORK=/mnt/data/hashwatcher/work
	rm -rf "$WORK"
	mkdir -p "$WORK" || fail "Not enough space to install Tailscale."
	index=$(wget -q -O - -T 25 -U HashWatcher-Deck "https://pkgs.tailscale.com/stable/?mode=json") || fail "Could not download Tailscale."
	ver=$(printf '%s' "$index" | sed -n 's/.*"Version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
	case "$ver" in
	""|*[!0-9.]*) fail "Could not download Tailscale." ;;
	esac
	url="https://pkgs.tailscale.com/stable/tailscale_${ver}_arm.tgz"
	say downloading 5 ""
	wget -q -O "$WORK/tailscale.tgz" -T 900 -U HashWatcher-Deck "$url" &
	wpid=$!
	while kill -0 "$wpid" 2>/dev/null; do
		size=$(wc -c < "$WORK/tailscale.tgz" 2>/dev/null | awk '{print $1}')
		case "$size" in
		''|*[!0-9]*) size=0 ;;
		esac
		pct=$((5 + size / 480000))
		if [ "$pct" -gt 80 ]; then
			pct=80
		fi
		if [ "$pct" -lt 5 ]; then
			pct=5
		fi
		say downloading "$pct" ""
		sleep 1
	done
	wait "$wpid" || fail "Could not download Tailscale."
	say installing 86 ""
	tar -xzf "$WORK/tailscale.tgz" -C "$WORK" || fail "Could not unpack Tailscale."
	bin=$(echo "$WORK"/tailscale_*_arm)
	[ -f "$bin/tailscale" ] && [ -f "$bin/tailscaled" ] || fail "Could not unpack Tailscale."
	say installing 92 ""
	cp "$bin/tailscale" "$bin/tailscaled" "$DIR/" || fail "Not enough space to install Tailscale."
	chmod 755 "$DIR/tailscale" "$DIR/tailscaled"
	rm -rf "$WORK"
	WORK=
fi

say installing 96 ""
wget -q -O "$DIR/supervise.sh" -T 25 -U HashWatcher-Deck "$BASE/supervise.sh" || fail "Could not download Tailscale."
wget -q -O "$DIR/www/cgi-bin/action" -T 25 -U HashWatcher-Deck "$BASE/cgi-bin/action" || fail "Could not download Tailscale."
wget -q -O "$DIR/www/cgi-bin/update-widget" -T 25 -U HashWatcher-Deck "$BASE/cgi-bin/update-widget" || fail "Could not download Tailscale."
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

say starting 98 ""
printf '%s\n' up > "$DIR/want"
if [ -f "$DIR/supervise.pid" ] && kill -0 "$(cat "$DIR/supervise.pid")" 2>/dev/null; then
	say done 100 ""
	exit 0
fi
"$DIR/supervise.sh" >>"$DIR/supervise.log" 2>&1 &
say done 100 ""
