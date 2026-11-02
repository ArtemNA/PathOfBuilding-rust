#@ SimpleGraphic
-- U2a gate: runs the ORIGINAL Lua control classes (Classes/*.lua of the PoB checkout, byte-copied into sg/src/ first)
-- in the real SimpleGraphic and records screenshots + control state for scripted input scenarios.
--   pwsh tools/sg-oracle/run.ps1 -Script controls-runner.lua [-ScriptArgs <scenario,...>]
-- Scenarios: sg/scenes/controls/<name>.json = { "build": <builder in controls-gallery.lua>, "steps": [ step, ... ] };
-- a step (one frame) may set "cursor": [x, y], "time": ms, "hold": [keys], "release": [keys] (IsKeyDown state), push
-- "events": [["KeyDown", key, dbl?] | ["KeyUp", key] | ["Char", ch]], then ask "dump": true (state after the frame) and
-- "shot": "<name>" (TakeScreenshot of the next frame; the state is redrawn without events until the file appears).
-- GetCursorPos / GetTime / IsKeyDown are replaced by the scripted values (UI-ARCH §5.1 (в)); the frame = main:OnFrame order:
-- ProcessControlsInput(inputEvents, viewPort) then DrawControls(viewPort). Outputs: sg/out/controls/<shot>.png (full
-- 1280x800 client), sg/out/controls/<scenario>.json (dumps, errors).
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua") -- written by run.ps1 from tools/upstream.json
local POB = UPSTREAM.root .. "src/"
local SRC = base .. "/src/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local function readAll(p) local f = assert(io.open(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(io.open(p, "wb")) f:write(s) f:close() end

-- 1. Copy of the original sources (read-only use of the checkout).
local files = { "Modules/Common", "Modules/Main", "Data/Global" }
for _, c in ipairs({ "Control", "ControlHost", "LabelControl", "ButtonControl", "CheckBoxControl", "EditControl", "UndoHandler",
	"ResizableEditControl", "DropDownControl", "SliderControl", "ScrollBarControl", "ListControl", "TextListControl", "Tooltip",
	"TooltipHost", "PopupDialog", "SearchHost", "SectionControl", "RectangleOutlineControl", "DraggerControl" }) do
	files[#files + 1] = "Classes/" .. c
end
for _, d in ipairs({ "src", "src/Modules", "src/Data", "src/Classes", "out", "out/controls" }) do MakeDir(base .. "/" .. d) end
for _, f in ipairs(files) do writeAll(SRC .. f .. ".lua", readAll(POB .. f .. ".lua")) end

-- 2. Environment the classes expect.
function LoadModule(name, ...) return assert(loadfile(SRC .. name .. ".lua"))(...) end
function PLoadModule(name, ...) return pcall(LoadModule, name, ...) end
launch = { devMode = false }
LoadModule("Modules/Common")
LoadModule("Data/Global")
main = { showFlavourText = true, notSupportedModTooltips = true }
do -- main:DrawArrow / main:DrawCheckMark (Main.lua:1497-1521) and main:WrapString (Main.lua:1795-1820), verbatim
	local src = readAll(SRC .. "Modules/Main.lua")
	local a = src:find("function main:DrawArrow", 1, true)
	local b = src:find("\ndo\r?\n\tlocal cos45", a)
	local c = src:find("\ndo\r?\n\tlocal wrapTable", b)
	local d = src:find("\nreturn main", c, true)
	assert(a and b and c and d, "Main.lua helpers not found")
	assert(loadstring("local t_insert = table.insert\n" .. src:sub(a, b) .. src:sub(c, d), "=Main.lua helpers"))()
end
local realGetTime, realIsKeyDown = GetTime, IsKeyDown
local cursorX, cursorY, nowMs, held = -100, -100, 0, {}
function GetCursorPos() return cursorX, cursorY end
function GetTime() return nowMs end
function IsKeyDown(k) return held[k] == true end
local clipboard -- scripted clipboard: the real one is never touched
function Copy(text) clipboard = text end
function Paste() return clipboard end
local gallery = assert(loadfile(base .. "/controls-gallery.lua"))()

-- 3. Scenario loop.
local names = json.decode(readAll(base .. "/scenes/controls/index.json"))
if select("#", ...) >= 2 then names = {} for n in tostring(select(2, ...)):gmatch("[^,]+") do names[#names + 1] = n end end
local function listShots()
	local t, h = {}, NewFileSearch(shotDir .. "*.png")
	while h do t[h:GetFileName()] = true if not h:NextFile() then break end end
	return t
end

local runner = { idx = 0 }
SetMainObject(runner)
function runner:OnInit() RenderInit("DPI_AWARE") ConExecute("set r_screenshotFormat png") SetDPIScaleOverridePercent(100) end

function runner:Begin()
	self.idx = self.idx + 1
	local name = names[self.idx]
	if not name then return false end
	local sc = json.decode(readAll(base .. "/scenes/controls/" .. name .. ".json"), 1, json.null)
	cursorX, cursorY, nowMs, held, clipboard = -100, -100, 0, {}, nil
	local env = gallery[sc.build]()
	self.sc = { name = name, def = sc, env = env, step = 0, dumps = {}, errors = {}, shots = {}, frames = 0 }
	return true
end

function runner:Frame(events)
	local env = self.sc.env
	local inputEvents = {}
	for _, e in ipairs(events or {}) do
		if e[1] == "KeyDown" then inputEvents[#inputEvents + 1] = { type = "KeyDown", key = e[2], doubleClick = e[3] == true }
		else inputEvents[#inputEvents + 1] = { type = e[1], key = e[2] } end
	end
	local vp = { x = 0, y = 0, width = 1280, height = 800 }
	SetDrawLayer(0, 0) SetViewport() SetDrawColor(1, 1, 1, 1)
	local ok, err = pcall(function()
		env.host:ProcessControlsInput(inputEvents, vp)
		env.host:DrawControls(vp)
	end)
	if not ok then self.sc.errors[#self.sc.errors + 1] = string.format("step %d: %s", self.sc.step, tostring(err)) end
end

function runner:OnFrame()
	if self.finished then return end
	if not self.sc and not self:Begin() then return self:Finish() end
	local sc = self.sc
	sc.frames = sc.frames + 1
	if sc.frames < 3 then return self:Frame() end -- settle (DPI override applies next frame)
	if sc.waitShot then
		self:Frame()
		if not sc.shotAt then
			if realGetTime() - (self.lastShot or -2000) >= 1100 then
				TakeScreenshot() sc.shotAt = realGetTime() self.lastShot = sc.shotAt
			end
			return
		end
		for f in pairs(listShots()) do
			if not sc.seen[f] then
				local ok, err = os.rename(shotDir .. f, base .. "/out/controls/" .. sc.waitShot .. ".png")
				if not ok then sc.errors[#sc.errors + 1] = "rename: " .. tostring(err) end
				sc.shots[#sc.shots + 1] = sc.waitShot
				sc.waitShot, sc.shotAt = nil, nil
				return
			end
		end
		if realGetTime() - sc.shotAt > 10000 then sc.errors[#sc.errors + 1] = "no screenshot for " .. sc.waitShot sc.waitShot, sc.shotAt = nil, nil end
		return
	end
	sc.step = sc.step + 1
	local st = sc.def.steps[sc.step]
	if not st then
		writeAll(base .. "/out/controls/" .. sc.name .. ".json", json.encode({ name = sc.name, dumps = sc.dumps, shots = sc.shots, errors = sc.errors },
			{ indent = true, keyorder = { "name", "step", "dumps", "shots", "errors" } }) .. "\n")
		self.sc = nil
		return
	end
	if st.cursor then cursorX, cursorY = st.cursor[1], st.cursor[2] end
	if st.time then nowMs = st.time end
	for _, k in ipairs(st.hold or {}) do held[k] = true end
	for _, k in ipairs(st.release or {}) do held[k] = nil end
	self:Frame(st.events)
	if st.dump then
		local ok, d = pcall(sc.env.dump)
		sc.dumps[#sc.dumps + 1] = { step = sc.step, state = ok and d or ("dump error: " .. tostring(d)) }
	end
	if st.shot then sc.waitShot, sc.seen = st.shot, listShots() end
end

function runner:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	Exit()
end
