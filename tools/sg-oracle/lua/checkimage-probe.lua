#@ SimpleGraphic
-- Probe of `DrawImage(handle, x, y, w, h, n)` with ONE argument after the rect (CheckBoxControl.lua:81 at 5606cc64f passes
-- `self.checkImage[1]` = 1): what SimpleGraphic does with a 6th argument, next to the 5-argument and 9-argument forms.
-- Draws Assets/monstercategory/categorybeasts.png (34x34) of the upstream checkout on a black screen, one form per cell,
-- small (24x24, the check box size - 2) and large (240x240), takes one screenshot -> sg/out/checkimage-probe/shot.png and
-- logs each call's pcall result and GetDrawColor() after SetDrawColor(2, 2, 2) -> sg/out/checkimage-probe/log.json.
--   pwsh tools/sg-oracle/run.ps1 -Script checkimage-probe.lua -Timeout 60
ConExecute("set vid_mode 8") ConExecute("set vid_resizable 3") ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua")
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local OUT = base .. "/out/checkimage-probe/"
MakeDir(base .. "/out")
MakeDir(OUT)
local function listShots()
	local t, h = {}, NewFileSearch(shotDir .. "*.png")
	while h do t[h:GetFileName()] = true if not h:NextFile() then break end end
	return t
end
local log = { calls = {} }
local main = { n = 0 }
SetMainObject(main)
local img
-- { label, x, y, size, extra args after the rect }
local CELLS = {
	{ "5 args", 10, 10, 24, {} },
	{ "6 args: 1", 50, 10, 24, { 1 } },
	{ "6 args: 0", 90, 10, 24, { 0 } },
	{ "6 args: 2", 130, 10, 24, { 2 } },
	{ "9 args: 1,0,0,0", 170, 10, 24, { 1, 0, 0, 0 } },
	{ "9 args: 0,0,1,1", 210, 10, 24, { 0, 0, 1, 1 } },
	{ "7 args: 1,0", 250, 10, 24, { 1, 0 } },
	{ "5 args large", 10, 60, 240, {} },
	{ "6 args: 1 large", 260, 60, 240, { 1 } },
	{ "9 args: 1,0,0,0 large", 510, 60, 240, { 1, 0, 0, 0 } },
	{ "6 args: 2 large", 760, 60, 240, { 2 } },
	{ "10 args: 0,0,1,1,1 large", 1010, 60, 240, { 0, 0, 1, 1, 1 } },
	-- the hovered checked box: SetDrawColor(2, 2, 2) (CheckBoxControl.lua:75), next to 1 and 0.5
	{ "6 args: 1, colour 2 large", 10, 320, 240, { 1 }, 2 },
	{ "6 args: 1, colour 1 large", 260, 320, 240, { 1 }, 1 },
	{ "6 args: 1, colour 0.5 large", 510, 320, 240, { 1 }, 0.5 },
}
function main:OnInit()
	RenderInit("DPI_AWARE")
	ConExecute("set r_screenshotFormat png")
	img = NewImageHandle()
	img:Load(UPSTREAM.root .. "src/Assets/monstercategory/categorybeasts.png")
	log.imageSize = { img:ImageSize() }
end
function main:OnFrame()
	if self.finished then return end
	self.n = self.n + 1
	SetDrawColor(0, 0, 0)
	DrawImage(nil, 0, 0, 1280, 800)
	for _, c in ipairs(CELLS) do
		local k = c[6] or 1
		SetDrawColor(k, k, k)
		local ok, err = pcall(DrawImage, img, c[2], c[3], c[4], c[4], unpack(c[5]))
		if self.n == 1 then
			log.calls[#log.calls + 1] = { label = c[1], rect = { c[2], c[3], c[4], c[4] }, ok = ok, err = ok and json.null or tostring(err) }
		end
	end
	if self.n == 1 then
		SetDrawColor(2, 2, 2)
		log.colorAfter2 = { GetDrawColor() }
	end
	if self.n == 10 then
		self.seen = listShots()
		TakeScreenshot()
	elseif self.n > 10 then
		for f in pairs(listShots()) do
			if not self.seen[f] then
				os.rename(shotDir .. f, OUT .. "shot.png")
				return self:Finish()
			end
		end
		if self.n > 600 then return self:Finish() end
	end
end
function main:Finish()
	self.finished = true
	local f = assert(io.open(OUT .. "log.json", "wb"))
	f:write(json.encode(log, { indent = true }), "\n")
	f:close()
	Exit()
end
