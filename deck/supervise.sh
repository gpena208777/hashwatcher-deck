#!/bin/sh
# Keep Tailscale in userspace on the Deck and replace the login URL when it expires.
# /etc/rc.local must start this script before its exit 0. A line after exit 0 never runs.
DIR=/mnt/data/tailscale
SOCK=$DIR/tailscaled.sock
TS=$DIR/tailscale
TSD=$DIR/tailscaled
WWW=$DIR/www
STATUS=$WWW/status.json
LOGIN_SECS=240

mkdir -p "$DIR/state" "$WWW"
echo $$ > "$DIR/supervise.pid"

start_daemon() {
	if [ -f "$DIR/tailscaled.pid" ] && kill -0 "$(cat "$DIR/tailscaled.pid")" 2>/dev/null; then
		return
	fi
	"$TSD" \
		--tun=userspace-networking \
		--state="$DIR/state/tailscaled.state" \
		--statedir="$DIR/state" \
		--socket="$SOCK" \
		--port=41641 \
		>>"$DIR/tailscaled.log" 2>&1 &
	echo $! > "$DIR/tailscaled.pid"
	sleep 1
}

start_http() {
	if [ -f "$DIR/uhttpd.pid" ] && kill -0 "$(cat "$DIR/uhttpd.pid")" 2>/dev/null; then
		return
	fi
	uhttpd -f -p 127.0.0.1:9418 -h "$WWW" -D >>"$DIR/uhttpd.log" 2>&1 &
	echo $! > "$DIR/uhttpd.pid"
}

json_field() {
	printf '%s' "$1" | sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" | head -n 1
}

write_status() {
	tmp="$STATUS.tmp"
	printf '{"state":"%s","hostname":"%s","ip":"%s","login_url":"%s","subnet":"%s","key_expiry":"%s","admin_url":"%s"}\n' \
		"$1" "$2" "$3" "$4" "$5" "$6" "$7" > "$tmp"
	mv "$tmp" "$STATUS"
}

self_json() {
	printf '%s\n' "$1" | sed '/"Peer":/q'
}

subnet_of() {
	self=$(self_json "$1")
	if printf '%s\n' "$self" | grep -q '192.168.0.0/24'; then
		printf approved
	else
		printf pending
	fi
}

expiry_of() {
	self=$(self_json "$1")
	exp=$(json_field "$self" KeyExpiry)
	case "$exp" in
	""|0001-01-01*) printf '' ;;
	*) printf '%s' "$exp" | cut -c1-10 ;;
	esac
}

admin_for() {
	if [ -n "$1" ]; then
		printf 'https://login.tailscale.com/admin/machines/%s' "$1"
	fi
}

up_alive() {
	[ -f "$DIR/up.pid" ] && kill -0 "$(cat "$DIR/up.pid")" 2>/dev/null
}

want_state() {
	state=$(cat "$DIR/want" 2>/dev/null || echo up)
	case "$state" in
	down) printf down ;;
	*) printf up ;;
	esac
}

stop_login() {
	if up_alive; then
		kill "$(cat "$DIR/up.pid")" 2>/dev/null
		sleep 1
	fi
}

daemon_pid() {
	if [ -f "$DIR/tailscaled.pid" ]; then
		pid=$(cat "$DIR/tailscaled.pid")
		if kill -0 "$pid" 2>/dev/null; then
			printf '%s' "$pid"
			return
		fi
	fi
	for dir in /proc/[0-9]*; do
		cmd=$(tr '\0' ' ' < "$dir/cmdline" 2>/dev/null) || continue
		case "$cmd" in
		*tailscaled*)
			printf '%s' "${dir#/proc/}"
			return
			;;
		esac
	done
}

stop_daemon() {
	pid=$(daemon_pid)
	if [ -n "$pid" ]; then
		kill "$pid" 2>/dev/null
		sleep 1
		kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null
	fi
	rm -f "$DIR/tailscaled.pid" "$SOCK"
}

stop_tailscale() {
	printf '%s\n' down > "$DIR/want"
	stop_login
	"$TS" --socket="$SOCK" down >/dev/null 2>&1 || true
}

start_tailscale() {
	printf '%s\n' up > "$DIR/want"
	start_daemon
	start_login
}

