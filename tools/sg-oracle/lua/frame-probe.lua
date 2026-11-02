#@ SimpleGraphic
-- U1.3 probe of host state across frames + TakeScreenshot, no input. Phases (each ends with a screenshot moved to
-- sg/out/frame-probe/<phase>.png; shots >= 1.1 s apart because SimpleGraphic names them by the second):
--   shot     — which frame TakeScreenshot captures: frames before the call are green, the calling frame red (call after
--              drawing), later frames blue.
--   viewport — viewport/layer at frame start: every frame first fills the screen yellow WITHOUT SetViewport, then ends
--              with SetViewport(100, 100, 50, 50) + SetDrawLayer(3, 4) + SetDrawColor(0, 0, 1, 0.5).
--   dpi      — SetDPIScaleOverridePercent(150) mid-frame in the frame AFTER the TakeScreenshot call (the one the shot
--              captures, see "shot"): black screen, green rect (10,10,20,20) drawn before the call, red rect
--              (100,100,100,100) after it.
-- Also logs SetDrawColor(escapeStr) variants (pcall + GetDrawColor) and GetDrawLayer at frame start.
-- Writes sg/out/frame-probe/log.json.
ConExecute("set vid_mode 8") ConExecute("set vid_resizable 3") ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
MakeDir(base .. "/out/frame-probe")
local function listShots()
	local t, h = {}, NewFileSearch(shotDir .. "*.png")
	while h do t[h:GetFileName()] = true if not h:NextFile() then break end end
	return t
end
local log = { colors = {}, frames = {} }
local main = { n = 0, phase = 1 }
SetMainObject(main)
local phases = { "shot", "viewport", "dpi" }
function main:OnInit()
	RenderInit("DPI_AWARE")
	ConExecute("set r_screenshotFormat png")
end
local function colorProbe()
	for _, s in ipairs({ "^1", "^1abc", "^xFF8000", "^xff8000zz", "^X00FF00", "^", "^x", "^x12345", "^xGG0000", "1", "", " ^1",
		"^1^2", "^x00FF00^1", "^a", "^^1", "^xFF80001" }) do
		SetDrawColor(0.25, 0.5, 0.75, 0.5)
		local ok, err = pcall(SetDrawColor, s)
		log.colors[#log.colors + 1] = { s, ok, ok and json.null or tostring(err), { GetDrawColor() } }
	end
	-- numeric forms: 3 args (alpha default), extra args
	SetDrawColor(0.25, 0.5, 0.75, 0.5) SetDrawColor(1, 0, 0)
	log.colors[#log.colors + 1] = { "(1,0,0)", true, json.null, { GetDrawColor() } }
end
function main:OnFrame()
	if self.finished then return end
	self.n = self.n + 1
	local l, sl = GetDrawLayer()
	local now = GetTime()
	if self.n == 2 then colorProbe() end
	local ph = phases[self.phase]
	if not ph then return self:Finish() end
	self.pf = (self.pf or 0) + 1
	if self.n <= 60 then log.frames[#log.frames + 1] = { self.n, ph, self.pf, l, sl or json.null, GetScreenScale() } end
	local shoot = false
	if not self.shotAt and self.pf >= 8 and now - (self.lastShot or -2000) >= 1100 then shoot = true end
	if ph == "shot" then
		if self.shotAt then SetDrawColor(0, 0, 1) elseif shoot then SetDrawColor(1, 0, 0) else SetDrawColor(0, 1, 0) end
		DrawImage(nil, 0, 0, 1280, 800)
	elseif ph == "viewport" then
		SetDrawColor(1, 1, 0)
		DrawImage(nil, 0, 0, 1280, 800)
	elseif ph == "dpi" then
		SetDrawColor(0, 0, 0) DrawImage(nil, 0, 0, 2000, 2000)
		SetDrawColor(0, 1, 0) DrawImage(nil, 10, 10, 20, 20)
		if self.shotAt and not log.dpiChange then
			local before = GetScreenScale()
			SetDPIScaleOverridePercent(150)
			log.dpiChange = { frame = self.n, before = before, after = GetScreenScale(), screen = { GetScreenSize() } }
		end
		SetDrawColor(1, 0, 0) DrawImage(nil, 100, 100, 100, 100)
	end
	if shoot then
		self.seen = listShots()
		TakeScreenshot()
		self.shotAt, self.lastShot = now, now
		log[ph] = log[ph] or {}
		log[ph].shotFrame = self.n
	elseif self.shotAt then
		for f in pairs(listShots()) do
			if not self.seen[f] then
				os.rename(shotDir .. f, base .. "/out/frame-probe/" .. ph .. ".png")
				log[ph].file, log[ph].foundAfterFrames = f, self.n - log[ph].shotFrame
				if ph == "dpi" then SetDPIScaleOverridePercent(100) end
				self.phase, self.shotAt, self.pf = self.phase + 1, nil, 0
				break
			end
		end
	end
	if ph == "viewport" then
		SetViewport(100, 100, 50, 50) SetDrawLayer(3, 4) SetDrawColor(0, 0, 1, 0.5)
	end
end
function main:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	local f = assert(io.open(base .. "/out/frame-probe/log.json", "wb"))
	f:write(json.encode(log, { indent = true }), "\n")
	f:close()
	Exit()
end
