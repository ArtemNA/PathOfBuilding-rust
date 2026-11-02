#@ SimpleGraphic
-- U3a gate (tree view; copy of build-runner.lua + the steps waitAssets / treeDump / treeOrder / search): runs the WHOLE original PoB (Launch.lua, Modules/Main, Modules/Build with every tab, Data, TreeData) of the
-- checkout in the real SimpleGraphic -- read in place (LoadModule / require / io.open / image loads of the src-relative
-- folders are redirected to the checkout, read-only; any relative write is a Lua error) -- against a TEST build folder in
-- the sandbox (sg/user/Path of Building/Builds: copies of builds/pc/3.29 + one golden), and records state + screenshots of
-- the BUILD screen for scripted scenarios (sg/scenes/tree/<name>.json; ui-runner.lua's step format plus `open`).
--   pwsh tools/sg-oracle/run.ps1 -Script tree-runner.lua [-ScriptArgs <scenario,...>] -Timeout 600
-- Harness overrides (as ui-runner.lua): GetUserPath -> sandbox, GetCursorPos/GetTime/IsKeyDown scripted, Copy/Paste/OpenURL
-- in memory, launch.CheckForUpdate = no-op, `arg` emptied, main.controls.checkUpdate hidden, main:SetManifestBranch no-op.
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua") -- written by run.ps1 from tools/upstream.json
local POB = UPSTREAM.root .. "src/"
local POB_ROOT = UPSTREAM.root
local BUILDS = "../../../builds/pc/3.29/"
local USER = base .. "/user"
local OUT = base .. "/out/tree/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local realOpen, realLines = io.open, io.lines
local function readAll(p) local f = assert(realOpen(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(realOpen(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

-- 1. Test build folder (the same list as crates/pob-ui/tests/tree.rs).
for _, d in ipairs({ "out", "out/tree", "user", "user/Path of Building", "user/Path of Building/Builds" }) do
	MakeDir(base .. "/" .. d)
end
local LOCAL_BUILDS = dofile(base .. "/local-builds.lua") -- personal builds, not in git
local BUILD_FILES = LOCAL_BUILDS.tree
for _, b in ipairs(BUILD_FILES) do
	writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(BUILDS .. b .. ".xml"))
end
writeAll(USER .. "/Path of Building/Builds/Generals Perforate Zerker.xml", readAll(POB_ROOT .. "spec/TestBuilds/3.13/Generals Perforate Zerker.xml"))
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
local realGetTime = GetTime
local cursorX, cursorY, nowMs, held = -100, -100, 0, { }
function GetCursorPos() return cursorX, cursorY end
function GetTime() return nowMs end
function IsKeyDown(k) return held[k] == true end
local clipboard, opened = nil, { }
function Copy(text) clipboard = text end
function Paste() return clipboard end
function OpenURL(url) opened[#opened + 1] = url end
local redirect = { ["changelog.txt"] = POB_ROOT .. "changelog.txt", ["help.txt"] = POB_ROOT .. "help.txt" }
local writesRefused = { }
local function inCheckout(name)
	return type(name) == "string" and name:gsub("\\", "/"):lower():sub(1, #POB_ROOT) == POB_ROOT:lower()
end
function io.open(name, mode, ...)
	local src = srcRelative(name)
	local writing = mode and mode:match("[wa+]")
	if (src or inCheckout(name)) and writing then
		-- e.g. DataJewelFileLoader's `.bin` cache next to the zip parts: the checkout is read-only
		writesRefused[#writesRefused + 1] = name
		return nil, "write refused by build-runner: " .. name
	end
	if src then return realOpen(src, mode, ...) end
	return realOpen(redirect[name] or name, mode, ...)
end
-- DataJewelFileLoader / DataLegionLookUpTableHelper read the Timeless Jewel data under GetScriptPath()
function GetScriptPath() return POB:sub(1, -2) end
function io.lines(name, ...) return realLines(srcRelative(name) or redirect[name] or name, ...) end
-- Image loads with a src-relative path read the checkout's file.
do
	local mt = getmetatable(NewImageHandle())
	local idx = mt and mt.__index
	if type(idx) == "table" and idx.Load then
		local load = idx.Load
		idx.Load = function(h, fileName, ...) return load(h, srcRelative(fileName) or fileName, ...) end
	else
		error("build-runner: image handle metatable has no Load to redirect")
	end
end

local runner = { }
local function fail(where, err)
	writeAll(OUT .. "startup-error.json", json.encode({ where = where, err = tostring(err), prompt = launch and launch.promptMsg or "nil" }, { indent = true }))
	runner.finished = true
	Exit()
end
local okL, errL = pcall(dofile, POB .. "Launch.lua")
if not okL then fail("dofile Launch", errL) end
local L = launch
L.CheckForUpdate = function() end
SetMainObject(runner)

-- 3. State dump of the BUILD screen.
local function nameOf(tbl, ctrl)
	if not ctrl then return "nil" end
	for k, v in pairs(tbl) do if v == ctrl then return tostring(k) end end
	return "other"
end
local function labelOf(v)
	if type(v) == "table" then return v.label or "nil" end
	return tostring(v)
end
local function drop(c)
	local list = { }
	for i, v in ipairs(c.list) do list[i] = labelOf(v) end
	return { shown = c:IsShown() and true or false, enabled = c:IsEnabled() and true or false, sel = c.selIndex or "nil", list = list }
end
local function buildDump(b)
	local c = b.controls
	local d = { }
	d.buildName = b.buildName or "nil"
	d.dbFileSubPath = b.dbFileSubPath or "nil"
	d.unsaved = b.unsaved and true or false
	d.viewMode = b.viewMode
	d.characterLevel = b.characterLevel
	d.levelBuf = c.characterLevel.buf
	d.levelMode = c.levelScalingButton.label
	d.points = c.pointDisplay.str or "nil"
	d.pointsReq = c.pointDisplay.req or "nil"
	d.mainSocketGroup = b.mainSocketGroup
	for _, k in ipairs({ "classDrop", "ascendDrop", "secondaryAscendDrop", "buildLoadouts", "mainSocketGroup", "mainSkill", "mainSkillPart", "mainSkillMinion", "mainSkillMinionSkill" }) do
		d[k] = drop(c[k])
	end
	d.stageCount = c.mainSkillStageCount:IsShown() and c.mainSkillStageCount.buf or "hidden"
	d.mineCount = c.mainSkillMineCount:IsShown() and c.mainSkillMineCount.buf or "hidden"
	d.minionLibrary = c.mainSkillMinionLibrary:IsShown() and true or false
	local lines = { }
	for i, l in ipairs(c.statBox.list) do
		local u = l.underline and ((l.underline[1] and "1" or "0") .. (l.underline[2] and "1" or "0")) or "-"
		lines[i] = string.format("%s|%s|%s|%s|%s|%s", tostring(l.height), tostring(l.align or "-"), tostring(l.x or "-"), tostring(l[1] or "nil"), tostring(l[2] or "nil"), u)
	end
	d.statBox = lines
	local warns = { }
	for i, w in ipairs(c.warnings.lines) do warns[i] = w end
	d.warnings = warns
	d.sel = nameOf(c, b.selControl)
	local rects = { }
	for _, k in ipairs({ "back", "save", "saveAs", "buildName", "pointDisplay", "levelScalingButton", "characterLevel", "classDrop", "ascendDrop", "secondaryAscendDrop", "buildLoadouts", "mainSocketGroup", "mainSkill", "mainSkillPart", "mainSkillStageCount", "mainSkillMinion", "statBox", "warnings" }) do
		local x, y = c[k]:GetPos()
		local w, h = c[k]:GetSize()
		rects[k] = string.format("%g,%g,%g,%g", x, y, w, h)
	end
	d.rects = rects
	return d
end
-- U3b: the tab's state, the steps with `tab: true` (same fields as pob_ui::tabs::tree::debug_dump).
local function tabDump(b)
	local t, s = b.treeTab, b.spec
	local c = t.controls
	local specs = { }
	for i, v in ipairs(c.specSelect.list) do specs[i] = v end
	local masteries, overrides = { }, { }
	for id, e in pairs(s.masterySelections) do masteries[#masteries + 1] = id .. ":" .. e end
	table.sort(masteries)
	for id, o in pairs(s.hashOverrides) do overrides[#overrides + 1] = id .. ":" .. o.dn end
	table.sort(overrides)
	local rects = { }
	for _, k in ipairs({ "specSelect", "compareCheck", "compareSelect", "reset", "versionText", "versionSelect", "treeSearch", "findTimelessJewel", "treeHeatMap", "nodePowerMaxDepthSelect", "treeHeatMapStatSelect", "powerReport", "powerReportList", "specConvertText", "specConvert", "specConvertAll" }) do
		if c[k]:IsShown() then
			local x, y = c[k]:GetPos()
			local w, h = c[k]:GetSize()
			rects[k] = string.format("%g,%g,%g,%g", x, y, w, h)
		end
	end
	local d = { activeSpec = t.activeSpec, compareSpec = t.activeCompareSpec, isComparing = t.isComparing and true or false, showConvert = t.showConvert and true or false,
		showHeatMap = t.viewer.showHeatMap and true or false, specs = specs, versionSel = c.versionSelect.selIndex, treeVersion = s.treeVersion, classId = s.curClassId,
		ascendClassId = s.curAscendClassId, secondary = s.curSecondaryAscendClassId or "nil", masteries = masteries, overrides = overrides, undo = #s.undo, redo = #s.redo, rects = rects,
		reportShown = c.powerReportList.shown and true or false, reportLabel = c.powerReportList.label or "nil" }
	local rows = { }
	for i, r in ipairs(c.powerReportList.list or { }) do rows[i] = tostring(r.type) .. "|" .. tostring(r.name) .. "|" .. tostring(r.powerStr) .. "|" .. tostring(r.pathDist) .. "|" .. tostring(r.pathPowerStr) end
	table.sort(rows)
	d.report = rows
	local p = main.popups[1]
	if p then
		local q = { title = p.title, width = p.width, height = p.height }
		local pc = p.controls
		local list = pc[1] and pc[1].list and pc[1] or pc.effect
		if list then
			local r = { }
			for i, v in ipairs(list.list) do r[i] = list:GetRowValue(1, i, v) end
			q.rows = r
		end
		if pc.modSelect then
			local r = { }
			for i, v in ipairs(pc.modSelect.list) do r[i] = v.label end
			q.mods = r
			q.modSel = pc.modSelect.selIndex
			q.tattooCount = pc.totalTattoos.label
		end
		if pc.edit then q.edit = pc.edit.buf end
		if pc.msg then q.msg = pc.msg.label end
		d.popup = q
	end
	return d
end

local function dump(st)
	local d = { mode = main.mode or "nil", popups = #main.popups, prompt = launch.promptMsg or "nil", clipboard = clipboard or "nil" }
	local p = main.popups[1]
	if p then
		d.popupTitle = p.title
		d.popupSel = nameOf(p.controls, p.selControl)
		d.popupEdit = p.controls.edit and p.controls.edit.buf or "nil"
	end
	if main.mode == "BUILD" and main.modes.BUILD.controls and main.modes.BUILD.controls.statBox then
		d.build = buildDump(main.modes.BUILD)
	end
	if st.file then
		local f = realOpen(main.buildPath .. st.file, "rb")
		d.file = f and "saved" or "missing"
		if f then
			writeAll(OUT .. st.fileOut, f:read("*a"))
			f:close()
		end
	end
	if main.mode == "BUILD" and main.modes.BUILD.treeTab then
		local b = main.modes.BUILD
		local v = b.treeTab.viewer
		local ids = { }
		for id in pairs(b.spec.allocNodes) do ids[#ids + 1] = id end
		table.sort(ids)
		local tp
		if v.tracePath then tp = { } for i, n in ipairs(v.tracePath) do tp[i] = n.id end end
		d.tree = { zoomLevel = v.zoomLevel, zoomX = string.format("%.6f", v.zoomX), zoomY = string.format("%.6f", v.zoomY), hover = v.hoverNode and v.hoverNode.id or "nil",
			alloc = ids, tracePath = tp or "nil", search = v.searchStr, cursorAt = cursorX .. "," .. cursorY, mainFocus = tostring(main.popups[1] ~= nil), mouseOver = nameOf(b.treeTab.controls, b.treeTab:GetMouseOverControl()), sel = nameOf(b.treeTab.controls, b.treeTab.selControl), bsel = nameOf(b.controls, b.selControl), mainSel = nameOf(main.controls, main.selControl), dragX = v.dragX or "nil", dragging = v.dragging and true or false, buildFlag = b.buildFlag and true or false }
		d.tooltip = v.tooltip and { } or nil
		if v.tooltip then for i, l in ipairs(v.tooltip.lines) do d.tooltip[i] = (l.text or "<sep>") end end
	end
	if st.tab and main.mode == "BUILD" then d.tab = tabDump(main.modes.BUILD) end
	d.writesRefused = table.concat(writesRefused, ",")
	if runner.bfTrace then d.bfTrace = runner.bfTrace runner.bfTrace = { } end
	return d
end

-- 4. Scenario loop.
local names = json.decode(readAll(base .. "/scenes/tree/index.json"))
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

function runner:Step()
	self.frames = self.frames + 1
	if not self.sc then
		local ok, bg = pcall(function() return main.tree[latestTreeVersion].assets.Background2.handle end)
		if not ok or launch.promptMsg or self.frames > 900 + 2000 * self.idx then
			writeAll(OUT .. "startup-error.json", json.encode({ err = tostring(bg), prompt = launch.promptMsg or "nil" }, { indent = true }))
			return self:Finish()
		end
		if self.frames < 5 or bg:ImageSize() == 0 or main.mode ~= "LIST" then return self:Frame() end
		self.idx = self.idx + 1
		local name = names[self.idx]
		if not name then return self:Finish() end
		self.sc = { name = name, def = json.decode(readAll(base .. "/scenes/tree/" .. name .. ".json"), 1, json.null), step = 0, dumps = { }, shots = { }, perf = { } }
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
		-- back to the build list for the next scenario
		if main.mode == "BUILD" then main.modes.BUILD.modFlag = false main:SetMode("LIST") end
		return
	end
	if st.waitAssets then
		-- every ASYNC tree image decoded (GetAsyncCount), then one frame
		if GetAsyncCount() > 0 then sc.step = sc.step - 1 end
		self:Frame()
		return
	end
	if st.openNew then
		-- U3b: a new build (BuildListControl's New: no file)
		main:SetMode("BUILD", false, "Unnamed build")
		self:Frame()
		return
	end
	if st.waitPower then
		-- U3b: the power coroutine ran out (CalcsTab:BuildPower), timed through the frames
		local cc = main.modes.BUILD.calcsTab
		sc.powerT0 = sc.powerT0 or os.clock()
		self:Frame()
		if cc.powerBuilder or cc.powerBuildFlag then
			sc.step = sc.step - 1
		else
			sc.perf[#sc.perf + 1] = { what = "power", ms = (os.clock() - sc.powerT0) * 1000 }
			sc.powerT0 = nil
		end
		return
	end
	if st.perfPower then
		-- U3b: SetPowerCalc(stat) + the frames until the coroutine ran out (GetTime is frozen: one frame, no yield)
		local b = main.modes.BUILD
		local stat
		for _, s in ipairs(b.treeTab.powerStatList) do if s.stat == st.perfPower then stat = s end end
		b.treeTab:SetPowerCalc(stat)
		local t0 = os.clock()
		L:OnFrame()
		while b.calcsTab.powerBuilder or b.calcsTab.powerBuildFlag do L:OnFrame() end
		sc.perf[#sc.perf + 1] = { what = "powerFull", ms = (os.clock() - t0) * 1000 }
		return
	end
	if st.perfSpec then
		-- U3b: SetActiveSpec + the recalculation frame
		local t = main.modes.BUILD.treeTab
		local t0 = os.clock()
		t:SetActiveSpec(st.perfSpec)
		L:OnFrame()
		L:OnFrame()
		sc.perf[#sc.perf + 1] = { what = "spec", ms = (os.clock() - t0) * 1000 }
		return
	end
	if st.search then
		-- what the search edit's changeFunc does (TreeTab.lua:181)
		main.modes.BUILD.treeTab.viewer.searchStr = st.search
	end
	if st.treeOrder then
		local b = main.modes.BUILD
		local ids, groups, conns, subs = { }, { }, { }, { }
		for id in pairs(b.spec.nodes) do ids[#ids + 1] = id end
		for id in pairs(b.spec.tree.groups) do groups[#groups + 1] = id end
		for _, c in ipairs(b.spec.tree.connectors) do conns[#conns + 1] = c.nodeId1 .. "-" .. c.nodeId2 .. ":" .. c.type end
		for id in pairs(b.spec.subGraphs) do subs[#subs + 1] = id end
		writeAll(OUT .. st.treeOrder .. ".json", json.encode({ nodes = ids, groups = groups, connectors = conns, subGraphs = subs, size = b.spec.tree.size }) .. "\n")
		return
	end
	if st.perfAlloc then
		-- the click's work + the recalculation frame: spec:AllocNode / DeallocNode, buildFlag, launch:OnFrame
		local b = main.modes.BUILD
		local node = b.spec.nodes[st.perfAlloc]
		local t0 = os.clock()
		if node.alloc then b.spec:DeallocNode(node) else b.spec:AllocNode(node) end
		b.spec:AddUndoState()
		b.buildFlag = true
		L:OnFrame()
		sc.perf[#sc.perf + 1] = { what = "click", ms = (os.clock() - t0) * 1000 }
		return
	end
	if st.idleMs then
		-- a static tree in the real main loop: inject-cpu.ps1 samples the process (sg/ready)
		if not sc.idleUntil then
			sc.idleUntil = realGetTime() + st.idleMs
			writeAll(base .. "/ready", "1")
		end
		local sec = math.floor((realGetTime() - (sc.idleUntil - st.idleMs)) / 1000)
		sc.idleFrames = sc.idleFrames or { }
		sc.idleFrames[sec + 1] = (sc.idleFrames[sec + 1] or 0) + 1
		if realGetTime() < sc.idleUntil then sc.step = sc.step - 1 else
			sc.idleUntil = nil
			sc.perf[#sc.perf + 1] = { what = "framesPerSecond", list = sc.idleFrames }
		end
		self:Frame()
		return
	end
	if st.perf then
		-- CPU of launch:OnFrame over N frames in one host frame.
		local t0 = os.clock()
		for _ = 1, st.perf do L:OnFrame() end
		sc.perf[#sc.perf + 1] = { what = "frame", frames = st.perf, ms = (os.clock() - t0) * 1000 / st.perf }
		return
	end
	if st.perfRecalc then
		-- CPU of the buildFlag branch of Build:OnFrame (BuildOutput + RefreshStatList) N times.
		local b = main.modes.BUILD
		local t0 = os.clock()
		for _ = 1, st.perfRecalc do
			wipeGlobalCache()
			b.outputRevision = b.outputRevision + 1
			b.skillsTab:UpdateSocketGroups()
			b.calcsTab:BuildOutput()
			b:RefreshStatList()
		end
		sc.perf[#sc.perf + 1] = { what = "recalc", n = st.perfRecalc, ms = (os.clock() - t0) * 1000 / st.perfRecalc }
		return
	end
	if st.open then
		-- what BuildListControl:LoadBuild does (main:SetMode("BUILD", fullFileName, buildName)); timed through the next frame
		main:SetMode("BUILD", main.buildPath .. st.open .. ".xml", st.open)
		local t0 = os.clock()
		self:Frame()
		sc.perf[#sc.perf + 1] = { what = "open", build = st.open, ms = (os.clock() - t0) * 1000 }
		return
	end
	if st.traceBuildFlag then
		-- probe: who sets build.buildFlag (the key is kept out of the table so every write goes through __newindex)
		local b = main.modes.BUILD
		local mt = getmetatable(b)
		local val = rawget(b, "buildFlag")
		rawset(b, "buildFlag", nil)
		self.bfTrace = { }
		local runnerSelf = self
		setmetatable(b, { __index = function(t, k) if k == "buildFlag" then return val end return mt.__index[k] end,
			__newindex = function(t, k, v) if k == "buildFlag" then val = v if v then runnerSelf.bfTrace[#runnerSelf.bfTrace + 1] = debug.traceback("", 2):sub(1, 600) end else rawset(t, k, v) end end })
		return
	end
	if st.cursor then cursorX, cursorY = st.cursor[1], st.cursor[2] end
	if st.cursorNode then
		-- the screen position of a node (PassiveTreeView.lua:281-287) in the 968x712 view of a latest-tree build
		local b = main.modes.BUILD
		local v, n = b.treeTab.viewer, b.spec.nodes[st.cursorNode]
		local W, H = 968, 712
		local scale = math.min(W, H) / b.spec.tree.size * v.zoom
		cursorX, cursorY = math.floor(n.x * scale + v.zoomX + 312 + W / 2), math.floor(n.y * scale + v.zoomY + 32 + H / 2)
	end
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
end
