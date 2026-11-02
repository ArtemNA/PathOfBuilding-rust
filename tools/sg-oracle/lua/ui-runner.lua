#@ SimpleGraphic
-- U2b gate: runs the ORIGINAL Launch.lua + Modules/Main.lua + Modules/BuildList.lua (+ BuildListHelpers, ToastNotification,
-- Classes/*) of the PoB checkout, byte-copied into sg/src/, in the real SimpleGraphic, against a TEST build folder inside
-- the sandbox (sg/user/Path of Building/Builds, copies of builds/pc/3.29), and records screenshots + state for scripted
-- input scenarios (sg/scenes/ui/<name>.json; same step format as controls-runner.lua, events go through launch:OnKeyDown /
-- OnKeyUp / OnChar like the host's).
--   pwsh tools/sg-oracle/run.ps1 -Script ui-runner.lua [-ScriptArgs <scenario,...>]
-- Stubbed (not on the build-list path; the Rust port has no counterpart yet): GameVersions (tree list = latest only),
-- Modules/CalcFormat, Data, ModTools, ItemTools, CalcTools, PantheonTools, BuildSiteTools, Build (BUILD mode = U2c),
-- Data/ModCache, Classes/PassiveTree (only `assets.Background2`, loaded like PassiveTree:LoadImage does).
-- Harness overrides: GetUserPath -> sandbox, GetCursorPos/GetTime/IsKeyDown scripted, Copy/Paste/OpenURL in memory,
-- launch.CheckForUpdate = no-op (no network), `arg` emptied, `changelog.txt`/`help.txt` read from the checkout root,
-- main.controls.checkUpdate hidden and main:SetManifestBranch a no-op (the Rust port has no update machinery: UI-ARCH §10 п. 6).
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua") -- written by run.ps1 from tools/upstream.json
local POB = UPSTREAM.root .. "src/"
local POB_ROOT = UPSTREAM.root
local BUILDS = "C:/pob-work/pob-rust/builds/pc/3.29/"
local SRC = base .. "/src/"
local USER = base .. "/user"
local OUT = base .. "/out/ui/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local function readAll(p) local f = assert(io.open(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(io.open(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

-- 0. Probe of the file functions (sandbox only).
local probe = { }
local probeOk, probeErr = pcall(function()
	local P = base .. "/probe/"
	local function pack(...) local t = { n = select("#", ...) } for i = 1, t.n do t[i] = tostring((select(i, ...))) end return table.concat(t, "|", 1, t.n) end
	local function names(h)
		local t = { }
		while h do t[#t + 1] = h:GetFileName() if not h:NextFile() then break end end
		return table.concat(t, ",")
	end
	probe.mkNew = pack(MakeDir(P))
	probe.mkExisting = pack(MakeDir(P))
	probe.mkNested = pack(MakeDir(P .. "a/b/c"))
	probe.mkA = pack(MakeDir(P .. "a"))
	probe.mkNested2 = pack(MakeDir(P .. "a/b/c"))
	probe.mkB = pack(MakeDir(P .. "a/b"))
	for _, n in ipairs({ "x.xml", "B.xml", "c.XML", "a b.xml", "_u.xml", "10.xml", "9.xml", "y.txt" }) do writeAll(P .. "a/" .. n, "<Build/>") end
	MakeDir(P .. "a/Z")
	MakeDir(P .. "a/e")
	probe.files = names(NewFileSearch(P .. "a/*.xml"))
	probe.dirs = names(NewFileSearch(P .. "a/*", true))
	probe.all = names(NewFileSearch(P .. "a/*"))
	probe.none = pack(NewFileSearch(P .. "a/*.nothing"))
	probe.emptyDir = pack(NewFileSearch(P .. "a/Z/*")) .. "/" .. pack(NewFileSearch(P .. "a/Z/*", true))
	local h = NewFileSearch(P .. "a/x.xml")
	probe.mtimeType = type(h:GetFileModifiedTime())
	probe.mtimeMinusNow = h:GetFileModifiedTime() - os.time()
	probe.rmNonEmpty = pack(RemoveDir(P .. "a"))
	probe.rmEmpty = pack(RemoveDir(P .. "a/Z"))
	probe.rmMissing = pack(RemoveDir(P .. "nope"))
	probe.mkOverFile = pack(MakeDir(P .. "a/x.xml"))
	probe.renameMissing = pack(os.rename(P .. "nope.xml", P .. "nope2.xml"))
	probe.rmRecursive = pack(RemoveDir(P .. "a", true))
	probe.afterRecursive = pack(NewFileSearch(P .. "a", true))
	RemoveDir(P, true)
	probe.cloud = pack(GetCloudProvider(base))
	probe.userPath = pack(GetUserPath() ~= nil)
end)
if not probeOk then probe.error = tostring(probeErr) end

-- 1. Copy of the original sources (read-only use of the checkout).
local files = { "Launch", "Modules/Common", "Modules/Main", "Modules/BuildList", "Modules/BuildListHelpers", "Modules/ToastNotification", "Data/Global" }
for _, c in ipairs({ "Control", "ControlHost", "LabelControl", "ButtonControl", "CheckBoxControl", "EditControl", "UndoHandler",
	"ResizableEditControl", "DropDownControl", "SliderControl", "ScrollBarControl", "ListControl", "TextListControl", "Tooltip",
	"TooltipHost", "PopupDialog", "SearchHost", "SectionControl", "RectangleOutlineControl", "DraggerControl", "BuildListControl",
	"FolderListControl", "PathControl" }) do
	files[#files + 1] = "Classes/" .. c
end
for _, d in ipairs({ "src", "src/Modules", "src/Data", "src/Classes", "out", "out/ui", "user", "user/Path of Building",
	"user/Path of Building/Builds", "user/Path of Building/Builds/Merc", "user/Path of Building/Builds/Empty" }) do
	MakeDir(base .. "/" .. d)
end
for _, f in ipairs(files) do writeAll(SRC .. f .. ".lua", readAll(POB .. f .. ".lua")) end
local stubs = {
	["GameVersions"] = 'treeVersionList = { "3_29" }\nlatestTreeVersion = "3_29"\ntreeVersions = { ["3_29"] = { display = "3.29" } }\n',
	["Modules/CalcFormat"] = "",
	["Modules/Data"] = "LoadModule(\"Data/Global\")\ndata = { setJewelRadiiGlobally = function() end, uniques = { }, rares = { }, itemMods = { }, printMissingMinionSkills = function() end }\n",
	["Modules/ModTools"] = "modLib = { parseModCache = { } }\n",
	["Modules/PantheonTools"] = "",
	["Modules/BuildSiteTools"] = "buildSites = { }\n",
	["Modules/Build"] = "local b = { }\nfunction b:Init(...) self.args = { ... } end\nfunction b:GetArgs() return unpack(self.args) end\nreturn b\n",
	["Data/ModCache"] = "return { }\n",
	["Classes/PassiveTree"] = 'local C = newClass("PassiveTree")\nfunction C:PassiveTree(v)\n\tself.treeVersion = v\n\tself.assets = { Background2 = { } }\n\tlocal d = self.assets.Background2\n\td.handle = NewImageHandle()\n\td.handle:Load("' .. POB .. 'TreeData/Background2.png", "ASYNC")\n\td.width, d.height = d.handle:ImageSize()\n\treturn self\nend\n',
}
for name, text in pairs(stubs) do writeAll(SRC .. name .. ".lua", text) end
-- Test build folder: 6 builds at the root, 2 in "Merc", empty folder "Empty".
for _, b in ipairs({ "build-g", "build-i", "build-d", "build-o", "build-h", "build-j" }) do
	writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(BUILDS .. b .. ".xml"))
end
for _, b in ipairs({ "build-c", "build-a" }) do
	writeAll(USER .. "/Path of Building/Builds/Merc/" .. b .. ".xml", readAll(BUILDS .. b .. ".xml"))
end
writeAll(USER .. "/Path of Building/Settings.xml", readAll(base .. "/scenes/ui/Settings.xml"))

-- 2. Environment.
package.path = SRC .. "?.lua;" .. package.path
package.loaded["Modules.ItemTools"] = true
package.loaded["Modules.CalcTools"] = true
itemLib = { wiki = { triggered = false } }
function LoadModule(name, ...) return assert(loadfile(SRC .. name .. ".lua"))(...) end
function PLoadModule(name, ...)
	local r = { pcall(LoadModule, name, ...) }
	if r[1] then return nil, unpack(r, 2, table.maxn(r)) end
	return r[2]
end
function GetUserPath() return USER end
local realGetTime = GetTime
local cursorX, cursorY, nowMs, held = -100, -100, 0, { }
function GetCursorPos() return cursorX, cursorY end
function GetTime() return nowMs end
function IsKeyDown(k) return held[k] == true end
local clipboard, opened = nil, { }
function Copy(text) clipboard = text end
function Paste() return clipboard end
function OpenURL(url) opened[#opened + 1] = url end
local realOpen, realLines = io.open, io.lines
local redirect = { ["changelog.txt"] = POB_ROOT .. "changelog.txt", ["help.txt"] = POB_ROOT .. "help.txt" }
function io.open(name, ...) return realOpen(redirect[name] or name, ...) end
function io.lines(name, ...) return realLines(redirect[name] or name, ...) end

local runner = { }
local function fail(where, err)
	writeAll(OUT .. "startup-error.json", json.encode({ where = where, err = tostring(err), prompt = launch and launch.promptMsg or "nil", probe = probe }, { indent = true }))
	runner.finished = true
	Exit()
end
local okL, errL = pcall(dofile, SRC .. "Launch.lua")
if not okL then fail("dofile Launch", errL) end
local L = launch
L.CheckForUpdate = function() end
SetMainObject(runner)

-- 3. State dump.
local function nameOf(tbl, ctrl)
	if not ctrl then return "nil" end
	for k, v in pairs(tbl) do if v == ctrl then return tostring(k) end end
	return "other"
end
local function listing(root)
	local out = { }
	local function walk(sub)
		local h = NewFileSearch(root .. sub .. "*")
		while h do out[#out + 1] = sub .. h:GetFileName() if not h:NextFile() then break end end
		h = NewFileSearch(root .. sub .. "*", true)
		local dirs = { }
		while h do dirs[#dirs + 1] = h:GetFileName() if not h:NextFile() then break end end
		for _, d in ipairs(dirs) do out[#out + 1] = sub .. d .. "/" walk(sub .. d .. "/") end
	end
	walk("")
	table.sort(out)
	return table.concat(out, ",")
end
local function settings()
	local x = common.xml.LoadXMLFile(main.userPath .. "Settings.xml")
	local s = { }
	for _, node in ipairs(x[1]) do
		if type(node) == "table" then
			if node.elem == "Mode" then
				local args = { }
				for _, a in ipairs(node) do
					if type(a) == "table" then args[#args + 1] = (a.attrib.string and "s:" .. a.attrib.string) or (a.attrib.number and "n:" .. a.attrib.number) or ("b:" .. tostring(a.attrib.boolean)) end
				end
				s.mode = node.attrib.mode .. "(" .. table.concat(args, ",") .. ")"
			elseif node.elem == "Misc" then
				for k, v in pairs(node.attrib) do s[k] = v end
			end
		end
	end
	return s
end
local function dump(st)
	local d = { mode = main.mode or "nil", popups = #main.popups, prompt = launch.promptMsg or "nil", clipboard = clipboard or "nil",
		opened = table.concat(opened, ","), mainSel = nameOf(main.controls, main.selControl) }
	local p = main.popups[1]
	if p then
		d.popupTitle = p.title
		d.popupSel = nameOf(p.controls, p.selControl)
		d.popupEdit = p.controls.edit and p.controls.edit.buf or "nil"
	end
	local lm = main.modes.LIST
	if lm.initialised then
		local bl = lm.controls.buildList
		local rows = { }
		for i, b in ipairs(bl.list) do rows[i] = bl:GetRowValue(1, i, b) .. "|" .. bl:GetRowValue(2, i, b) end
		d.rows = rows
		d.selIndex = bl.selIndex or "nil"
		d.subPath = lm.subPath
		d.sort = main.buildSortMode
		d.search = lm.controls.searchText.buf
		d.listSel = nameOf(lm.controls, lm.selControl)
		local folders = { }
		for i, f in ipairs(bl.controls.path.folderList) do folders[i] = f.label end
		d.path = table.concat(folders, "/")
		d.copy = bl.copyBuild and bl.copyBuild.fullFileName:sub(#main.buildPath + 1) or "nil"
		d.cut = bl.cutBuild and bl.cutBuild.fullFileName:sub(#main.buildPath + 1) or "nil"
		d.drag = bl.selDragActive == true
	end
	d.files = listing(main.buildPath)
	if st.settings then d.settings = settings() end
	return d
end

-- 4. Scenario loop (frame = launch:OnFrame of the original; events go through launch's handlers first).
local names = json.decode(readAll(base .. "/scenes/ui/index.json"))
if scriptArgs then names = { } for n in scriptArgs:gmatch("[^,]+") do names[#names + 1] = n end end
local function listShots()
	local t, h = { }, NewFileSearch(shotDir .. "*.png")
	while h do t[h:GetFileName()] = true if not h:NextFile() then break end end
	return t
end

function runner:OnInit()
	local ok, err = pcall(self.Init, self)
	if not ok then fail("OnInit", err) end
end

function runner:Init()
	RenderInit("DPI_AWARE")
	ConExecute("set r_screenshotFormat png")
	SetDPIScaleOverridePercent(100)
	L:OnInit()
	main.controls.checkUpdate.shown = false
	-- Options/Save rewrites the install's manifest.xml (absent in the sandbox runtime → a Lua error); not ported.
	main.SetManifestBranch = function() end
	self.idx, self.frames, self.errors = 0, 0, { }
end

function runner:Frame()
	L:OnFrame()
end

function runner:OnFrame()
	if self.finished then return end
	local ok, err = pcall(self.Step, self)
	if not ok then fail("OnFrame", err) end
end

function runner:Step()
	self.frames = self.frames + 1
	if not self.sc then
		-- Settle: the first frames switch to LIST mode and load the ASYNC background.
		local ok, bg = pcall(function() return main.tree[latestTreeVersion].assets.Background2.handle end)
		if not ok or launch.promptMsg or self.frames > 600 + 1000 * self.idx then
			writeAll(OUT .. "startup-error.json", json.encode({ err = tostring(bg), prompt = launch.promptMsg or "nil", probe = probe }, { indent = true }))
			return self:Finish()
		end
		if self.frames < 5 or bg:ImageSize() == 0 then return self:Frame() end
		self.idx = self.idx + 1
		local name = names[self.idx]
		if not name then return self:Finish() end
		self.sc = { name = name, def = json.decode(readAll(base .. "/scenes/ui/" .. name .. ".json"), 1, json.null), step = 0, dumps = { }, shots = { } }
	end
	local sc = self.sc
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
				local ok, err = os.rename(shotDir .. f, OUT .. sc.waitShot .. ".png")
				if not ok then self.errors[#self.errors + 1] = "rename: " .. tostring(err) end
				sc.shots[#sc.shots + 1] = sc.waitShot
				sc.waitShot, sc.shotAt = nil, nil
				return
			end
		end
		if realGetTime() - sc.shotAt > 10000 then self.errors[#self.errors + 1] = "no screenshot for " .. sc.waitShot sc.waitShot, sc.shotAt = nil, nil end
		return
	end
	sc.step = sc.step + 1
	local st = sc.def.steps[sc.step]
	if not st then
		writeAll(OUT .. sc.name .. ".json", json.encode({ name = sc.name, probe = probe, perf = sc.perf, dumps = sc.dumps, shots = sc.shots, errors = self.errors },
			{ indent = true, keyorder = { "name", "probe", "perf", "step", "dumps", "shots", "errors" } }) .. "\n")
		self.sc = nil
		return
	end
	if st.perf then
		-- CPU of launch:OnFrame (Lua UI + SimpleGraphic command recording) over N frames in one host frame.
		local t0 = os.clock()
		for _ = 1, st.perf do L:OnFrame() end
		sc.perf = { frames = st.perf, msPerFrame = (os.clock() - t0) * 1000 / st.perf }
		return
	end
	if st.cursor then cursorX, cursorY = st.cursor[1], st.cursor[2] end
	if st.time then nowMs = st.time end
	for _, k in ipairs(st.hold or { }) do held[k] = true end
	for _, k in ipairs(st.release or { }) do held[k] = nil end
	for _, e in ipairs(st.events or { }) do
		if e[1] == "KeyDown" then L:OnKeyDown(e[2], e[3] == true)
		elseif e[1] == "KeyUp" then L:OnKeyUp(e[2])
		else L:OnChar(e[2]) end
	end
	self:Frame()
	if st.dump then
		local ok, d = pcall(dump, st)
		sc.dumps[#sc.dumps + 1] = { step = sc.step, state = ok and d or ("dump error: " .. tostring(d)) }
	end
	if st.shot then sc.waitShot, sc.seen = st.shot, listShots() end
end

function runner:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	Exit()
end

function runner:OnExit()
	-- launch:OnExit would save Settings.xml (sandbox) — harmless, but the gate reads Settings only at explicit steps.
end
