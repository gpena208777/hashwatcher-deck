#!/bin/sh
# Scan walks one spot from left to right, then starts again.
# The Deck's own Scan effect always begins on the right, on the same beat.
# A new run picks a random place on that path so it does not restart together.
trap '' HUP
LOCK=/tmp/hashwatcher-scan.lock
if ! mkdir "$LOCK" 2>/dev/null; then
	exit 0
fi
trap 'rmdir "$LOCK" 2>/dev/null' EXIT

lua - <<'EOF'
local modefile = "/tmp/hashwatcher-led.mode"
local colorfile = "/tmp/hashwatcher-led.rgb"
local nixio = require("nixio")
local spi = io.open("/dev/spidev0.0", "wb")
if not spi then
	os.exit(0)
end
spi:setvbuf("no")

local function clocks()
	local ok, sec, usec = pcall(nixio.gettimeofday)
	if not ok or not sec then
		return nil
	end
	return sec, usec or 0
end

local s0, u0 = clocks()
if not s0 then
	spi:close()
	os.exit(0)
end

local function elapsed()
	local sec, usec = clocks()
	if not sec then
		return 0
	end
	return (sec - s0) + (usec - u0) / 1000000
end

local offset = 0
local rf = io.open("/dev/urandom", "rb")
if rf then
	local raw = rf:read(2)
	rf:close()
	if raw and #raw == 2 then
		offset = (raw:byte(1) * 256 + raw:byte(2)) / 65536
	end
end

local function pix(level, r, g, b)
	if level <= 0 then
		return string.char(224, 0, 0, 0)
	end
	local bri = math.floor(level * 31 + 0.5)
	if bri < 1 then bri = 1 end
	if bri > 31 then bri = 31 end
	return string.char(224 + bri, b, g, r)
end

-- Head travels past the strip so the spot leaves on the right before it
-- comes back in on the left. Visual 1 is the left end.
local travel = 12.4
local trail = 2.4
local period = 2.2

local function frame_for(head, r, g, b)
	local function level(i)
		local d = head - (i - 1)
		if d <= 0 or d >= trail then
			return 0
		end
		if d < 0.35 then
			return d / 0.35
		end
		if d > trail - 0.7 then
			local tail = (trail - d) / 0.7
			if tail < 0 then
				return 0
			end
			return tail
		end
		return 1
	end
	local frame = string.char(0, 0, 0, 0)
	for chain = 1, 10 do
		frame = frame .. pix(level(11 - chain), r, g, b)
	end
	return frame .. string.rep(string.char(255), 12)
end

local function wrap(value)
	return value - math.floor(value)
end

local r, g, b = 0, 90, 255
local mode = "scan"
local shown = -1
local frame = frame_for(offset * travel, r, g, b)
local tick = 0
while true do
	tick = tick + 1
	local dirty = false
	if tick >= 100 then
		tick = 0
		local mf = io.open(modefile, "r")
		if mf then
			mode = mf:read("*l") or mode
			mf:close()
		end
		if mode ~= "scan" then
			break
		end
		local cf = io.open(colorfile, "r")
		if cf then
			local line = cf:read("*l") or ""
			cf:close()
			local nr, ng, nb = line:match("^(%d+)%s+(%d+)%s+(%d+)")
			if nr then
				local next_r = tonumber(nr) or r
				local next_g = tonumber(ng) or g
				local next_b = tonumber(nb) or b
				if next_r > 255 then next_r = 255 end
				if next_g > 255 then next_g = 255 end
				if next_b > 255 then next_b = 255 end
				if next_r ~= r or next_g ~= g or next_b ~= b then
					r, g, b = next_r, next_g, next_b
					dirty = true
				end
			end
		end
	end
	if mode ~= "scan" then
		break
	end
	local head = wrap(elapsed() / period + offset) * travel
	if dirty or math.abs(head - shown) >= 0.03 then
		frame = frame_for(head, r, g, b)
		shown = head
	end
	spi:write(frame)
	nixio.nanosleep(0, 400000)
end
spi:close()
EOF
