#@ SimpleGraphic
-- Exact corpus of the pure text functions of the original SimpleGraphic -> sg/out/text/*.json
--   strip.json   StripEscapes(s) for every corpus string
--   glyphs.json  DrawStringWidth(h, font, char) for printable ASCII 32..126, every font, h = 1..80 (+ fractional)
--   width.json   DrawStringWidth(h, font, s) for every corpus string, font, height
--   cursor.json  DrawStringCursorIndex(h, font, s, x, y) for x = -3..width+3 (row arrays), several y
-- Strings are valid UTF-8 (JSON-safe). Run: pwsh tools/sg-oracle/run.ps1 -Script text-corpus.lua
local dir = GetScriptPath() .. "/out/text/"
local json = require("dkjson")
local q = json.quotestring

local FONTS = { "FIXED", "VAR", "VAR BOLD", "FONTIN SC", "FONTIN SC ITALIC", "FONTIN", "FONTIN ITALIC" }
local HEIGHTS = {}
for h = 1, 80 do HEIGHTS[#HEIGHTS + 1] = h end
for _, h in ipairs({ 0, 10.5, 12.5, 13.5, 14.4, 15.99, 16.01, 18.75, 21, 24.5, 100, 128 }) do HEIGHTS[#HEIGHTS + 1] = h end
local STRINGS = {
	"", " ", "a", "W", "i", "Hello", "Hello World", "The quick brown fox jumps over the lazy dog",
	"0123456789", "+12.5% increased Damage", "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~",
	"    spaces    ", "iiiiiiii", "MMMMMMMM", "tab\there", "cr\rlf", "\r\n",
	"^7White", "^1Red^2Green^3Blue", "^0^1^2^3^4^5^6^7^8^9", "^xFF8000Orange", "^xff8000lower", "^xAbCdEfMixed",
	"^x12345", "^x1234567", "^xGGGGGGbad", "^", "a^", "^^1", "^a", "^10", "1^2 3", "^x", "x^xFF00", "^ x",
	"line1\nline2", "short\nmuch longer second line", "a\n\nb", "\n", "trail\n", "\nlead", "^1red\nstill",
	"é", "Café", "Привіт", "日本", "€100", "a–b", "Ünïcödé ^1mix", "\226\128\148",
}
local CURSOR_STRINGS = { "Hello World", "^1Red ^7White", "line1\nline2 longer", "Café", "iiiWWW", "", "a b", "^xFF0000X^7Y", "a\n\nbc" }
local CURSOR_HEIGHTS = { 10, 12, 13, 14, 16, 18, 20, 24, 32 }

local function arr(t, f) local o = {} for i, v in ipairs(t) do o[i] = f and f(v) or tostring(v) end return "[" .. table.concat(o, ",") .. "]" end
local function num(v)
	if type(v) ~= "number" then return "null" end
	return (v == math.floor(v) and string.format("%d", v)) or string.format("%.17g", v)
end
local errors = {}
local function W(h, font, s)
	local ok, v = pcall(DrawStringWidth, h, font, s)
	if not ok then errors[#errors + 1] = q("width " .. font .. " " .. h .. ": " .. tostring(v)) return nil end
	return v
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
	local t0 = GetTime()
	MakeDir(GetScriptPath() .. "/out/text")
	-- strip
	local o = { "{\"cases\":[" }
	for i, s in ipairs(STRINGS) do o[#o + 1] = (i > 1 and "," or "") .. "\n[" .. q(s) .. "," .. q(StripEscapes(s)) .. "]" end
	write("strip.json", table.concat(o) .. "\n]}\n")
	-- glyphs: per font, per height, widths of chars 32..126
	o = { "{\"chars_from\":32,\"chars_to\":126,\"heights\":" .. arr(HEIGHTS, num) .. ",\"fonts\":{" }
	for fi, font in ipairs(FONTS) do
		o[#o + 1] = (fi > 1 and "," or "") .. "\n" .. q(font) .. ":["
		for hi, h in ipairs(HEIGHTS) do
			local w = {}
			for c = 32, 126 do w[#w + 1] = W(h, font, string.char(c)) or false end
			o[#o + 1] = (hi > 1 and "," or "") .. "\n" .. arr(w, num)
		end
		o[#o + 1] = "]"
	end
	write("glyphs.json", table.concat(o) .. "}}\n")
	-- width: fonts x heights x strings
	o = { "{\"heights\":" .. arr(HEIGHTS, num) .. ",\"strings\":" .. arr(STRINGS, q) .. ",\"fonts\":{" }
	for fi, font in ipairs(FONTS) do
		o[#o + 1] = (fi > 1 and "," or "") .. "\n" .. q(font) .. ":["
		for hi, h in ipairs(HEIGHTS) do
			local w = {}
			for si, s in ipairs(STRINGS) do w[si] = W(h, font, s) or false end
			o[#o + 1] = (hi > 1 and "," or "") .. "\n" .. arr(w, num)
		end
		o[#o + 1] = "]"
	end
	write("width.json", table.concat(o) .. "}}\n")
	-- cursor: rows {font, h, s, y, x0, [idx for x = x0 .. x0+n-1]}
	o = { "{\"rows\":[" }
	local n = 0
	for _, font in ipairs(FONTS) do
		for _, h in ipairs(CURSOR_HEIGHTS) do
			for _, s in ipairs(CURSOR_STRINGS) do
				local w = DrawStringWidth(h, font, s)
				for _, y in ipairs({ -3, 0, math.floor(h / 2), h - 1, h, h + math.floor(h / 2), 2 * h, 3 * h + 1 }) do
					local r = {}
					for x = -3, w + 3 do r[#r + 1] = DrawStringCursorIndex(h, font, s, x, y) end
					o[#o + 1] = (n > 0 and "," or "") .. "\n[" .. q(font) .. "," .. h .. "," .. q(s) .. "," .. y .. ",-3," .. arr(r, num) .. "]"
					n = n + 1
				end
			end
		end
	end
	write("cursor.json", table.concat(o) .. "\n]}\n")
	write("errors.json", "[" .. table.concat(errors, ",\n") .. "]\n")
	write("meta.json", string.format("{\"elapsed_ms\":%d,\"cursor_rows\":%d,\"width_errors\":%d}\n", GetTime() - t0, n, #errors))
end
