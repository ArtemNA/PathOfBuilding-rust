#@ SimpleGraphic
-- U6 gate (the Items tab; copy of items-runner.lua + the items dump and the cursor steps of this tab): runs the WHOLE original PoB (Launch.lua, Modules/Main, Modules/Build with every tab, Data, TreeData) of the
-- checkout in the real SimpleGraphic -- read in place (LoadModule / require / io.open / image loads of the src-relative
-- folders are redirected to the checkout, read-only; any relative write is a Lua error) -- against a TEST build folder in
-- the sandbox (sg/user/Path of Building/Builds: copies of builds/pc/3.29 + one golden), and records state + screenshots of
-- the BUILD screen for scripted scenarios (sg/scenes/items/<name>.json; ui-runner.lua's step format plus `open`).
--   pwsh tools/sg-oracle/run.ps1 -Script items-runner.lua [-ScriptArgs <scenario,...>] -Timeout 600
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
local BUILDS = "../../../../builds/pc/3.29/" -- cwd = runtime/sg
local USER = base .. "/user"
local OUT = base .. "/out/items/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local realOpen, realLines = io.open, io.lines
local function readAll(p) local f = assert(realOpen(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(realOpen(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

-- 1. Test build folder (the same list as crates/pob-ui/tests/items.rs).
for _, d in ipairs({ "out", "out/items", "user", "user/Path of Building", "user/Path of Building/Builds" }) do
	MakeDir(base .. "/" .. d)
end
local function writeBuilds()
local LOCAL_BUILDS = dofile(base .. "/local-builds.lua") -- personal builds, not in git
local BUILD_FILES = LOCAL_BUILDS.items or { }
for _, b in ipairs(BUILD_FILES) do
	local src = BUILDS .. b .. ".xml"
	local probe = realOpen(src, "rb")
	if probe then probe:close() else src = "../../../../builds/pc/" .. b .. ".xml" end
	writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(src))
end
for _, b in ipairs({ "Generals Perforate Zerker", "Dual Savior", "Dual Wield Cospris CoC", "Mirage Archer Toxic Rain", "OccVortex" }) do
	writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(POB_ROOT .. "spec/TestBuilds/3.13/" .. b .. ".xml"))
end
-- A public golden has no minions, mines or stages: OccVortex + three groups (the same edit as crates/pob-ui/tests/calcs.rs).
do
	local gem = '<Gem level="20" gemId="Metadata/Items/Gems/%s" skillId="%s" qualityId="Default" quality="0" enabled="true" nameSpec="%s"/>'
	local group = function(g) return '<Skill mainActiveSkillCalcs="1" enabled="true" mainActiveSkill="1">' .. g .. '</Skill>' end
	local x = readAll(POB_ROOT .. "spec/TestBuilds/3.13/OccVortex.xml")
	x = x:gsub("(<Build [^>]*>)", function(m) return m .. '<Spectre id="Metadata/Monsters/Axis/AxisCaster"/><Spectre id="Metadata/Monsters/DemonModular/DemonModularBladeVortex"/>' end, 1)
	x = x:gsub("(<Skills [^>]*>)", function(m)
		return m .. group(gem:format("SkillGemRaiseSpectre", "RaiseSpectre", "Raise Spectre"))
			.. group(gem:format("SkillGemFireball", "Fireball", "Fireball") .. gem:format("SkillGemSupportBlastchainMine", "SupportBlastchainMine", "Blastchain Mine"))
			.. group(gem:format("SkillGemIncinerate", "Incinerate", "Incinerate"))
	end, 1)
	writeAll(USER .. "/Path of Building/Builds/Spectre Test.xml", x)
end
-- ItemsTab.lua:1350-1366 and :1451-1469 over a public golden (the same edit as crates/pob-ui/tests/items.rs).
do
	local w = '<TradeSearchWeights><Stat stat="FullDPS" label="old label" weightMult="1.5"/><Stat stat="NoSuchStat" label="gone" weightMult="2"/><Stat stat="Life" label="zero" weightMult="0"/></TradeSearchWeights><TradeSearchWeights><Stat stat="TotalEHP" weightMult="0.125"/><Stat stat="Armour" label="not a number" weightMult="x"/></TradeSearchWeights>'
	local x = readAll(POB_ROOT .. "spec/TestBuilds/3.13/OccVortex.xml"):gsub("</Items>", function() return w .. "</Items>" end, 1)
	writeAll(USER .. "/Path of Building/Builds/Trade Weights Test.xml", x)
end
end
writeBuilds()
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
-- U3c: no background scripts: FetchLeagues of the Timeless popup (TreeTab.lua:2358) is the network, and `launch:DownloadPage` does nothing without an id
function LaunchSubScript() return nil end
local realGetTime, realIsKeyDown = GetTime, IsKeyDown
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

local calcsDump, itemsDump
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
		if p.title == "Find a Timeless Jewel" then
			local labels = function(c) local r = { } for i, v in ipairs(c.list) do r[i] = type(v) == "table" and v.label or v end return r end
			local rows = { }
			for i, v in ipairs(pc.searchResults.list) do rows[i] = v.label end
			local boxes, i = { }, 1
			while pc["protectAllocatedBox" .. i] do boxes[i] = pc["protectAllocatedBox" .. i].label i = i + 1 end
			local td = b.timelessData
			q.timeless = { jewelSel = pc.jewelSelect.selIndex, conquerorSel = pc.conquerorSelect.selIndex, conquerors = labels(pc.conquerorSelect), socketSel = pc.socketSelect.selIndex, sockets = labels(pc.socketSelect),
				nodeSel = pc.nodeSelect.selIndex or "nil", nodes = labels(pc.nodeSelect), fallbackModes = labels(pc.fallbackWeightsList), slider1 = pc.nodeSliderValue.label, slider2 = pc.nodeSlider2Value.label, slider3 = pc.nodeSlider3Value.label,
				slider2Label = pc.nodeSlider2Label.label, slider2Enabled = pc.nodeSlider2.enabled ~= false, searchList = pc.searchList.buf, searchListFallback = pc.searchListFallback.buf, fallbackShown = pc.searchListFallback.shown and true or false,
				results = rows, resultSel = pc.searchResults.selIndex or "nil", highlight = pc.searchResults.highlightIndex or "nil", tradeEnabled = pc.searchTradeButton.enabled and true or false, tradeLabel = pc.searchTradeButton.label,
				protect = boxes, filter = pc.socketFilter.state and true or false, distance = td.socketFilterDistance or 0, jewelSocket = td.jewelSocket.id }
		end
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
		d.popupOrder = { }
		for k in pairs(p.controls) do d.popupOrder[#d.popupOrder + 1] = tostring(k) end
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
	if main.mode == "BUILD" and st.calcs then d.calcs = calcsDump(main.modes.BUILD) end
	if main.mode == "BUILD" then d.items = itemsDump(main.modes.BUILD) end
	if p and p.title == "Spectre Library" then
		local dest, src = { }, p.controls.source
		for i, v in ipairs(p.controls.list.list) do dest[i] = v end
		d.library = { dest = dest, source = #src.list, sourceFirst = src.list[1] or json.null, sel = src.selIndex or json.null, search = src.controls.searchText.buf }
	end
	d.writesRefused = table.concat(writesRefused, ",")
	if runner.bfTrace then d.bfTrace = runner.bfTrace runner.bfTrace = { } end
	return d
end

-- U4: the Calcs tab (the same fields as pob_ui::tabs::calcs::CalcsTab::debug_dump).
local function bdDump(b)
	local d = { shown = b.shown and true or false, pinned = b.pinned and true or false, scroll = b.controls.scrollBar.offset, sections = { } }
	if not b.shown then return d end
	d.rect = tostring(b.x) .. "," .. tostring(b.y) .. "," .. tostring(b.width) .. "," .. tostring(b.height)
	d.contentWidth, d.contentHeight = b.contentWidth, b.contentHeight
	for i, s in ipairs(b.sectionList) do
		local o = { type = s.type, width = s.width, height = s.height, offset = s.offset }
		if s.type == "TEXT" then
			o.lines, o.textSize = { }, s.textSize
			for j, l in ipairs(s.lines) do o.lines[j] = l end
		elseif s.type == "RADIUS" then
			o.radius = s.radius
		else
			o.label, o.footer = s.label or json.null, s.footer or json.null
			o.cols, o.rows = { }, { }
			for j, c in ipairs(s.colList) do o.cols[j] = { label = c.label, key = c.key, right = c.right and true or false, width = c.width or json.null } end
			for j, r in ipairs(s.rowList) do
				local row = { }
				for k, c in ipairs(s.colList) do
					local v = r[c.key]
					row[k] = (type(v) == "string" or type(v) == "number") and tostring(v) or json.null
				end
				o.rows[j] = row
			end
		end
		d.sections[i] = o
	end
	return d
end
calcsDump = function(b)
	local t = b.calcsTab
	local d = { input = { }, scroll = t.controls.scrollBar.offset, search = t.controls.search.buf, pinned = t.displayPinned and true or false, display = json.null, sections = { } }
	for k, v in pairs(t.input) do d.input[k] = v end
	d.sel = "nil"
	local sel = t.selControl
	if sel then
		d.sel = "other"
		if sel == t.controls.search then d.sel = "search" elseif sel == t.controls.breakdown then d.sel = "breakdown" elseif sel == t.controls.scrollBar then d.sel = "scrollBar" end
		for _, s in ipairs(t.sectionList) do if s == sel then d.sel = s.id end end
	end
	for i, s in ipairs(t.sectionList) do
		local x, y = s:GetPos()
		local w, h = s:GetSize()
		local q = { id = s.id, enabled = s.enabled and true or false, overlay = s.isOverlay and true or false, rect = tostring(x) .. "," .. tostring(y) .. "," .. tostring(w) .. "," .. tostring(h), subs = { } }
		for j, sub in ipairs(s.subSection) do
			local rows = { }
			for k, r in ipairs(sub.data) do
				rows[k] = r.enabled and "1" or "0"
				for l, c in ipairs(r) do if c == t.displayData then d.display = s.id .. "/" .. j .. "/" .. k .. "/" .. l end end
			end
			q.subs[j] = { id = sub.id, collapsed = sub.collapsed and true or false, rows = table.concat(rows) }
		end
		d.sections[i] = q
	end
	local sc = t.sectionList[1].controls
	local list = function(c)
		local r = { }
		for i, v in ipairs(c.list) do r[i] = type(v) == "table" and (v.label or "") or tostring(v) end
		return { shown = c:IsShown() and true or false, enabled = c:IsEnabled() and true or false, sel = c.selIndex or 0, list = r }
	end
	d.skill = { mainSocketGroup = list(sc.mainSocketGroup), mainSkill = list(sc.mainSkill), mainSkillPart = list(sc.mainSkillPart), mainSkillMinion = list(sc.mainSkillMinion), mainSkillMinionSkill = list(sc.mainSkillMinionSkill),
		mode = sc.mode.selIndex, showMinion = sc.showMinion.state and true or false, stage = sc.mainSkillStageCount:IsShown() and sc.mainSkillStageCount.buf or "hidden", mine = sc.mainSkillMineCount:IsShown() and sc.mainSkillMineCount.buf or "hidden" }
	d.breakdown = bdDump(t.controls.breakdown)
	d.sidebar = bdDump(b.controls.breakdown)
	d.allocMain, d.allocCalcs = { }, { }
	for id in pairs(t.mainEnv.allocNodes) do d.allocMain[#d.allocMain + 1] = id end
	for id in pairs(t.calcsEnv.allocNodes) do d.allocCalcs[#d.allocCalcs + 1] = id end
	d.bsel = b.selControl == b.controls.breakdown and "breakdown" or (b.selControl and "other" or "nil")
	d.unsaved = b.unsaved and true or false
	return d
end
-- U6: the Items tab (the same fields as pob_ui::tabs::items::ItemsTab::debug_dump).
itemsDump = function(b)
	local t = b.itemsTab
	local c = t.controls
	local function dropOf(x)
		local list = { }
		for i, v in ipairs(x.list or { }) do list[i] = type(v) == "table" and (v.label or "") or tostring(v) end
		local shownNow = x:IsShown() and true or false
		return { shown = shownNow, enabled = shownNow and x:IsEnabled() and true or false, sel = x.selIndex or 0, list = list }
	end
	local function tipOf(tt)
		local r = { }
		for i, l in ipairs(tt.lines) do r[i] = l.text or "<sep>" end
		return r
	end
	local rows, order, items = { }, { }, { }
	for i, id in ipairs(c.itemList.list) do rows[i] = c.itemList:GetRowValue(1, i, id) end
	for i, id in ipairs(t.itemOrderList) do
		order[i] = id
		items[i] = tostring(id) .. "|" .. t.items[id]:BuildRaw()
	end
	local slots = { }
	local slotTip
	for _, s in ipairs(t.orderedSlots) do
		if s:IsShown() then
			local x, y = s:GetPos()
			slots[s.slotName] = { label = s.label, sel = s.selItemId, pos = string.format("%g,%g", x, y), active = s.active == true, drop = dropOf(s) }
			if s.tooltip.lines[1] then slotTip = tipOf(s.tooltip) end
		end
	end
	local sets = { }
	for i, id in ipairs(t.itemSetOrderList) do sets[i] = { id = id, title = t.itemSets[id].title or json.null, second = t.itemSets[id].useSecondWeaponSet and true or false } end
	local d = { rows = rows, order = order, items = items, slots = slots, sets = sets, active = t.activeItemSetId, setSelect = dropOf(c.setSelect), specSelect = dropOf(c.specSelect),
		listSel = c.itemList.selIndex or json.null, listTip = tipOf(c.itemList.tooltip), filter = dropOf(c.itemList.controls.loadoutFilter), undo = #t.undo, redo = #t.redo,
		showStat = t.showStatDifferences and true or false, scrollH = c.scrollBarH.offset, scrollV = c.scrollBarV.offset, selectDB = c.selectDB.selIndex, slotTip = slotTip or json.null }
	for _, key in ipairs({ "uniqueDB", "rareDB" }) do
		local db = c[key]
		if db:IsShown() then
			local first = { }
			for i, it in ipairs(db.list) do first[i] = db:GetRowValue(1, i, it) end
			d[key] = { n = #db.list, first = first, tip = tipOf(db.tooltip), slot = dropOf(db.controls.slot), type = dropOf(db.controls.type), search = db.controls.search.buf, mode = db.controls.searchMode.selIndex }
		end
	end
	local item = t.displayItem
	if item then
		local rects = { }
		for _, n in ipairs({ "addDisplayItem", "displayItemEnchant", "displayItemAnoint", "displayItemCorrupt", "displayItemAddImplicit", "displayItemInfluence", "displayItemQualityEdit", "displayItemCatalyst",
			"displayItemClusterJewelSkill", "craftingSorting", "displayItemAddCustom", "displayItemAddCrucible", "displayItemRangeLine", "displayItemTooltipAnchor", "displayItemAffix1", "displayItemSocket1" }) do
			local x, y = c[n]:GetPos()
			local w, h = c[n]:GetSize()
			rects[n] = c[n]:IsShown() and string.format("%g,%g,%g,%g", x, y, w, h) or false
		end
		local affixes = { }
		for i = 1, 9 do
			local a = c["displayItemAffix" .. i]
			if a:IsShown() then affixes[#affixes + 1] = { drop = dropOf(a), slider = a.slider:IsShown() and true or false, val = a.slider.val } end
		end
		d.display = { raw = item:BuildRaw(), id = item.id or json.null, tooltip = tipOf(t.displayItemTooltip), rects = rects, variant = dropOf(c.displayItemVariant), version = dropOf(c.displayItemVersion),
			rangeLine = dropOf(c.displayItemRangeLine), rangeVal = c.displayItemRangeSlider.val or 0, quality = c.displayItemQualityEdit.buf, catalyst = dropOf(c.displayItemCatalyst),
			influence = { dropOf(c.displayItemInfluence), dropOf(c.displayItemInfluence2) }, affixes = affixes, custom = item.customCount or 0 }
	end
	local p = main.popups[1]
	if p then
		local q = { }
		for _, n in ipairs({ "rarity", "type", "base", "source", "sort", "enchantment", "enchantmentSource", "skill", "modSelect", "implicit1", "implicit2", "edit", "title", "custom", "save" }) do
			local x = p.controls[n]
			if x and x.list then q[n] = dropOf(x) end
			if x and x.buf then q[n] = x.buf end
			if n == "save" and x then q.saveLabel = x:GetProperty("label") q.saveTip = tipOf(x.tooltip) q.saveEnabled = x:IsEnabled() and true or false end
		end
		if p.controls.searchResults then q.timelessTip = tipOf(p.controls.searchResults.tooltip) end
		local db = p.controls.notableDB
		if db then
			local rows = { }
			for i, node in ipairs(db.list) do rows[i] = db:GetRowValue(1, i, node) end
			q.notables = rows
			q.notableTip = tipOf(db.tooltip)
		end
		d.popup = q
	end
	local sel = t.selControl
	if sel then
		d.sel = nameOf(c, sel)
		for _, s in ipairs(t.orderedSlots) do if s == sel then d.sel = "slot:" .. s.slotName end end
	end
	return d
end
local function itemsCursor(st)
	local b = main.modes.BUILD
	local t = b.itemsTab
	local function pc(name)
		local controls = main.popups[1].controls
		return controls[name] or controls[tonumber(name)]
	end
	local function ctl(name)
		if name:sub(1, 5) == "slot:" then return t.slots[name:sub(6)] end
		return t.controls[name]
	end
	local function centre(c, dx, dy)
		local x, y = c:GetPos()
		local w, h = c:GetSize()
		cursorX, cursorY = math.floor(x + w / 2) + (dx or 0), math.floor(y + h / 2) + (dy or 0)
	end
	if st.cursorItem then
		centre(ctl(st.cursorItem[1]), st.cursorItem[2], st.cursorItem[3])
	elseif st.cursorBuild then
		centre(b.controls[st.cursorBuild[1]], st.cursorBuild[2], st.cursorBuild[3])
	elseif st.cursorPopup then
		centre(pc(st.cursorPopup[1]), st.cursorPopup[2], st.cursorPopup[3])
	elseif st.cursorChild then
		centre(ctl(st.cursorChild[1]).controls[st.cursorChild[2]], st.cursorChild[3], st.cursorChild[4])
	elseif st.cursorPopupChild then
		centre(pc(st.cursorPopupChild[1]).controls[st.cursorPopupChild[2]])
	elseif st.cursorSkillCtl then
		centre(b.skillsTab.controls[st.cursorSkillCtl])
	elseif st.cursorSkillRow then
		local x, y = b.skillsTab.controls.groupList:GetPos()
		local row = st.cursorSkillRow > 0 and st.cursorSkillRow or #b.skillsTab.socketGroupList
		cursorX, cursorY = x + 10, y + 2 + 16 * (row - 1) + 8
	elseif st.cursorItemRow then
		local x, y = ctl(st.cursorItemRow[1]):GetPos()
		cursorX, cursorY = x + 10, y + 2 + 16 * (st.cursorItemRow[2] - 1) + 8
	elseif st.cursorPopupRow then
		local x, y = pc(st.cursorPopupRow[1]):GetPos()
		cursorX, cursorY = x + 10, y + 2 + 16 * (st.cursorPopupRow[2] - 1) + 8
	elseif st.cursorPopupPick then
		local c = pc(st.cursorPopupPick[1])
		local x, y = c:GetPos()
		local _, h = c:GetSize()
		cursorX, cursorY = x + 10, y + h + 2 + (st.cursorPopupPick[2] - 1) * (h - 4) + math.floor((h - 4) / 2)
	elseif st.cursorBuildPick then
		local c = b.controls[st.cursorBuildPick[1]]
		local x, y = c:GetPos()
		local _, h = c:GetSize()
		cursorX, cursorY = x + 10, y + h + 2 + (st.cursorBuildPick[2] - 1) * (h - 4) + math.floor((h - 4) / 2)
	elseif st.cursorPick then
		local c = ctl(st.cursorPick[1])
		local x, y = c:GetPos()
		local _, h = c:GetSize()
		cursorX, cursorY = x + 10, y + h + 2 + (st.cursorPick[2] - 1) * (h - 4) + math.floor((h - 4) / 2)
	end
end
local function calcsCursor(st)
	local b = main.modes.BUILD
	local t = b.calcsTab
	local function section(id) for _, s in ipairs(t.sectionList) do if s.id == id then return s end end error("section " .. id) end
	if st.cursorCell then -- { section id, row label, 1-based column }: the first enabled row of that label
		local s = section(st.cursorCell[1])
		for _, sub in ipairs(s.subSection) do
			for _, r in ipairs(sub.data) do
				if r.enabled and r.label == st.cursorCell[2] then
					local c = r[st.cursorCell[3] or 1]
					cursorX, cursorY = c.x + 5, c.y + 5
					return
				end
			end
		end
		error("cursorCell: no row " .. st.cursorCell[2])
	elseif st.cursorControl then -- { section id, control name }
		local c = section(st.cursorControl[1]).controls[st.cursorControl[2]]
		local x, y = c:GetPos()
		local w, h = c:GetSize()
		cursorX, cursorY = math.floor(x + w / 2), math.floor(y + h / 2)
	elseif st.cursorOverlay then -- { section id, dx, dy }: inside a pane
		local s = section(st.cursorOverlay[1])
		cursorX, cursorY = s.overlayX + st.cursorOverlay[2], s.overlayY + st.cursorOverlay[3]
	elseif st.cursorRel then
		cursorX, cursorY = cursorX + st.cursorRel[1], cursorY + st.cursorRel[2]
	elseif st.cursorStat then -- a stat box line whose text has this (plain) string
		local c = b.controls.statBox
		local x, y = c:GetPos()
		local rowY = y - c.controls.scrollBar.offset + 2
		for _, l in ipairs(c.list) do
			if l[1] and l[1]:find(st.cursorStat, 1, true) then
				cursorX, cursorY = x + 100, rowY + 2
				return
			end
			rowY = rowY + l.height
		end
		error("cursorStat " .. st.cursorStat)
	elseif st.cursorBd then -- { which, row.source, column key }: the first table row of that source in a popup
		local bd = st.cursorBd[1] == "sidebar" and b.controls.breakdown or t.controls.breakdown
		for _, s in ipairs(bd.sectionList) do
			if s.type == "TABLE" then
				for ri, r in ipairs(s.rowList) do
					if r.source == st.cursorBd[2] then
						local y = bd.y - bd.controls.scrollBar.offset + s.offset + (s.label and 16 or 0) + 20 + 14 * (ri - 1) + 5
						local x = bd.x + 4
						for _, c in ipairs(s.colList) do
							if c.key == st.cursorBd[3] then break end
							x = x + (c.width or 0)
						end
						cursorX, cursorY = x + 3, y
						return
					end
				end
			end
		end
		error("cursorBd: no row of " .. st.cursorBd[2])
	end
end

-- 4. Scenario loop.
local names = json.decode(readAll(base .. "/scenes/items/index.json"))
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
		if self.frames < 5 or bg:ImageSize() == 0 or main.mode ~= "LIST" or main.uniqueDB.loading or main.rareDB.loading then return self:Frame() end
		self.idx = self.idx + 1
		local name = names[self.idx]
		if not name then return self:Finish() end
		clipboard = nil writeBuilds() wipeTable(main.popups) self.sc = { name = name, def = json.decode(readAll(base .. "/scenes/items/" .. name .. ".json"), 1, json.null), step = 0, dumps = { }, shots = { }, perf = { } }
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
		local c = realOpen(base .. "/cursor.txt", "r")
		if c then
			local x, y = c:read("*n", "*n")
			c:close()
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
		-- every ASYNC tree image decoded (GetAsyncCount), then one frame
		if GetAsyncCount() > 0 then sc.step = sc.step - 1 end
		self:Frame()
		return
	end
	if st.closeBuild then
		-- back to the list without the save popup, for the next `open` of the scene
		main.modes.BUILD.modFlag = false
		main.modes.BUILD.skillsTab.modFlag = false
		main.modes.BUILD.calcsTab.modFlag = false
		main.modes.BUILD.unsaved = false
		main:SetMode("LIST")
		self:Frame()
		return
	end
	if st.variantOrder then
		-- R13: which variant `pairs(possibleVariants)` (SkillsTab.lua:474) gave this process, for crates/pob-ui/tests/items.rs
		local gems = { }
		for _, set in pairs(main.modes.BUILD.skillsTab.skillSets) do
			for _, g in ipairs(set.socketGroupList) do
				for _, x in ipairs(g.gemList) do
					if x.gemData and x.skillId then gems[#gems + 1] = { gemId = x.gemData.gameId, skillId = x.skillId, variantId = x.gemData.variantId } end
				end
			end
		end
		writeAll(OUT .. st.variantOrder .. ".json", json.encode({ gems = gems }) .. "\n")
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
	if st.legionLines then
		-- U3d: what TreeTab.lua:2006 asks of Data/StatDescriptions, for crates/pob-data/src/legion_stat_lines.json
		local lines = { }
		local legion = main.tree[latestTreeVersion].legion
		for _, list in ipairs({ legion.nodes, legion.additions }) do
			for _, p in ipairs(list) do
				if p.sortedStats and #p.sortedStats > 1 then
					local byStat = { }
					for _, statKey in ipairs(p.sortedStats) do
						local statValues = { }
						for key in pairs(p.stats) do statValues[key] = key == statKey and 100 or 0 end
						byStat[statKey] = data.describeStats(statValues, "stat_descriptions")[1]
					end
					lines[p.id] = byStat
				end
			end
		end
		writeAll(OUT .. st.legionLines .. ".json", json.encode({ pobSha = UPSTREAM.sha, lines = lines }, { indent = true }) .. "\n")
		return
	end
	if st.treeOrder then
		local b = main.modes.BUILD
		local ids, groups, conns, subs = { }, { }, { }, { }
		for id in pairs(b.spec.nodes) do ids[#ids + 1] = id end
		for id in pairs(b.spec.tree.groups) do groups[#groups + 1] = id end
		for _, c in ipairs(b.spec.tree.connectors) do conns[#conns + 1] = c.nodeId1 .. "-" .. c.nodeId2 .. ":" .. c.type end
		for id in pairs(b.spec.subGraphs) do subs[#subs + 1] = id end
		local clusterNames = { }
		for name in pairs(b.spec.tree.clusterNodeMap) do clusterNames[#clusterNames + 1] = name end
		-- U3d: `pairs(nodesInRadius[3])` of every socket: `pairs(self.nodes)` fills it in an order of this process, and the search adds its non-integer weights in that order
		local radiusOrder = { }
		for id, node in pairs(b.spec.tree.nodes) do
			if node.isJewelSocket and node.nodesInRadius then
				local o = { }
				for nodeId in pairs(node.nodesInRadius[3]) do o[#o + 1] = nodeId end
				radiusOrder[tostring(id)] = o
			end
		end
		writeAll(OUT .. st.treeOrder .. ".json", json.encode({ nodes = ids, groups = groups, connectors = conns, subGraphs = subs, clusterNames = clusterNames, size = b.spec.tree.size, radiusOrder = radiusOrder }) .. "\n")
		return
	end
	if st.treePaths then
		-- U3c: `node.path` of every node with a path + `pairs(allocNodes)`: what equal shortest paths resolved to in this spec's history
		local b = main.modes.BUILD
		local paths, order = { }, { }
		for id, node in pairs(b.spec.nodes) do
			if node.path then
				local p = { }
				for i, n in ipairs(node.path) do p[i] = n.id end
				paths[tostring(id)] = p
			end
		end
		for id in pairs(b.spec.allocNodes) do order[#order + 1] = id end
		writeAll(OUT .. st.treePaths .. ".json", json.encode({ paths = paths, allocOrder = order }) .. "\n")
		return
	end
	if st.buffOrder then
		-- R13: the mods of env.buffs[name] of this process (crates/pob-engine set_mod_row_order_sample)
		local b = main.modes.BUILD
		local keys = { }
		for _, list in ipairs({ b.calcsTab.calcsEnv.player.breakdown.SkillBuffs.modList, b.calcsTab.calcsEnv.player.breakdown.SkillDebuffs.modList }) do
			for _, r in ipairs(list) do keys[#keys + 1] = r.mod.name .. "|" .. r.mod.flags .. "|" .. r.mod.keywordFlags .. "|" .. tostring(r.mod.source or "") end
		end
		writeAll(OUT .. st.buffOrder .. ".json", json.encode({ keys = keys }) .. "\n")
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
		assert(main.mode == "BUILD" and main.modes.BUILD.dbFileName, "open: no build " .. st.open) -- MINEFIELD R55
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
	calcsCursor(st)
	itemsCursor(st)
	if st.clipboard then clipboard = st.clipboard end
	if st.cursorNode then
		-- the screen position of a node (PassiveTreeView.lua:281-287) in the 968x712 view of a latest-tree build
		local b = main.modes.BUILD
		local v, n = b.treeTab.viewer, b.spec.nodes[st.cursorNode]
		local W, H = 968, 712
		local scale = math.min(W, H) / b.spec.tree.size * v.zoom
		cursorX, cursorY = math.floor(n.x * scale + v.zoomX + 312 + W / 2), math.floor(n.y * scale + v.zoomY + 32 + H / 2)
	end
	if st.cursorFind then
		-- U3c: the smallest id of a kind of node (compare-only, allocated only in the compare spec, an allocated cluster socket), at its place on screen
		local b = main.modes.BUILD
		local v, spec, cmp = b.treeTab.viewer, b.spec, b.treeTab.viewer.compareSpec
		local best
		local W, H = 968, 712
		local scale = math.min(W, H) / spec.tree.size * v.zoom
		local function take(id, n)
			if n.x and (not best or id < best.id) then
				local x, y = math.floor(n.x * scale + v.zoomX + 312 + W / 2), math.floor(n.y * scale + v.zoomY + 32 + H / 2)
				if st.cursorFind == "compareOnly" or (x > 320 and x < 1270 and y > 40 and y < 736) then best = { id = id, x = x, y = y } end
			end
		end
		if st.cursorFind == "clusterSocket" then
			for id, n in pairs(spec.nodes) do if n.alloc and n.expansionJewel and id < 65536 then take(id, n) end end
		else
			for id, c in pairs(cmp.nodes) do
				if c.alloc and ((st.cursorFind == "compareOnly" and not spec.nodes[id]) or (st.cursorFind == "compareAlloc" and spec.nodes[id] and not spec.nodes[id].alloc)) then take(id, c) end
			end
		end
		assert(best, "cursorFind: none " .. st.cursorFind)
		cursorX, cursorY = best.x, best.y
	end
	if st.time then nowMs = st.time end
	for _, k in ipairs(st.hold or { }) do held[k] = true end
	for _, k in ipairs(st.release or { }) do held[k] = nil end
	local perfT0 = os.clock()
	for _, e in ipairs(st.events or { }) do
		if e[1] == "KeyDown" then L:OnKeyDown(e[2], e[3] == true)
		elseif e[1] == "KeyUp" then L:OnKeyUp(e[2])
		else L:OnChar(e[2]) end
	end
	self:Frame()
	if st.perfClick then sc.perf[#sc.perf + 1] = { what = st.perfClick, ms = (os.clock() - perfT0) * 1000 } end
	if st.dump then
		local ok, d = xpcall(function() return dump(st) end, debug.traceback)
		sc.dumps[#sc.dumps + 1] = { step = sc.step, state = ok and d or ("dump error: " .. tostring(d)) }
	end
	if st.shot then sc.waitShot, sc.seen = st.shot, listShots() end
end

function runner:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	Exit()
end

-- U3c: the events of the real window (scene real-input, inject-real.ps1)
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
function runner:RealState()
	local b = main.modes.BUILD
	local t = b.itemsTab
	local pos = function(c) local x, y = c:GetPos() local w, h = c:GetSize() return { x = math.floor(x + w / 2), y = math.floor(y + h / 2) } end
	local lx, ly = t.controls.itemList:GetPos()
	local db = t.controls.uniqueDB
	local p = main.popups[1]
	return { ready = true, events = realEvents, itemsReal = { viewMode = b.viewMode, modeBtn = pos(b.controls.modeItems), rowPos = { x = lx + 10, y = ly + 2 + 16 * 1 + 8 }, listSel = t.controls.itemList.selIndex or 0, displayRaw = t.displayItem and t.displayItem:BuildRaw() or "nil", craftPos = pos(t.controls.craftDisplayItem), popupSave = p and pos(p.controls.save) or json.null, searchPos = pos(db.controls.search), search = db.controls.search.buf, dbN = #db.list, scrollV = t.controls.scrollBarV.offset } }
end
function runner:OnExit()
end
