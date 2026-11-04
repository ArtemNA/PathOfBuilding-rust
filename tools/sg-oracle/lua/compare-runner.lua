#@ SimpleGraphic
-- U8b gate (the Compare tab): runs the WHOLE original PoB (Launch.lua, Modules/Main, Modules/Build with every tab, Data, TreeData) of the checkout in the real SimpleGraphic -- read in place,
-- any relative write is a Lua error -- against a TEST build folder in the sandbox (sg/user/Path of Building/Builds), and records state + screenshots for the scripted scenes
-- sg/scenes/compare/<name>.json.
--   pwsh tools/sg-oracle/run.ps1 -Script compare-runner.lua [-ScriptArgs <scene,...>] -Timeout 600
-- Harness overrides (as items-runner.lua): GetUserPath -> sandbox, GetCursorPos/GetTime/IsKeyDown scripted, Copy/Paste/OpenURL in memory, launch.CheckForUpdate = no-op,
-- LaunchSubScript = no-op (no network), `arg` emptied, main.controls.checkUpdate hidden, main:SetManifestBranch no-op.
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua") -- written by run.ps1 from tools/upstream.json
local POB = UPSTREAM.root .. "src/"
local POB_ROOT = UPSTREAM.root
local BUILDS = "../../../../builds/pc/3.29/" -- cwd = runtime/sg
local USER = base .. "/user"
local OUT = base .. "/out/compare/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local realOpen, realLines = io.open, io.lines
local function readAll(p) local f = assert(realOpen(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(realOpen(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

-- 1. Test build folder (the same list as crates/pob-ui/tests/compare.rs).
for _, d in ipairs({ "out", "out/compare", "user", "user/Path of Building", "user/Path of Building/Builds" }) do
	MakeDir(base .. "/" .. d)
end
local function writeBuilds()
	local LOCAL_BUILDS = dofile(base .. "/local-builds.lua") -- personal builds, not in git
	for _, b in ipairs(LOCAL_BUILDS.compare or { }) do
		local src = BUILDS .. b .. ".xml"
		local probe = realOpen(src, "rb")
		if probe then probe:close() else src = "../../../../builds/pc/" .. b .. ".xml" end
		writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(src))
	end
	for _, b in ipairs({ "Generals Perforate Zerker", "Dual Savior", "Dual Wield Cospris CoC", "Mirage Archer Toxic Rain", "OccVortex" }) do
		writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(POB_ROOT .. "spec/TestBuilds/3.13/" .. b .. ".xml"))
	end
	-- Sets, minions, mines and stages on a public golden (the same edit as crates/pob-ui/tests/compare.rs).
	local gem = '<Gem level="20" gemId="Metadata/Items/Gems/%s" skillId="%s" qualityId="Default" quality="0" enabled="true" nameSpec="%s"/>'
	local group = function(g) return '<Skill mainActiveSkillCalcs="1" enabled="true" mainActiveSkill="1">' .. g .. '</Skill>' end
	local x = readAll(POB_ROOT .. "spec/TestBuilds/3.13/OccVortex.xml")
	x = x:gsub("(<Build [^>]*>)", function(m) return m .. '<Spectre id="Metadata/Monsters/Axis/AxisCaster"/>' end, 1)
	x = x:gsub("(<Skills [^>]*>)", function(m)
		return m .. group(gem:format("SkillGemRaiseSpectre", "RaiseSpectre", "Raise Spectre"))
			.. group(gem:format("SkillGemFireball", "Fireball", "Fireball") .. gem:format("SkillGemSupportBlastchainMine", "SupportBlastchainMine", "Blastchain Mine"))
			.. group(gem:format("SkillGemIncinerate", "Incinerate", "Incinerate"))
	end, 1)
	writeAll(USER .. "/Path of Building/Builds/Spectre Test.xml", x)
	-- A second item set (empty) of a public golden.
	local y = readAll(POB_ROOT .. "spec/TestBuilds/3.13/Dual Savior.xml")
	y = y:gsub("</Items>", function() return '<ItemSet id="2" title="Empty set"/></Items>' end, 1)
	writeAll(USER .. "/Path of Building/Builds/Sets Test.xml", y)
	-- The folder popup: a sub-folder with a build, a broken file, and enough files to scroll.
	local dir = USER .. "/Path of Building/Builds/Folder Test/"
	MakeDir(dir) MakeDir(dir .. "Many")
	writeAll(dir .. "Inner Build.xml", readAll(POB_ROOT .. "spec/TestBuilds/3.13/Generals Perforate Zerker.xml"))
	writeAll(dir .. "Not A Build.xml", "not a build")
	for i = 1, 30 do writeAll(dir .. string.format("Many/Row %02d.xml", i), "<PathOfBuilding/>") end
end
-- The real window (inject-compare-real.ps1): only two builds, the same as compare-rust-input.ps1.
local function writeRealBuilds()
	for _, b in ipairs({ "Dual Savior", "OccVortex" }) do
		writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(POB_ROOT .. "spec/TestBuilds/3.13/" .. b .. ".xml"))
	end
end
local REAL = scriptArgs == "real-input"
if REAL then writeRealBuilds() else writeBuilds() end
writeAll(USER .. "/Path of Building/Settings.xml", readAll(base .. "/scenes/tree/Settings.xml"))

-- 2. Environment: the checkout's src, read in place.
local SRC_DIRS = { "Data/", "TreeData/", "Assets/", "Classes/", "Modules/", "Export/" }
local function srcRelative(name)
	if type(name) ~= "string" or name:match("^%a:") or name:match("^[/\\]") then return nil end
	for _, d in ipairs(SRC_DIRS) do
		if name:sub(1, #d) == d then return POB .. name end
	end
	return nil
end
package.path = POB .. "?.lua;" .. package.path
function LoadModule(name, ...)
	local file = name:match("%.lua$") and name or name .. ".lua"
	return assert(loadfile(POB .. file))(...)
end
function PLoadModule(name, ...)
	local r = { pcall(LoadModule, name, ...) }
	if r[1] then return nil, unpack(r, 2, table.maxn(r)) end
	return r[2]
end
function GetUserPath() return USER end
function LaunchSubScript() return nil end
local realGetTime, realIsKeyDown = GetTime, IsKeyDown
local cursorX, cursorY, nowMs, held = -100, -100, 0, { }
function GetCursorPos() return cursorX, cursorY end
function GetTime() return nowMs end
function IsKeyDown(k) return held[k] == true end
local clipboard = nil
function Copy(text) clipboard = text end
function Paste() return clipboard end
function OpenURL() end
local redirect = { ["changelog.txt"] = POB_ROOT .. "changelog.txt", ["help.txt"] = POB_ROOT .. "help.txt" }
local function inCheckout(name)
	return type(name) == "string" and name:gsub("\\", "/"):lower():sub(1, #POB_ROOT) == POB_ROOT:lower()
end
function io.open(name, mode, ...)
	local src = srcRelative(name)
	local writing = mode and mode:match("[wa+]")
	if (src or inCheckout(name)) and writing then
		return nil, "write refused by compare-runner: " .. name
	end
	if src then return realOpen(src, mode, ...) end
	return realOpen(redirect[name] or name, mode, ...)
end
function GetScriptPath() return POB:sub(1, -2) end
function io.lines(name, ...) return realLines(srcRelative(name) or redirect[name] or name, ...) end
do
	local mt = getmetatable(NewImageHandle())
	local idx = mt and mt.__index
	if type(idx) == "table" and idx.Load then
		local load = idx.Load
		idx.Load = function(h, fileName, ...) return load(h, srcRelative(fileName) or fileName, ...) end
	else
		error("compare-runner: image handle metatable has no Load to redirect")
	end
end

-- Every DrawString of compareTab:Draw of a dump step, as text (crates/pob-ui/src/sg.rs `text_log`).
local textLog
local realDrawString = DrawString
function DrawString(l, t, a, h, f, s)
	if textLog then textLog[#textLog + 1] = string.format("%.14g|%.14g|%s|%.14g|%s|%s", l, t, tostring(a), h, tostring(f), tostring(s)) end
	return realDrawString(l, t, a, h, f, s)
end

local runner = { }
local function fail(where, err)
	writeAll(OUT .. "startup-error.json", json.encode({ where = where, err = tostring(err), prompt = launch and launch.promptMsg or "nil", scene = runner.sc and runner.sc.name or "nil", step = runner.sc and runner.sc.step or 0 }, { indent = true }))
	if runner.sc then writeAll(OUT .. runner.sc.name .. ".json", json.encode({ name = runner.sc.name, perf = { }, dumps = runner.sc.dumps, shots = runner.sc.shots, errors = { "failed at step " .. runner.sc.step } }) .. "\n") end
	runner.finished = true
	Exit()
end
local okL, errL = pcall(dofile, POB .. "Launch.lua")
if not okL then fail("dofile Launch", errL) end
local L = launch
L.CheckForUpdate = function() end
SetMainObject(runner)

-- 3. State.
local function labelOf(v)
	if type(v) == "table" then return v.label or "nil" end
	return tostring(v)
end
local function wrapCompareDraw(b)
	local ct = b.compareTab
	if ct and not rawget(ct, "Draw") then
		local d = ct.Draw
		ct.Draw = function(self, ...)
			local want = rawget(self, "__capture")
			if want then textLog = { } end
			d(self, ...)
			if want then rawset(self, "__log", textLog) textLog = nil end
		end
	end
end
local function compareDump(b)
	local t = b.compareTab
	if not rawget(t, "Draw") then wrapCompareDraw(b) end
	local c = t.controls
	local function drop(d)
		local list = { }
		for i, v in ipairs(d.list) do list[i] = labelOf(v) end
		return { shown = d:IsShown() and true or false, enabled = d:IsEnabled() and true or false, sel = d.selIndex or "nil", list = list }
	end
	local function edit(e) return e:IsShown() and e.buf or "hidden" end
	local entries = { }
	for i, e in ipairs(t.compareEntries) do
		local o = e.calcsTab.mainOutput
		-- %.17g: dkjson writes numbers with 14 digits
		local function num(k) return o[k] and string.format("%.17g", o[k]) or json.null end
		entries[i] = { label = e.label, level = e.characterLevel, mainSocketGroup = e.mainSocketGroup, Life = num("Life"), TotalDPS = num("TotalDPS"), FullDPS = num("FullDPS"), TotalEHP = num("TotalEHP") }
	end
	local log = rawget(t, "__log") or { }
	table.sort(log)
	local powerRows = { }
	for i, r in ipairs(c.comparePowerReportList.list) do
		powerRows[i] = string.format("%s|%s|%s|%s|%s", r.category, r.name, tostring(r.combinedImpactStr), r.pathDist and tostring(r.pathDist) or "", r.perPointStr or "")
	end
	return { mode = t.compareViewMode, active = t.activeCompareIndex, entries = entries, scrollY = t.scrollY, text = log, powerRows = powerRows,
		buildSelect = drop(c.compareBuildSelect), specSelect = drop(c.compareSpecSelect), skillSetSelect = drop(c.compareSkillSetSelect), itemSetSelect = drop(c.compareItemSetSelect), configSetSelect = drop(c.compareConfigSetSelect),
		cmpSocketGroup = drop(c.cmpSocketGroup), cmpMainSkill = drop(c.cmpMainSkill), cmpSkillPart = drop(c.cmpSkillPart), cmpMinion = drop(c.cmpMinion), cmpMinionSkill = drop(c.cmpMinionSkill), cmpStageCount = edit(c.cmpStageCount), cmpMineCount = edit(c.cmpMineCount) }
end
-- The popup "Import from Folder" (the same fields as `FolderHost::debug_dump`).
local function folderDump(p)
	local c = p.controls
	local l = c.buildList
	local rows, labels = { }, { }
	for i, v in ipairs(l.list) do rows[i] = l:GetRowValue(1, i, v) .. "|" .. l:GetRowValue(2, i, v) end
	for i, f in ipairs(l.controls.path.folderList) do labels[i] = f.label end
	return { rows = rows, sel = l.selIndex or 0, subPath = l.listMode.subPath, path = labels, search = c.searchText.buf, sort = c.sort.selIndex, sortMode = c.sort:GetSelValue().sortMode,
		openEnabled = c.open:IsEnabled() and true or false, scrollV = l.controls.scrollBarV.offset }
end
local function dump(st)
	local d = { mode = main.mode or "nil", popups = #main.popups, prompt = launch.promptMsg or "nil", cursorAt = { cursorX, cursorY } }
	if main.popups[1] then d.popupTitle = main.popups[1].title end
	for _, p in ipairs(main.popups) do
		if p.controls.buildList then d.folder = folderDump(p) end
	end
	local b = main.modes.BUILD
	local lines = { }
	for i, l in ipairs(b.controls.statBox.list) do lines[i] = tostring(l[1] or "nil") .. "|" .. tostring(l[2] or "nil") end
	d.build = { viewMode = b.viewMode, statBox = lines }
	d.compare = compareDump(b)
	if st.setsState then
		local c = b.compareTab.controls
		local function drop(s) local list = { } for i, v in ipairs(s.list) do list[i] = labelOf(v) end return { sel = s.selIndex or "nil", list = list } end
		d.sets = { primaryItem = drop(c.primaryItemSetSelect), compareItem = drop(c.compareItemSetSelect2), primaryTree = drop(c.primaryTreeSetSelect), compareTree = drop(c.compareTreeSetSelect) }
	end
	if st.radius then d.radiusTable = data.jewelRadius == data.jewelRadii["3_15"] and "3_15" or "3_16" end
	if st.saved then d.savedHasCompare = readAll(main.buildPath .. st.saved .. ".xml"):find("<Compare", 1, true) ~= nil end
	if st.nodeTip then
		-- the primary build's node tooltip (the comparison of `calcFunc` over the cache the entries left)
		local v = b.treeTab.viewer
		d.tooltip = { }
		if v.tooltip then for i, l in ipairs(v.tooltip.lines) do d.tooltip[i] = l.text or "<sep>" end end
	end
	return d
end
local function compareCursor(st)
	local c = main.modes.BUILD and main.modes.BUILD.compareTab and main.modes.BUILD.compareTab.controls
	if st.cursorCompare then
		local a = st.cursorCompare
		local x, y = c[a[1]]:GetPos()
		local w, h = c[a[1]]:GetSize()
		cursorX, cursorY = math.floor(x + w / 2) + (a[2] or 0), math.floor(y + h / 2) + (a[3] or 0)
	elseif st.cursorCompareRow then
		local a = st.cursorCompareRow
		local x, y = c[a[1]]:GetPos()
		local w, h = c[a[1]]:GetSize()
		-- a negative row counts from the end of the list
		local row = a[2] < 0 and #c[a[1]].list + a[2] + 1 or a[2]
		cursorX, cursorY = x + 10, y + h + 2 + (row - 1) * (h - 4) + math.floor((h - 4) / 2)
	elseif st.compareItemButton then
		-- the buttons of a compact row of the Items view (TradeHelpers.lua:440-496, CompareTab.lua:3792-3793)
		local a = st.compareItemButton
		local t = main.modes.BUILD.compareTab
		local start = 312 + t.itemsColWidth + 10 - c.itemsHScrollBar.offset + 310 + 6
		local dx = a[2] == "copy" and 94 or a[2] == "buy" and 30 or 158
		cursorX, cursorY = start + dx, 236 + 4 - t.scrollY + 20 * a[1] + 10
	elseif st.cursorBuild then
		local a = st.cursorBuild
		local ctl = main.modes.BUILD.controls[a[1]]
		local x, y = ctl:GetPos()
		local w, h = ctl:GetSize()
		cursorX, cursorY = math.floor(x + w / 2) + (a[2] or 0), math.floor(y + h / 2) + (a[3] or 0)
	elseif st.cursorPopup then
		local a = st.cursorPopup
		local ctl = main.popups[1].controls[a[1]]
		local x, y = ctl:GetPos()
		local w, h = ctl:GetSize()
		cursorX, cursorY = math.floor(x + w / 2) + (a[2] or 0), math.floor(y + h / 2) + (a[3] or 0)
	elseif st.cursorPopupDrop then
		-- { control of the popup, 1-based row of the open list }
		local a = st.cursorPopupDrop
		local x, y = main.popups[1].controls[a[1]]:GetPos()
		local w, h = main.popups[1].controls[a[1]]:GetSize()
		cursorX, cursorY = math.floor(x + w / 2), y + h + 2 + (a[2] - 1) * (h - 4) + 4
	elseif st.cursorFolderRow then
		local x, y = main.popups[1].controls.buildList:GetPos()
		cursorX, cursorY = x + 20, y + 2 + (st.cursorFolderRow - 1) * 20 + 10
	elseif st.cursorFolderPath then
		local ctl = main.popups[1].controls.buildList.controls.path.folderList[st.cursorFolderPath].button
		local x, y = ctl:GetPos()
		local w, h = ctl:GetSize()
		cursorX, cursorY = math.floor(x + w / 2), math.floor(y + h / 2)
	elseif st.cursorCompareNode then
		-- the overlay of the Compare tab: the viewer's viewport is relative, GetCursorPos is patched by that offset (CompareTab.lua:3425-3429)
		local b = main.modes.BUILD
		local v = b.treeTab.viewer
		local n = assert(b.spec.nodes[st.cursorCompareNode], "no such node")
		local W, H = 968, 768 - 126 - 74 - 30
		local scale = math.min(W, H) / b.spec.tree.size * v.zoom
		cursorX, cursorY = math.floor(n.x * scale + v.zoomX + W / 2) + 312, math.floor(n.y * scale + v.zoomY + H / 2) + 32 + 126 + 74
	elseif st.cursorNodeAuto then
		-- the smallest id of an unallocated notable next to the allocated tree (a node with a path of 1)
		local b = main.modes.BUILD
		local v = b.treeTab.viewer
		-- the nearest to the centre of the view: the smallest id may be out of the window at the default zoom (no tooltip, the probe of U8b)
		local best, bestD
		for id, n in pairs(b.spec.nodes) do
			if n.type == "Notable" and not n.alloc and n.x then
				local d = (n.x + v.zoomX * b.spec.tree.size / (v.zoom * 712)) ^ 2 + (n.y + v.zoomY * b.spec.tree.size / (v.zoom * 712)) ^ 2
				if not best or d < bestD or (d == bestD and id < best) then best, bestD = id, d end
			end
		end
		assert(best, "no notable")
		local n = b.spec.nodes[best]
		-- TreeTab.lua: the viewer's height depends on the convert bar of an old tree, recorded by the wrapper of its Draw
		if not rawget(v, "__vp") then
			local draw = v.Draw
			v.Draw = function(self, build, vp, ev) rawset(self, "__vp", { x = vp.x, y = vp.y, width = vp.width, height = vp.height }) return draw(self, build, vp, ev) end
		end
		local p = rawget(v, "__vp") or { x = 312, y = 32, width = 968, height = 712 }
		local W, H = p.width, p.height
		local scale = math.min(W, H) / b.spec.tree.size * v.zoom
		cursorX, cursorY = math.floor(n.x * scale + v.zoomX + p.x + W / 2), math.floor(n.y * scale + v.zoomY + p.y + H / 2)
	elseif st.cursorNode then
		local b = main.modes.BUILD
		local v, n = b.treeTab.viewer, b.spec.nodes[st.cursorNode]
		local W, H = 968, 712
		local scale = math.min(W, H) / b.spec.tree.size * v.zoom
		cursorX, cursorY = math.floor(n.x * scale + v.zoomX + 312 + W / 2), math.floor(n.y * scale + v.zoomY + 32 + H / 2)
	end
end

-- 4. Scenario loop.
local names = json.decode(readAll(base .. "/scenes/compare/index.json"))
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
	local t0 = realGetTime()
	L:OnInit()
	self.initMs = realGetTime() - t0
	main.controls.checkUpdate.shown = false
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

-- The events of the real window (scene real-input, inject-compare-real.ps1).
local realEvents = { }
function runner:OnKeyDown(key, double)
	if self.real then realEvents[#realEvents + 1] = "down " .. key .. (double and " double" or "") L:OnKeyDown(key, double) end
end
function runner:OnKeyUp(key)
	if self.real then realEvents[#realEvents + 1] = "up " .. key L:OnKeyUp(key) end
end
function runner:OnChar(key)
	if self.real then realEvents[#realEvents + 1] = "char " .. key L:OnChar(key) end
end
function runner:OnExit() end
-- The state of the real window (the same fields as pob_ui::tabs::compare::debug_input_state).
function runner:RealState()
	local b = main.modes.BUILD
	local t = b.compareTab
	local c = t.controls
	local pos = function(ctl) local x, y = ctl:GetPos() local w, h = ctl:GetSize() return { x = math.floor(x + w / 2), y = math.floor(y + h / 2) } end
	local shown = function(ctl) if ctl and ctl:IsShown() then return pos(ctl) end return { x = 0, y = 0 } end
	local p = main.popups[1]
	local pc = p and p.controls or { }
	local labels = { }
	for i, e in ipairs(t.compareEntries) do labels[i] = e.label end
	local list = pc.buildList
	local row2 = { x = 0, y = 0 }
	if list then local x, y = list:GetPos() row2 = { x = x + 20, y = y + 2 + 20 + 10 } end
	return { ready = true, events = realEvents, compareReal = { viewMode = b.viewMode, modeCompare = pos(b.controls.modeCompare), importBtn = pos(c.importBtn), removeBtn = shown(c.removeBtn), popups = #main.popups, popupTitle = p and p.title or "",
		input = shown(pc.input), inputBuf = pc.input and pc.input.buf or "", state = pc.state and pc.state:GetProperty("label") or "", go = shown(pc.go), importFolder = shown(pc.importFolder),
		folderRow2 = row2, folderN = list and #list.list or 0, folderSel = list and list.selIndex or 0, folderOpen = shown(pc.open),
		entries = labels, active = t.activeCompareIndex, mode = t.compareViewMode, scrollY = t.scrollY, subItems = shown(c.subTabItems), subConfig = shown(c.subTabConfig), subTree = shown(c.subTabTree),
		configSearch = shown(c.configSearchEdit), configBuf = c.configSearchEdit.buf } }
end

function runner:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	Exit()
end

function runner:Step()
	self.frames = self.frames + 1
	if not self.sc then
		local ok, bg = pcall(function() return main.tree[latestTreeVersion].assets.Background2.handle end)
		if not ok or launch.promptMsg or self.frames > 900 + 2000 * self.idx then
			writeAll(OUT .. "startup-error.json", json.encode({ err = tostring(bg), prompt = launch.promptMsg or "nil" }, { indent = true }))
			return self:Finish()
		end
		if self.frames < 5 or bg:ImageSize() == 0 or main.mode ~= "LIST" or main.uniqueDB.loading or main.rareDB.loading then return self:Frame() end
		self.idx = self.idx + 1
		local name = names[self.idx]
		if not name then return self:Finish() end
		clipboard = nil if not REAL then writeBuilds() end wipeTable(main.popups)
		self.sc = { name = name, def = json.decode(readAll(base .. "/scenes/compare/" .. name .. ".json"), 1, json.null), step = 0, dumps = { }, shots = { }, perf = { } }
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
		writeAll(OUT .. sc.name .. ".json", json.encode({ name = sc.name, initMs = self.initMs, perf = sc.perf, dumps = sc.dumps, shots = sc.shots, errors = self.errors },
			{ indent = true, keyorder = { "name", "initMs", "perf", "step", "dumps", "shots", "errors" } }) .. "\n")
		self.sc = nil
		if main.mode == "BUILD" then
			local b = main.modes.BUILD
			b.modFlag, b.unsaved, b.skillsTab.modFlag, b.calcsTab.modFlag, b.itemsTab.modFlag = false, false, false, false, false
			main:SetMode("LIST")
		end
		return
	end
	if st.realInput then
		if not self.real then
			self.real = true
			GetTime, IsKeyDown = realGetTime, realIsKeyDown
			writeAll(base .. "/ready", "1")
		end
		local cf = realOpen(base .. "/cursor.txt", "r")
		if cf then
			local x, y = cf:read("*n", "*n")
			cf:close()
			if x and y then cursorX, cursorY = x, y end
		end
		self:Frame()
		writeAll(base .. "/state.tmp", json.encode(self:RealState()))
		os.remove(base .. "/state.json")
		os.rename(base .. "/state.tmp", base .. "/state.json")
		local stop = realOpen(base .. "/stop", "r")
		if stop then stop:close() return self:Finish() end
		sc.step = sc.step - 1
		return
	end
	if st.waitAssets then
		if GetAsyncCount() > 0 then sc.step = sc.step - 1 end
		self:Frame()
		return
	end
	if st.closeBuild then
		local b = main.modes.BUILD
		b.modFlag, b.skillsTab.modFlag, b.calcsTab.modFlag, b.unsaved = false, false, false, false
		main:SetMode("LIST")
		self:Frame()
		return
	end
	if st.open then
		main:SetMode("BUILD", main.buildPath .. st.open .. ".xml", st.open)
		local t0 = os.clock()
		self:Frame()
		assert(main.mode == "BUILD" and main.modes.BUILD.dbFileName, "open: no build " .. st.open) -- MINEFIELD R55
		sc.perf[#sc.perf + 1] = { what = "open", build = st.open, ms = (os.clock() - t0) * 1000 }
		return
	end
	if st.compareImport then
		-- what the popups call: compareTab:ImportBuild(xmlText, label)
		local b = main.modes.BUILD
		wrapCompareDraw(b)
		local t0 = os.clock()
		local ok = b.compareTab:ImportBuild(readAll(main.buildPath .. st.compareImport[1] .. ".xml"), st.compareImport[2])
		sc.perf[#sc.perf + 1] = { what = "import " .. st.compareImport[1], ms = (os.clock() - t0) * 1000 }
		assert(ok, "import failed")
		self:Frame()
		return
	end
	if st.compareSel then
		-- a list that drops up over the rows of the scroll position: the selection itself
		main.modes.BUILD.compareTab.controls[st.compareSel[1]]:SetSel(st.compareSel[2])
		self:Frame()
		return
	end
	if st.cacheProbe then
		-- R47 and the entries: `getMiscCalculator` of the primary build around one config value, without and with `wipeGlobalCache()`
		local b = main.modes.BUILD
		local var, val, statName = st.cacheProbe[1], st.cacheProbe[2], st.cacheProbe[3]
		local stat
		for _, e in ipairs(data.powerStatList) do if e.stat == statName then stat = e end end
		local calcs = b.compareTab.calcs
		local function value()
			local _, base = calcs.getMiscCalculator(b)
			return data.powerStatList.GetFromOutput(base, stat)
		end
		local input = b.configTab.input
		local saved = input[var]
		local r = { var = var, stat = statName, base = value() }
		input[var] = val
		b.configTab:BuildModList()
		r.editedNoWipe = value()
		wipeGlobalCache()
		r.editedWiped = value()
		input[var] = saved
		b.configTab:BuildModList()
		r.restoredNoWipe = value()
		wipeGlobalCache()
		r.restoredWiped = value()
		b.buildFlag = true
		for k, v in pairs(r) do if type(v) == "number" then r[k] = string.format("%.17g", v) end end
		sc.perf[#sc.perf + 1] = { what = "cacheProbe", r = r }
		self:Frame()
		return
	end
	if st.quadLog then
		-- diagnostics (MINEFIELD R60): every DrawImage / DrawImageQuad of one frame that touches the box, with its Lua line
		local box = st.quadLog
		local log = { }
		local realQuad, realImage = DrawImageQuad, DrawImage
		local function touches(...)
			local a = { ... }
			local minX, maxX, minY, maxY = math.huge, -math.huge, math.huge, -math.huge
			for i = 1, #a, 2 do
				minX, maxX = math.min(minX, a[i]), math.max(maxX, a[i])
				minY, maxY = math.min(minY, a[i + 1]), math.max(maxY, a[i + 1])
			end
			return maxX >= box[1] and minX <= box[3] and maxY >= box[2] and minY <= box[4]
		end
		function DrawImageQuad(h, x1, y1, x2, y2, x3, y3, x4, y4, s1, t1, s2, t2, s3, t3, s4, t4, ...)
			if touches(x1, y1, x2, y2, x3, y3, x4, y4) then
				local w, hh = 0, 0
				if h then w, hh = h:ImageSize() end
				log[#log + 1] = string.format("%d|%dx%d|%.3f,%.3f %.3f,%.3f %.3f,%.3f %.3f,%.3f|%.5f,%.5f %.5f,%.5f %.5f,%.5f %.5f,%.5f", debug.getinfo(2, "l").currentline, w, hh, x1, y1, x2, y2, x3, y3, x4, y4, s1 or 0, t1 or 0, s2 or 0, t2 or 0, s3 or 0, t3 or 0, s4 or 0, t4 or 0)
			end
			if s1 == nil then return realQuad(h, x1, y1, x2, y2, x3, y3, x4, y4) end
			return realQuad(h, x1, y1, x2, y2, x3, y3, x4, y4, s1, t1, s2, t2, s3, t3, s4, t4, ...)
		end
		function DrawImage(h, x, y, w, hh, ...)
			if touches(x, y, x + w, y + hh) then
				local iw, ih = 0, 0
				if h then iw, ih = h:ImageSize() end
				log[#log + 1] = string.format("%d|%dx%d|%.3f,%.3f %.3f,%.3f", debug.getinfo(2, "l").currentline, iw, ih, x, y, w, hh)
			end
			return realImage(h, x, y, w, hh, ...)
		end
		self:Frame()
		DrawImageQuad, DrawImage = realQuad, realImage
		writeAll(OUT .. st.quadOut .. ".json", json.encode(log, { indent = true }) .. "\n")
		return
	end
	if st.treeCenter then
		-- the view as a precondition of a scene: the zoom position that the XML saved leaves nodes out of the window
		local v = main.modes.BUILD.treeTab.viewer
		v.zoomX, v.zoomY = 0, 0
		self:Frame()
		return
	end
	if st.treeOrder then
		-- R27, R31: the overlap order of this process, of both builds
		local function orders(b)
			local ids, conns, subs, sockets = { }, { }, { }, { }
			for id in pairs(b.spec.nodes) do ids[#ids + 1] = id end
			for _, c in ipairs(b.spec.tree.connectors) do conns[#conns + 1] = c.nodeId1 .. "-" .. c.nodeId2 .. ":" .. c.type end
			for id in pairs(b.spec.subGraphs) do subs[#subs + 1] = id end
			for id in pairs(b.spec.tree.sockets) do sockets[#sockets + 1] = id end
			return { nodes = ids, connectors = conns, subGraphs = subs, sockets = sockets }
		end
		local b = main.modes.BUILD
		local entry = b.compareTab:GetActiveCompare()
		writeAll(OUT .. st.treeOrder .. ".json", json.encode({ primary = orders(b), entry = entry and orders(entry) or nil }) .. "\n")
		return
	end
	if st.compareScrollX then
		main.modes.BUILD.compareTab.controls.itemsHScrollBar:SetOffset(st.compareScrollX)
		self:Frame()
		return
	end
	if st.writeFile then
		writeAll(main.buildPath .. st.writeFile[1], st.writeFile[2])
		return
	end
	if st.save then
		main.modes.BUILD:SaveDBFile()
		self:Frame()
		return
	end
	if st.perf then
		local t0 = os.clock()
		for _ = 1, st.perf do L:OnFrame() end
		sc.perf[#sc.perf + 1] = { what = "frame", frames = st.perf, ms = (os.clock() - t0) * 1000 / st.perf }
		return
	end
	if st.cursor then cursorX, cursorY = st.cursor[1], st.cursor[2] end
	compareCursor(st)
	if st.clipboard then clipboard = st.clipboard end
	if st.time then nowMs = st.time end
	for _, k in ipairs(st.hold or { }) do held[k] = true end
	for _, k in ipairs(st.release or { }) do held[k] = nil end
	if st.dump and main.mode == "BUILD" then wrapCompareDraw(main.modes.BUILD) rawset(main.modes.BUILD.compareTab, "__capture", true) rawset(main.modes.BUILD.compareTab, "__log", nil) end
	local perfT0 = os.clock()
	for _, e in ipairs(st.events or { }) do
		if e[1] == "KeyDown" then L:OnKeyDown(e[2], e[3] == true)
		elseif e[1] == "KeyUp" then L:OnKeyUp(e[2])
		else L:OnChar(e[2]) end
	end
	self:Frame()
	if st.perfClick then sc.perf[#sc.perf + 1] = { what = st.perfClick, ms = (os.clock() - perfT0) * 1000 } end
	if st.dump then
		rawset(main.modes.BUILD.compareTab, "__capture", nil)
		local ok, d = xpcall(function() return dump(st) end, debug.traceback)
		sc.dumps[#sc.dumps + 1] = { step = sc.step, state = ok and d or ("dump error: " .. tostring(d)) }
	end
	if st.shot then sc.waitShot, sc.seen = st.shot, listShots() end
end
