#@ SimpleGraphic
-- Behaviour probe: frame cadence (idle / animated / minimized), every input callback with arguments, IsKeyDown at events,
-- draw state at frame start. Input is POSTED to this window by inject.ps1 (no real mouse/keyboard use); inject.ps1 marks
-- each step with WM_CHAR "A".."Y" (step n = byte - 64), "Z" ends the run. Writes sg/out/measure-raw-<mode>.json with the
-- injector's own log embedded. Mode (2nd command-line arg): "window" (default, 1280x800 like scene-runner) | "desktop" |
-- "manual" (1280x800, no injector: a human uses the REAL mouse/keyboard for 30 s, then the run ends by itself).
local mode = select(2, ...) or "window"
if mode == "desktop" then -- SimpleGraphic defaults of a fresh SimpleGraphic.cfg: desktop-size window
	ConExecute("set vid_mode -1") ConExecute("set vid_resizable 0")
else
	ConExecute("set vid_mode 8") ConExecute("set vid_resizable 3") ConExecute("set vid_last 1280,800,40,40,0")
end
local json = require("dkjson")
local dir = GetScriptPath() .. "/"
local ev, frames = {}, {}
local trace = io.open(dir .. "trace.log", "w") -- progressive copy, survives a hang/kill
local function E(kind, ...)
	ev[#ev + 1] = { GetTime(), kind, ... }
	trace:write(json.encode(ev[#ev]), "\n") trace:flush()
end
local function bytes(s) local t = {} for i = 1, #s do t[i] = s:byte(i) end return t end
local function keys() return { ctrl = IsKeyDown("CTRL"), shift = IsKeyDown("SHIFT"), lb = IsKeyDown("LEFTBUTTON") } end

local main = { n = 0 }
SetMainObject(main)
function main:OnInit() RenderInit("DPI_AWARE") E("OnInit") end
function main:OnKeyDown(key, dbl) E("OnKeyDown", key, dbl or false, keys()) end
function main:OnKeyUp(key) E("OnKeyUp", key, keys()) end
function main:OnChar(ch)
	if not ch:match("^%u$") then return E("OnChar", ch, bytes(ch), keys()) end
	local n = ch:byte() - 64
	E("mark", n)
	self.anim = (n == 1) -- mark 1 starts the animated phase, mark 2 stops it
	if n == 26 then self:Finish() end
end
function main:CanExit() E("CanExit") return true end
function main:OnExit() E("OnExit") end
function main:OnFrame()
	if self.finished then return end
	local t = GetTime()
	self.n = self.n + 1
	-- draw state as the frame starts (previous frame ended with layer 7:3 and colour 1,0,0,0.5)
	local r, g, b, a = GetDrawColor()
	local ok, l, sl = pcall(GetDrawLayer)
	frames[#frames + 1] = { t, ok and l or tostring(l), sl or false, r, g, b, a }
	if self.n % 120 == 0 then trace:write("frame ", self.n, " ", t, "\n") trace:flush() end
	if self.n == 3 then local f = io.open(dir .. "ready", "w") f:write(t) f:close() end
	if mode == "manual" and t - frames[1][1] > 30000 then return self:Finish() end
	SetDrawColor(0.2, 0.2, 0.2)
	DrawImage(nil, 0, 0, 400, 100)
	SetDrawColor(1, 1, 1)
	DrawString(10, 10, "LEFT", 16, "VAR", "sg-oracle measure: posted input only")
	if self.anim then SetDrawColor(1, 0, 0) DrawImage(nil, t % 300, 50, 20, 20) end
	SetDrawLayer(7, 3)
	SetDrawColor(1, 0, 0, 0.5)
end
function main:Finish()
	self.finished = true
	local w, h = GetScreenSize()
	local fi = io.open(dir .. "inject.json", "rb")
	local inj = fi and json.decode(fi:read("*a")) or json.null
	if fi then fi:close() end
	local rt = GetRuntimePath()
	local function rel(p) return p and (p:sub(1, #rt) == rt and "<rt>" .. p:sub(#rt + 1) or p) end
	local paths = { script = rel(GetScriptPath()), runtime = "<rt>", work = rel(GetWorkDir()),
		package_path = rel(package.path:gsub("\\", "/"):gsub((rt:gsub("%p", "%%%0")), "<rt>")) }
	local res = { mode = mode, screen = { w, h }, scale = GetScreenScale(), paths = paths, inject = inj, events = ev, frames = frames }
	local f = io.open(dir .. "out/measure-raw-" .. mode .. ".json", "wb")
	f:write(json.encode(res, { indent = false }), "\n")
	f:close()
	Exit()
end
-- Wrap every callback: a Lua error is logged to sg/error.txt (and the event list) instead of stalling the run.
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
