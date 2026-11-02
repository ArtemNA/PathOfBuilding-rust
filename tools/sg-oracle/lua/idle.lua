#@ SimpleGraphic
-- U1.5 idle probe: draws scene u15-demo (as scene-runner.lua does) every frame for 14 s, then Exit(). Run with
-- inject-cpu.ps1, which samples the process CPU time between 2 s and 12 s (no input is posted).
--   pwsh tools/sg-oracle/run.ps1 -Script idle.lua -Inject inject-cpu.ps1
ConExecute("set vid_mode 8") ConExecute("set vid_resizable 3") ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local function readAll(p) local f = assert(io.open(p, "rb")) local s = f:read("*a") f:close() return s end
local s = json.decode(readAll(base .. "/scenes/u15-demo.json"), 1, json.null)
local handles = {}
local main = { n = 0 }
-- errors go to sg/error.txt instead of stalling the run
local function guard(name, f) return function(self, ...) local ok, e = pcall(f, self, ...) if not ok then local h = io.open(base .. "/error.txt", "a") h:write(name, ": ", tostring(e), "\n") h:close() Exit() end end end
SetMainObject(main)
function main:OnInit()
	RenderInit("DPI_AWARE")
	-- images load after the renderer exists (a Load at script top level leaves the handle empty)
	for k, spec in pairs(s.images or {}) do local h = NewImageHandle() h:Load(unpack(spec)) handles[k] = h end
	self.t0 = GetTime()
end
function main:OnFrame()
	self.n = self.n + 1
	if self.n == 3 then local f = io.open(base .. "/ready", "w") f:write(GetTime()) f:close() end
	SetDrawLayer(0, 0) SetViewport() SetDrawColor(1, 1, 1, 1)
	for _, call in ipairs(s.calls) do
		local args = {}
		for j = 2, #call do args[j - 1] = (call[j] ~= json.null) and call[j] or nil end
		if (call[1] == "DrawImage" or call[1] == "DrawImageQuad") and type(args[1]) == "string" then args[1] = handles[args[1]] end
		_G[call[1]](unpack(args, 1, #call - 1))
	end
	if GetTime() - self.t0 > 14000 then
		local f = io.open(base .. "/out/idle-frames.json", "wb") f:write(json.encode({ frames = self.n, ms = GetTime() - self.t0 })) f:close()
		Exit()
	end
end

main.OnInit = guard("OnInit", main.OnInit)
main.OnFrame = guard("OnFrame", main.OnFrame)
