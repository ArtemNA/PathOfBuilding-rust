#@ SimpleGraphic
-- U1.1 extra text measurements (cases the text-corpus.lua corpus leaves ambiguous) -> sg/out/text-extra/*.json
--   cases.json  rows [fn, font, height, bytes[], x, y, result]  (fn = "w" DrawStringWidth, "c" DrawStringCursorIndex,
--               "s" StripEscapes -> result is bytes[]); text as byte arrays so invalid UTF-8 survives JSON.
--   perf.json   ms for N calls of DrawStringWidth / DrawStringCursorIndex / StripEscapes / GetTime (call overhead).
-- Run: pwsh tools/sg-oracle/run.ps1 -Script text-extra.lua
local dir = GetScriptPath() .. "/out/text-extra/"
local json = require("dkjson")
local q = json.quotestring

local function num(v)
	if type(v) ~= "number" then return "null" end
	return (v == math.floor(v) and string.format("%d", v)) or string.format("%.17g", v)
end
local function bytes(s)
	local o = {}
	for i = 1, #s do o[i] = tostring(s:byte(i)) end
	return "[" .. table.concat(o, ",") .. "]"
end
local rows = {}
local function row(fn, font, h, s, x, y, r)
	local rs = type(r) == "string" and bytes(r) or (type(r) == "number" and num(r) or q(tostring(r)))
	rows[#rows + 1] = "[" .. q(fn) .. "," .. q(font or "") .. "," .. num(h) .. "," .. bytes(s) .. "," .. num(x) .. "," .. num(y) .. "," .. rs .. "]"
end
local function W(h, font, s)
	local ok, v = pcall(DrawStringWidth, h, font, s)
	row("w", font, h, s, nil, nil, ok and v or ("error: " .. tostring(v)))
end
local function C(h, font, s, x, y)
	local ok, v = pcall(DrawStringCursorIndex, h, font, s, x, y)
	row("c", font, h, s, x, y, ok and v or ("error: " .. tostring(v)))
end
local function S(s)
	local ok, v = pcall(StripEscapes, s)
	row("s", nil, nil, s, nil, nil, ok and v or ("error: " .. tostring(v)))
end
local function write(name, s) local f = assert(io.open(dir .. name, "wb")) f:write(s) f:close() end

local main, done = {}, false
SetMainObject(main)
function main:OnInit() RenderInit("DPI_AWARE") end
local body
function main:OnFrame()
	if done then return end
	done = true
	local ok, err = pcall(body)
	if not ok then local f = io.open(GetScriptPath() .. "/out/error.txt", "w") f:write(tostring(err)) f:close() end
	Exit()
end

function body()
	MakeDir(GetScriptPath() .. "/out/text-extra")
	local FONTS = { "FIXED", "VAR", "FONTIN SC" }
	-- A. cursor: line height (h vs effective even height), y boundaries incl. fractional y
	for _, font in ipairs({ "VAR" }) do
		for _, h in ipairs({ 9, 11, 13, 13.5, 15, 29, 33 }) do
			local y = -2
			while y <= 4 * (h + 1) + 4 do C(h, font, "a\nb\nc\nd", 0, y) y = y + 0.5 end
		end
	end
	-- A2. cursor: rounding of fractional y (quarter steps around line bottoms)
	for _, h in ipairs({ 13, 16 }) do
		local y = -1.5
		while y <= 3 * (h + 1) + 2 do C(h, "VAR", "a\nb\nc", 0, y) y = y + 0.25 end
	end
	-- B. cursor: pen accumulation (per-glyph ceil vs float) at non-unit scale; C. fractional x
	for _, font in ipairs(FONTS) do
		for _, h in ipairs({ 30, 34, 46, 100, 3, 13 }) do
			for _, s in ipairs({ "iiiiiiiiiiiiiiiiiiii", "Hello, World!", "a\tb\tc", "x\195\169y" }) do
				local w = DrawStringWidth(h, font, s)
				local step = (h == 13 or h == 30) and s == "Hello, World!" and 0.25 or 1
				local x = -1
				while x <= w + 2 do C(h, font, s, x, 0) x = x + step end
			end
		end
	end
	-- D. escapes: upper-case X, edge cases
	local ESC = { "^XFF8000A", "^Xff8000", "^X12345", "a^XABCDEFb", "^x^1FF00AA", "^1^x", "^x1^2", "^9^x000000", "^^^1", "^1\n^2", "^", "^xAbCdE", "^xABCDEF" }
	-- E. non-ASCII beyond BMP, invalid UTF-8, control codes
	local UNI = { "\240\159\152\128", "a\240\159\152\128b", "\244\143\191\191", "\255", "a\195", "\195(", "\226\130", "\128", "\1\2\3\127", "\r", "\0", "a\0b", "\239\187\191x" }
	for _, s in ipairs(ESC) do S(s) end
	for _, s in ipairs(UNI) do S(s) end
	for _, font in ipairs(FONTS) do
		for _, h in ipairs({ 10, 14, 16, 30, 64, 100 }) do
			for _, s in ipairs(ESC) do W(h, font, s) end
			for _, s in ipairs(UNI) do W(h, font, s) end
		end
		for _, s in ipairs(UNI) do
			for x = -1, DrawStringWidth(16, font, s) + 2 do C(16, font, s, x, 0) end
		end
	end
	-- F. heights: negative, rounding of halves, huge
	for _, font in ipairs(FONTS) do
		for _, h in ipairs({ -100, -10, -1.5, -1, -0.5, 0.49, 0.5, 0.51, 1.49, 1.5, 2.5, 3.5, 11.5, 12.49, 12.5, 29.5, 63.5, 64.5, 65, 200, 1000 }) do
			W(h, font, "Hello")
			W(h, font, "W")
			W(h, font, "\195\169")
			C(h, font, "ab\ncd", 3, 2)
		end
	end
	write("cases.json", "{\"rows\":[\n" .. table.concat(rows, ",\n") .. "\n]}\n")

	-- Perf: N calls each; GetTime() is ms.
	local N = 200000
	local function time(f)
		local t0 = GetTime()
		f()
		return GetTime() - t0
	end
	local s1 = "+12.5% increased Damage"
	local s2 = "^7The quick brown fox ^xFF8000jumps over the lazy dog"
	local p = {}
	p[#p + 1] = "\"n\":" .. N
	p[#p + 1] = "\"width_short_ms\":" .. time(function() for _ = 1, N do DrawStringWidth(16, "VAR", s1) end end)
	p[#p + 1] = "\"width_long_ms\":" .. time(function() for _ = 1, N do DrawStringWidth(16, "VAR", s2) end end)
	p[#p + 1] = "\"width_scaled_ms\":" .. time(function() for _ = 1, N do DrawStringWidth(30, "VAR", s1) end end)
	p[#p + 1] = "\"cursor_ms\":" .. time(function() for _ = 1, N do DrawStringCursorIndex(16, "VAR", s1, 80, 5) end end)
	p[#p + 1] = "\"strip_ms\":" .. time(function() for _ = 1, N do StripEscapes(s2) end end)
	p[#p + 1] = "\"gettime_ms\":" .. time(function() for _ = 1, N do GetTime() end end)
	write("perf.json", "{" .. table.concat(p, ",") .. "}\n")
end
