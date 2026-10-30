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
trap 'printf "%s\n" pause > /tmp/hashwatcher-led.mode; if [ -n "$WORK" ]; then rm -rf "$WORK"; fi; rmdir "$LOCK" 2>/dev/null' EXIT

DIR=/mnt/data/tailscale
BASE=https://raw.githubusercontent.com/gpena208777/hashwatcher-deck/main/deck
STATUS=/mnt/data/hashwatcher/install-status.json
mkdir -p "$DIR/state" "$DIR/www/cgi-bin" /mnt/data/hashwatcher

PCT=/tmp/hashwatcher-led.pct
printf '0\n' > "$PCT"
# The widget sets this to pause when the user leaves the page mid-download.
case "$(cat /tmp/hashwatcher-led.mode 2>/dev/null)" in
pause) ;;
*) printf '%s\n' fill > /tmp/hashwatcher-led.mode ;;
esac

say() {
	printf '{"state":"%s","progress":%s,"error":"%s"}\n' "$1" "$2" "$3" > "$STATUS.tmp"
	mv "$STATUS.tmp" "$STATUS"
	printf '%s\n' "$2" > "$PCT"
}

# One open SPI handle for the whole download. Reopening the device blanks
# the strip before the next frame, which is the flash between LEDs.
# The chain's first pixel is the right end, so the fill is written backwards.
lua - "$LOCK" "$PCT" <<'EOF' >/dev/null 2>&1 &
local lock, pctfile = arg[1], arg[2]
local modefile = "/tmp/hashwatcher-led.mode"
local nixio = require("nixio")
local spi = io.open("/dev/spidev0.0", "wb")
if spi then spi:setvbuf("no") end
local function frame_for(pct)
	if pct < 0 then pct = 0 end
	if pct > 100 then pct = 100 end
	local full = math.floor(pct / 10)
	local frac = pct / 10 - full
	local function pix(level)
		if level <= 0 then return string.char(224, 0, 0, 0) end
		local bri = math.floor(level * 31 + 0.5)
		if bri < 1 then bri = 1 end
		if bri > 31 then bri = 31 end
		return string.char(224 + bri, 102, 204, 0)
	end
	local level_at = {}
	for pos = 1, 10 do
		local level = 0
		if pos <= full then level = 1
		elseif pos == full + 1 then level = frac end
		level_at[pos] = level
	end
	local frame = string.char(0, 0, 0, 0)
	for chain = 1, 10 do
		frame = frame .. pix(level_at[11 - chain])
	end
	-- Extra clocks so the last pixel latches before the next frame starts.
	-- A short end frame is what makes the strip flicker between writes.
	return frame .. string.rep(string.char(255), 12)
end
local pct = 0
local mode = "fill"
local frame = frame_for(0)
local tick = 0
while nixio.fs.stat(lock) do
	tick = tick + 1
	-- Reading the percent file on every pass leaves a gap, and the Deck
	-- paints the strip black in that gap. Check the files rarely.
	if tick >= 100 then
		tick = 0
		local mf = io.open(modefile, "r")
		if mf then
			mode = mf:read("*l") or mode
			mf:close()
		end
		local rf = io.open(pctfile, "r")
		if rf then
			local next_pct = tonumber(rf:read("*a"))
			rf:close()
			if next_pct and next_pct ~= pct then
				pct = next_pct
				frame = frame_for(pct)
			end
		end
	end
	if mode ~= "pause" and spi then
		spi:write(frame)
	end
	nixio.nanosleep(0, 400000)
end
if spi then spi:close() end
EOF

tick() {
	lua -e 'require("nixio").nanosleep(0, 200000000)' >/dev/null 2>&1 || sleep 1
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
	: > "$WORK/tailscale.tgz"
	wget -q -O "$WORK/tailscale.tgz" -T 900 -U HashWatcher-Deck "$url" &
	wpid=$!
	while kill -0 "$wpid" 2>/dev/null; do
		size=$(wc -c "$WORK/tailscale.tgz" 2>/dev/null | awk '{print $1}')
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
		tick
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
chmod 755 "$DIR/supervise.sh"
# Start login as soon as the binary is on disk so the sign-in code is ready
# when the progress bar finishes, instead of after another round trip.
printf '%s\n' up > "$DIR/want"
if ! { [ -f "$DIR/supervise.pid" ] && kill -0 "$(cat "$DIR/supervise.pid")" 2>/dev/null; }; then
	"$DIR/supervise.sh" >>"$DIR/supervise.log" 2>&1 &
fi
wget -q -O "$DIR/www/cgi-bin/action" -T 25 -U HashWatcher-Deck "$BASE/cgi-bin/action" || fail "Could not download Tailscale."
wget -q -O "$DIR/www/cgi-bin/update-widget" -T 25 -U HashWatcher-Deck "$BASE/cgi-bin/update-widget" || fail "Could not download Tailscale."
chmod 755 "$DIR/www/cgi-bin/action" "$DIR/www/cgi-bin/update-widget"

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
if ! { [ -f "$DIR/supervise.pid" ] && kill -0 "$(cat "$DIR/supervise.pid")" 2>/dev/null; }; then
	"$DIR/supervise.sh" >>"$DIR/supervise.log" 2>&1 &
fi
say done 100 ""
