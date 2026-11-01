#@ SimpleGraphic
-- Replays JSON scenes (sg/scenes/<name>.json, order from index.json) in the real SimpleGraphic and screenshots each one.
-- Scene: {"size":[w,h] (virtual px), "dpi":100|150, "images":{name:[path, flags...]}, "calls":[[api, args...], ...]}
--   null -> nil; a string first arg of DrawImage/DrawImageQuad names an image; ["@log","GetDrawColor"] records the call's results.
-- Every frame starts with SetDrawLayer(0,0), SetViewport(), SetDrawColor(1,1,1,1) (runner reset), then the scene calls.
-- TakeScreenshot names files by the second (SimpleGraphic/Screenshots/%m%d%y_%H%M%S.png) -> shots >= 1.1 s apart; each new
-- file is moved to sg/out/scenes/<name>.png. run.ps1 then crops it to size*dpi/100 (top-left) per manifest.json.
-- Fixed window: same console vars as Launch.lua:12-13 plus vid_last "w,h,x,y,maximized" -> 1280x800 client area.
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local function readAll(p) local f = assert(io.open(p, "rb")) local s = f:read("*a") f:close() return s end
local names = json.decode(readAll(base .. "/scenes/index.json"))
if select("#", ...) >= 2 then -- optional 2nd command-line arg: comma-separated subset of scenes
	names = {}
	for n in tostring(select(2, ...)):gmatch("[^,]+") do names[#names + 1] = n end
end
MakeDir(base .. "/out/scenes")

local function listShots()
	local t, h = {}, NewFileSearch(shotDir .. "*.png")
	while h do t[h:GetFileName()] = true if not h:NextFile() then break end end
	return t
end

local main = { idx = 0, manifest = {} }
SetMainObject(main)
function main:OnInit()
	RenderInit("DPI_AWARE")
	ConExecute("set r_screenshotFormat png")
end

function main:Begin()
	self.idx = self.idx + 1
	local name = names[self.idx]
	if not name then return false end
	local s = json.decode(readAll(base .. "/scenes/" .. name .. ".json"), 1, json.null)
	s.name, s.frames, s.logs, s.errors, s.handles = name, 0, {}, {}, {}
	for k, spec in pairs(s.images or {}) do
		local h = NewImageHandle()
		local ok, err = pcall(h.Load, h, unpack(spec))
		if not ok then s.errors[#s.errors + 1] = "load " .. k .. ": " .. tostring(err) end
		s.handles[k] = h
	end
	SetDPIScaleOverridePercent(s.dpi or 100)
	self.scene, self.seen = s, listShots()
	return true
end

function main:Draw(s, record)
	SetDrawLayer(0, 0) SetViewport() SetDrawColor(1, 1, 1, 1)
	for i, call in ipairs(s.calls) do
		local args = {}
		for j = 2, #call do args[j - 1] = (call[j] ~= json.null) and call[j] or nil end
		if call[1] == "@log" then
			if record then s.logs[#s.logs + 1] = { i, call[2], { _G[call[2]]() } } end
		else
			if (call[1] == "DrawImage" or call[1] == "DrawImageQuad") and type(args[1]) == "string" then args[1] = s.handles[args[1]] end
			local ok, err = pcall(_G[call[1]], unpack(args, 1, #call - 1))
			if not ok and record then s.errors[#s.errors + 1] = string.format("call %d %s: %s", i, call[1], tostring(err)) end
		end
	end
end

function main:OnFrame()
	if self.finished then return end
	local now = GetTime()
	if not self.scene and not self:Begin() then return self:Finish() end
	local s = self.scene
	s.frames = s.frames + 1
	self:Draw(s, s.frames == 1)
	if not s.shotAt then
		-- settle >= 5 frames (DPI override applies next frame) and keep shots >= 1100 ms apart (per-second file names)
		if s.frames >= 5 and now - (self.lastShot or -2000) >= 1100 then
			local w, h = GetScreenSize()
			s.screen, s.scale, s.virtual = { w, h }, GetScreenScale(), { GetVirtualScreenSize and GetVirtualScreenSize() }
			TakeScreenshot()
			s.shotAt, s.shotFrame, self.lastShot = now, s.frames, now
		end
		return
	end
	for f in pairs(listShots()) do
		if not self.seen[f] then
			local ok, err = os.rename(shotDir .. f, base .. "/out/scenes/" .. s.name .. ".png")
			if not ok then s.errors[#s.errors + 1] = "rename: " .. tostring(err) end
			s.file, s.foundFrame = f, s.frames
			local m = { name = s.name, shot = f, size = s.size, dpi = s.dpi, screen = s.screen, scale = s.scale,
				frames_before_shot = s.shotFrame, frames_until_file = s.foundFrame - s.shotFrame, logs = s.logs, errors = s.errors }
			self.manifest[#self.manifest + 1] = m
			self.scene = nil
			return
		end
	end
	if now - s.shotAt > 10000 then s.errors[#s.errors + 1] = "no screenshot file after 10 s" self.manifest[#self.manifest + 1] = { name = s.name, errors = s.errors } self.scene = nil end
end

function main:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	local f = assert(io.open(base .. "/out/scenes/manifest.json", "wb"))
	f:write(json.encode(self.manifest, { indent = true, keyorder = { "name", "shot", "size", "dpi", "screen", "scale" } }), "\n")
	f:close()
	Exit()
end
