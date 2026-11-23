-- Compare sandbox rectangles with rectangles captured in game.
--
-- Run inside the sandbox after opening the same UI as in the capture:
--   wowprogramming ctl exec "/run WowProgrammingCompareFrames()"
-- or load this file with dofile-like means. Uses WowProgrammingCapture,
-- which a scenario sets from capture.txt. Prints the worst differences.

function WowProgrammingCompareFrames(limit, tolerance)
	limit = limit or 40
	tolerance = tolerance or 1
	local capture = WowProgrammingCapture
	if not capture or not capture.frames then
		print("No capture with frames loaded.")
		return
	end

	-- Resolve "A.B.C" through globals and keys. Anonymous parts (hex
	-- addresses in the client) cannot be resolved.
	local function resolve(path)
		local obj
		for part in path:gmatch("[^.]+") do
			if obj == nil then
				obj = _G[part]
			elseif type(obj) == "table" then
				obj = rawget(obj, part)
			else
				return nil
			end
			if obj == nil then return nil end
		end
		return obj
	end

	local rows, compared, missing, hidden = {}, 0, 0, 0
	for _, e in ipairs(capture.frames) do
		if e.rect and not e.path:find("%x%x%x%x%x%x%x%x") then
			local obj = resolve(e.path)
			if type(obj) ~= "table" or not obj.GetRect then
				missing = missing + 1
			elseif not obj:IsVisible() then
				hidden = hidden + 1
			else
				local l, b, w, h = obj:GetRect()
				if l then
					compared = compared + 1
					local d = math.max(math.abs(l - e.rect[1]), math.abs(b - e.rect[2]),
						math.abs(w - e.rect[3]), math.abs(h - e.rect[4]))
					if d > tolerance then
						table.insert(rows, { d = d, path = e.path, want = e.rect, got = { l, b, w, h } })
					end
				else
					missing = missing + 1
				end
			end
		end
	end
	table.sort(rows, function(a, b) return a.d > b.d end)
	print(string.format("Compared %d rectangles: %d differ by more than %g; %d not found, %d hidden.",
		compared, #rows, tolerance, missing, hidden))
	local function fmt(r)
		return string.format("%.1f,%.1f %.1fx%.1f", r[1], r[2], r[3], r[4])
	end
	for i = 1, math.min(limit, #rows) do
		local r = rows[i]
		print(string.format("%6.1f  %s\n        game %s\n        here %s", r.d, r.path, fmt(r.want), fmt(r.got)))
	end
end