restart_tailscale() {
	stop_login
	stop_daemon
	start_daemon
	if [ "$(want_state)" != down ]; then
		start_login
	fi
}

reset_tailscale() {
	printf '%s\n' up > "$DIR/want"
	stop_login
	"$TS" --socket="$SOCK" logout >/dev/null 2>&1 || true
	stop_daemon
	rm -f "$DIR/state"/* "$DIR/up.log" "$DIR/up.pid" "$DIR/up.started"
	mkdir -p "$DIR/state"
	start_daemon
	start_login
}

apply_cmd() {
	[ -s "$DIR/cmd" ] || return 0
	cmd=$(cat "$DIR/cmd" 2>/dev/null)
	rm -f "$DIR/cmd"
	case "$cmd" in
	stop) stop_tailscale ;;
	start) start_tailscale ;;
	restart) restart_tailscale ;;
	reset) reset_tailscale ;;
	relogin)
		printf '%s\n' up > "$DIR/want"
		stop_login
		start_login
		;;
	esac
}

start_login() {
	if up_alive; then
		kill "$(cat "$DIR/up.pid")" 2>/dev/null
		sleep 1
	fi
	: > "$DIR/up.log"
	"$TS" --socket="$SOCK" up \
		--hostname=HashWatcher-Deck \
		--timeout=4m \
		--accept-dns=false \
		--accept-routes=false \
		--advertise-routes=192.168.0.0/24 \
		>>"$DIR/up.log" 2>&1 &
	echo $! > "$DIR/up.pid"
	date +%s > "$DIR/up.started"
}

login_url() {
	url=$(json_field "$1" AuthURL)
	if [ -z "$url" ]; then
		url=$(sed -n 's#.*\(https://login\.tailscale\.com/a/[A-Za-z0-9][A-Za-z0-9]*\).*#\1#p' "$DIR/up.log" 2>/dev/null | tail -n 1)
	fi
	printf '%s' "$url"
}

while true; do
	start_daemon
	start_http
	apply_cmd
	raw=$("$TS" --socket="$SOCK" status --json 2>/dev/null) || raw=""
	state=$(json_field "$raw" BackendState)
	host=$(json_field "$raw" HostName)
	ip=$(printf '%s\n' "$raw" | sed -n 's/.*"\(100\.[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)".*/\1/p' | head -n 1)
	[ -n "$host" ] || host="HashWatcher-Deck"
	case "$host" in
	braiins-deck|hashwatcher-deck|HashWatcher-Deck) host="HashWatcher-Deck" ;;
	esac
	subnet=$(subnet_of "$raw")
	expiry=$(expiry_of "$raw")
	admin=$(admin_for "$ip")
	if [ "$(want_state)" = down ]; then
		case "$state" in
		Running|Starting)
			"$TS" --socket="$SOCK" down >/dev/null 2>&1 || true
			;;
		esac
		write_status stopped "$host" "" "" "" "" ""
		sleep 1
		continue
	fi
	case "$state" in
	Running)
		if up_alive; then
			kill "$(cat "$DIR/up.pid")" 2>/dev/null
		fi
		write_status connected "$host" "$ip" "" "$subnet" "$expiry" "$admin"
		;;
	NeedsLogin|NeedsMachineAuth)
		now=$(date +%s)
		started=$(cat "$DIR/up.started" 2>/dev/null || echo 0)
		age=$((now - started))
		if ! up_alive; then
			if [ "$age" -ge 8 ] || [ "$started" -eq 0 ]; then
				start_login
			fi
		elif [ "$age" -ge "$LOGIN_SECS" ]; then
			start_login
		fi
		url=$(login_url "$raw")
		if [ -z "$url" ]; then
			url=$(json_field "$(cat "$STATUS" 2>/dev/null)" login_url)
		fi
		write_status needs_login "$host" "$ip" "$url" "$subnet" "$expiry" "$admin"
		;;
	Stopped)
		start_login
		write_status starting "$host" "$ip" "" "$subnet" "$expiry" "$admin"
		;;
	*)
		write_status starting "$host" "$ip" "" "$subnet" "$expiry" "$admin"
		;;
	esac
	sleep 1
done
