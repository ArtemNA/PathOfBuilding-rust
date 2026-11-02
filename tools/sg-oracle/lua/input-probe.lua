#@ SimpleGraphic
-- U1.4 input probe (extends measure.lua): every callback with arguments + IsKeyDown of several names + GetCursorPos at the
-- event, so the key-name table, modifiers, double-click, wheel and cursor/DPI behaviour can be replayed by pob-gfx tests.
-- Input is POSTED to this window only (inject-probe.ps1; no real mouse/keyboard use). Step marks are WM_CHAR "A".."Y"
-- (step n = byte - 64), "Z" ends the run. Steps 2/3 switch SetDPIScaleOverridePercent to 150/100 on the next frame.
-- 2nd command-line arg = output tag (default "raw"): writes sg/out/input-probe-<tag>.json with the injector's own log embedded.
-- 3rd command-line arg (optional): window position "x,y" (default 40,40), e.g. to put the idle OS cursor inside the client.
local winPos = select(3, ...) or "40,40"
ConExecute("set vid_mode 8") ConExecute("set vid_resizable 3") ConExecute("set vid_last 1280,800," .. winPos .. ",0")
local outTag = select(2, ...) or "raw"
local json = require("dkjson")
local dir = GetScriptPath() .. "/"
local ev, frames = {}, {}
local trace = io.open(dir .. "trace.log", "w")
local names = { "CTRL", "SHIFT", "ALT", "LEFTBUTTON", "RIGHTBUTTON", "MIDDLEBUTTON", "MOUSE4", "a", "UP", "A" }
local function state()
	local k = {}
	for _, n in ipairs(names) do
		local ok, v = pcall(IsKeyDown, n)
		k[#k + 1] = ok and (v and 1 or 0) or ("err " .. tostring(v))
	end
	local x, y = GetCursorPos()
	return { k, x, y }
end
local function E(kind, ...)
	ev[#ev + 1] = { GetTime(), kind, ... }
	trace:write(json.encode(ev[#ev]), "\n") trace:flush()
end
local function bytes(s) local t = {} for i = 1, #s do t[i] = s:byte(i) end return t end

local main = { n = 0 }
SetMainObject(main)
function main:OnInit()
	RenderInit("DPI_AWARE")
	local errs = {}
	for _, n in ipairs({ "", "FOO", "A", "Z", "LEFT", "F15", "F16", "NUMLOCK", "SCROLLLOCK", " " }) do
		local ok, v = pcall(IsKeyDown, n)
		errs[#errs + 1] = { n, ok, tostring(v) }
	end
	E("OnInit", names, errs, GetTime())
end
function main:OnKeyDown(key, dbl) E("OnKeyDown", key, dbl or false, state()) end
function main:OnKeyUp(key) E("OnKeyUp", key, state()) end
function main:OnChar(ch)
	-- in "chars" mode upper-case letters are data, not marks (the run ends when the injector writes sg/done)
	if outTag == "chars" or not ch:match("^%u$") then return E("OnChar", ch, bytes(ch), state()) end
	local n = ch:byte() - 64
	E("mark", n, state())
	if n == 2 then self.dpi = 150 elseif n == 3 then self.dpi = 100 end
	if n == 25 then TakeScreenshot() end -- written with the next frame; Finish moves it to out/input-probe-<tag>.png
	if n == 26 then self:Finish() end
end
function main:CanExit() E("CanExit") return true end
function main:OnExit() E("OnExit") end
function main:OnFrame()
	if self.finished then return end
	local t = GetTime()
	self.n = self.n + 1
	if outTag == "chars" and self.n % 10 == 0 then local f = io.open(dir .. "done", "rb") if f then f:close() return self:Finish() end end
	if self.dpi then
		local dpi = self.dpi
		self.dpi = nil
		local before = GetScreenScale()
		SetDPIScaleOverridePercent(dpi)
		local sw, sh = GetScreenSize()
		local s = GetScreenScale()
		local vw, vh = math.floor(sw / s), math.floor(sh / s) -- Common.lua:1118-1126
		E("dpi", dpi, before, GetScreenScale(), { GetScreenSize() }, { vw, vh }, state())
	end
	local x, y = GetCursorPos()
	if x ~= self.lx or y ~= self.ly then E("cursor", x, y, GetScreenScale()) self.lx, self.ly = x, y end
	frames[#frames + 1] = t
	if self.n == 3 then local f = io.open(dir .. "ready", "w") f:write(t) f:close() end
	SetDrawColor(0.2, 0.2, 0.2)
	DrawImage(nil, 0, 0, 400, 100)
	SetDrawColor(1, 1, 1)
	DrawString(10, 10, "LEFT", 16, "VAR", "sg-oracle input-probe: posted input only")
end
function main:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	local w, h = GetScreenSize()
	local fi = io.open(dir .. "inject.json", "rb")
	local inj = fi and json.decode(fi:read("*a")) or json.null
	if fi then fi:close() end
	local shots = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
	local hs = NewFileSearch(shots .. "*.png")
	if hs then os.rename(shots .. hs:GetFileName(), dir .. "out/input-probe-" .. outTag .. ".png") end
	local res = { screen = { w, h }, scale = GetScreenScale(), names = names, inject = inj, events = ev, frames = #frames }
	local f = io.open(dir .. "out/input-probe-" .. outTag .. ".json", "wb")
	f:write(json.encode(res, { indent = false }), "\n")
	f:close()
	Exit()
end
for _, name in ipairs({ "OnInit", "OnFrame", "OnKeyDown", "OnKeyUp", "OnChar", "CanExit", "OnExit" }) do
	local f = main[name]
	main[name] = function(self, ...)
		local ok, a = pcall(f, self, ...)
		if not ok then
			E("error", name, tostring(a))
			local fh = io.open(dir .. "error.txt", "a") fh:write(name, ": ", tostring(a), "\n") fh:close()
			return
		end
		return a
	end
end
