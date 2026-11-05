#@ SimpleGraphic
-- How often the original calls OnFrame while mouse messages arrive at a high rate. A changing draw call keeps frames from being elided.
-- Marks are WM_CHAR "A".."Y" (the phase number), "Z" ends the run. Input is posted to this window only (inject-frame-rate.ps1).
ConExecute("set vid_mode 8") ConExecute("set vid_resizable 3") ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local dir = GetScriptPath() .. "/"
local marks, frames = {}, {}
STATIC = select(2, ...) == "static" -- the 2nd command-line arg: the same draw list every frame
local main = {}
SetMainObject(main)
function main:OnInit()
	RenderInit("DPI_AWARE")
	local f = io.open(dir .. "ready", "w") f:write("1") f:close()
end
function main:OnChar(ch)
	if ch == "Z" then
		local f = io.open(dir .. "out/frame-rate.json", "w")
		f:write(json.encode({ marks = marks, frames = frames }))
		f:close()
		self.done = true
		Exit()
	elseif ch:match("^%u$") then
		marks[#marks + 1] = { ch:byte() - 64, GetTime() }
	end
end
function main:OnFrame()
	if self.done then return end
	local t = GetTime()
	frames[#frames + 1] = t
	SetDrawColor(1, 1, 1)
	if STATIC then
		DrawImage(nil, 100, 100, 20, 20)
		return
	end
	local x, y = GetCursorPos()
	DrawString(10, 10, "LEFT", 16, "VAR", t .. " " .. x .. "," .. y)
	DrawImage(nil, x, y, 20, 20)
end
