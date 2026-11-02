#@ SimpleGraphic
-- U1.2 perf probe: CPU cost of recording a scene's draw calls in the original (Lua -> SimpleGraphic), per frame.
-- For each scene in the list: on one OnFrame, replay its calls N times between two GetTime() reads; then Exit().
-- Output: sg/out/perf/draw-perf.json {scene: {reps, ms_total, us_per_frame}}.
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local function readAll(p) local f = assert(io.open(p, "rb")) local s = f:read("*a") f:close() return s end
local names = { "text-fonts", "u12-holdout", "image-tint" }
local REPS = 300
MakeDir(base .. "/out/perf")
local main = { frame = 0, out = {} }
SetMainObject(main)
function main:OnInit()
	RenderInit("DPI_AWARE")
end

local function replay(s, handles)
	SetDrawLayer(0, 0) SetViewport() SetDrawColor(1, 1, 1, 1)
	for _, call in ipairs(s.calls) do
		if call[1] ~= "@log" then
			local args = {}
			for j = 2, #call do args[j - 1] = (call[j] ~= json.null) and call[j] or nil end
			if (call[1] == "DrawImage" or call[1] == "DrawImageQuad") and type(args[1]) == "string" then args[1] = handles[args[1]] end
			_G[call[1]](unpack(args, 1, #call - 1))
		end
	end
end

function main:OnFrame()
	self.frame = self.frame + 1
	local name = names[self.frame]
	if not name then
		local f = assert(io.open(base .. "/out/perf/draw-perf.json", "wb"))
		f:write(json.encode(self.out, { indent = true }), "\n")
		f:close()
		return Exit()
	end
	local s = json.decode(readAll(base .. "/scenes/" .. name .. ".json"), 1, json.null)
	local handles = {}
	for k, spec in pairs(s.images or {}) do
		local h = NewImageHandle()
		h:Load(unpack(spec))
		handles[k] = h
	end
	SetDPIScaleOverridePercent(s.dpi or 100)
	local t0 = GetTime()
	for _ = 1, REPS do replay(s, handles) end
	local ms = GetTime() - t0
	self.out[name] = { reps = REPS, ms_total = ms, us_per_frame = ms * 1000 / REPS }
end
