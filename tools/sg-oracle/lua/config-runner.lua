#@ SimpleGraphic
-- U7 gate (the Config and Notes tabs; copy of calcs-runner.lua + the dumps and the cursor steps of these tabs): runs the WHOLE original PoB (Launch.lua, Modules/Main, Modules/Build with every tab, Data, TreeData) of the
-- checkout in the real SimpleGraphic -- read in place (LoadModule / require / io.open / image loads of the src-relative
-- folders are redirected to the checkout, read-only; any relative write is a Lua error) -- against a TEST build folder in
-- the sandbox (sg/user/Path of Building/Builds: copies of builds/pc/3.29 + one golden), and records state + screenshots of
-- the BUILD screen for scripted scenarios (sg/scenes/config/<name>.json; ui-runner.lua's step format plus `open`).
--   pwsh tools/sg-oracle/run.ps1 -Script config-runner.lua [-ScriptArgs <scenario,...>] -Timeout 600
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
local OUT = base .. "/out/config/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local realOpen, realLines = io.open, io.lines
local function readAll(p) local f = assert(realOpen(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(realOpen(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

-- 1. Test build folder (the same list as crates/pob-ui/tests/tree.rs).
for _, d in ipairs({ "out", "out/config", "user", "user/Path of Building", "user/Path of Building/Builds" }) do
	MakeDir(base .. "/" .. d)
end
local LOCAL_BUILDS = dofile(base .. "/local-builds.lua") -- personal builds, not in git
local BUILD_FILES = LOCAL_BUILDS.config or { }
for _, b in ipairs(BUILD_FILES) do
	-- a local copy next to the fixtures when builds/pc has no such build
	local src = BUILDS .. b .. ".xml"
	local f = realOpen(src, "rb")
	if f then f:close() else src = "../../fixtures/config/local-builds/" .. b .. ".xml" end
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
	-- U10: a skill for the only option of type `float` (ConfigOptions.lua:527); the same edit as crates/pob-ui/tests/config.rs
	local y = readAll(POB_ROOT .. "spec/TestBuilds/3.13/OccVortex.xml")
	y = y:gsub("(<Skills [^>]*>)", function(m) return m .. '<Skill mainActiveSkillCalcs="1" enabled="true" mainActiveSkill="1"><Gem level="20" gemId="Metadata/Items/Gems/SkillGemIntuitiveLink" skillId="IntuitiveLink" qualityId="Default" quality="0" enabled="true" nameSpec="Intuitive Link"/></Skill>' end, 1)
	writeAll(USER .. "/Path of Building/Builds/Float Test.xml", y)
end
	-- U7: a loadout over a second tree and a second config set (the same edit as crates/pob-ui/tests/config.rs).
	do
		local x = readAll(POB_ROOT .. "spec/TestBuilds/3.13/OccVortex.xml")
		x = x:gsub("(<Spec [^>]*>.-</Spec>)", function(m) return m .. "\n" .. m:gsub("<Spec ", '<Spec title="Alt" ', 1) end, 1)
		x = x:gsub("<Config.-</Config>", '<Config activeConfigSet="1"><ConfigSet id="1" title="Default"/><ConfigSet id="2" title="Alt"><Input name="buffOnslaught" boolean="true"/></ConfigSet></Config>', 1)
		writeAll(USER .. "/Path of Building/Builds/Loadout Test.xml", x)
	end
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
	writeAll(OUT .. "startup-error.json", json.encode({ where = where, err = tostring(err), scene = runner.sc and runner.sc.name, step = runner.sc and runner.sc.step, prompt = launch and launch.promptMsg or "nil" }, { indent = true }))
	if runner.sc then writeAll(OUT .. runner.sc.name .. ".json", json.encode({ name = runner.sc.name, dumps = runner.sc.dumps, shots = runner.sc.shots })) end
	runner.finished = true
	Exit()
end
local okL, errL = pcall(dofile, POB .. "Launch.lua")
if not okL then fail("dofile Launch", errL) end
local L = launch
L.CheckForUpdate = function() end
SetMainObject(runner)

local configDump
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
	if main.mode == "BUILD" then d.config, d.notes, d.popup = configDump(main.modes.BUILD) d.activeSpec = main.modes.BUILD.treeTab.activeSpec end
	d.writesRefused = table.concat(writesRefused, ",")
	if runner.bfTrace then d.bfTrace = runner.bfTrace runner.bfTrace = { } end
	return d
end

-- U7: the tabs (the same fields as pob_ui::tabs::config::ConfigTab::debug_dump, notes::NotesTab::debug_dump, config::debug_popup).
local function ts(v) return tostring(v) end
local function rectOf(c)
	local x, y = c:GetPos()
	local w, h = c:GetSize()
	return ts(x) .. "," .. ts(y) .. "," .. ts(w) .. "," .. ts(h)
end
local function borderOf(c)
	local r, g, b = c.borderFunc()
	return ts(r) .. "," .. ts(g) .. "," .. ts(b)
end
local function tipLines(c)
	local out = { }
	if c.tooltip then for i, l in ipairs(c.tooltip.lines) do out[i] = l.text or "<sep>" end end
	return out
end
configDump = function(b)
	local t = b.configTab
	local cs = t.configSets[t.activeConfigSetId]
	local d = { active = t.activeConfigSetId, order = { }, titles = { }, search = t.controls.search.buf, scroll = t.controls.scrollBar.offset, toggleConfigs = t.toggleConfigs and true or false,
		toggleLabel = t.controls.toggleConfigs:GetProperty("label"), modFlag = t.modFlag and true or false, undo = #t.undo, redo = #t.redo, enemyLevel = t.enemyLevel, input = { }, placeholder = { }, blocks = { }, sections = { }, labels = { }, tips = { } }
	for i, id in ipairs(t.configSetOrderList) do d.order[i] = id d.titles[i] = t.configSets[id].title or "nil" end
	for k, v in pairs(cs.input) do d.input[k] = v end
	for k, v in pairs(cs.placeholder) do d.placeholder[k] = v end
	for i, bl in ipairs(cs.customModsList) do d.blocks[i] = { title = bl.title or "Default", enabled = bl.enabled ~= false, text = bl.text or "" } end
	local rev = { }
	for var, c in pairs(t.varControls) do rev[c] = var end
	d.sel = "nil"
	local sel = t.selControl
	if sel then
		if sel == t.controls.search then d.sel = "search"
		elseif sel == t.controls.scrollBar then d.sel = "scrollBar"
		elseif sel == t.controls.setSelect then d.sel = "setSelect"
		elseif rev[sel] then d.sel = "var:" .. rev[sel]
		else
			d.sel = "other"
			if sel._className == "CheckBoxControl" or sel._className == "EditControl" or sel._className == "DropDownControl" or sel._className == "Control" then
				for _, s in ipairs(t.sectionList) do for _, c in ipairs(s.varControlList) do if c == sel then d.sel = "var:-" end end end
			end
		end
	end
	local setList = { }
	for i, v in ipairs(t.controls.setSelect.list) do setList[i] = v end
	d.setSelect = { sel = t.controls.setSelect.selIndex, enabled = t.controls.setSelect:IsEnabled() and true or false, dropped = t.controls.setSelect.dropped and true or false, list = setList }
	for i, s in ipairs(t.sectionList) do
		local rows = { }
		for _, c in ipairs(s.varControlList) do
			if c:IsShown() then
				local cls, name = c._className, rev[c] or "-"
				local en = c:IsEnabled() and "true" or "false"
				if cls == "CheckBoxControl" then
					rows[#rows + 1] = "check|" .. name .. "|" .. rectOf(c) .. "|" .. ts(c.state and true or false) .. "|" .. en .. "|" .. borderOf(c)
				elseif cls == "EditControl" then
					rows[#rows + 1] = "edit|" .. name .. "|" .. rectOf(c) .. "|" .. c.buf .. "|" .. ts(c.placeholder or "") .. "|" .. en .. "|" .. borderOf(c)
				elseif cls == "DropDownControl" then
					rows[#rows + 1] = "list|" .. name .. "|" .. rectOf(c) .. "|" .. ts(c.selIndex) .. "|" .. en .. "|" .. ts(c.dropped and true or false) .. "|" .. borderOf(c)
				elseif cls == "CustomModBlockControl" then
					rows[#rows + 1] = "block|" .. rectOf(c) .. "|" .. c.controls.titleEdit.buf .. "|" .. ts(c.controls.enableCheck.state and true or false) .. "|" .. c.controls.textEdit.buf
				elseif c == t.controls.customModsAddBlock then
					rows[#rows + 1] = "add|" .. rectOf(c)
				else
					rows[#rows + 1] = "bare|" .. name .. "|" .. rectOf(c)
				end
			end
		end
		d.sections[i] = { label = s.label, shown = s:IsShown() and true or false, collapsed = s.collapsed and true or false, rect = s:IsShown() and rectOf(s) or "hidden", toggle = s.collapsed and "+" or "-", rows = rows }
	end
	for _, c in ipairs(t.controls) do
		if c._className == "LabelControl" and c:IsShown() then
			local x, y = c:GetPos()
			d.labels[#d.labels + 1] = ts(x) .. "," .. ts(y) .. "|" .. c:GetProperty("label")
		end
	end
	for var, c in pairs(t.varControls) do
		local l = tipLines(c)
		if #l > 0 then d.tips[var] = l end
	end
	local n = b.notesTab
	local e = n.controls.edit
	local ex, ey = e:GetPos()
	local ew, eh = e:GetSize()
	local notes = { buf = e.buf, caret = e.caret, sel = e.sel or 0, lineHeight = e.lineHeight or 0, show = n.showColorCodes and true or false, toggleLabel = n.controls.toggleColorCodes.label, modFlag = n.modFlag and true or false,
		rect = ts(ex) .. "," .. ts(ey) .. "," .. ts(ew) .. "," .. ts(eh), focus = n.selControl == e and "edit" or (n.selControl and "other" or "nil") }
	local popup = json.null
	local p = main.popups[1]
	if p then
		popup = { title = p.title, width = p.width, height = p.height }
		local pc = p.controls
		if pc[1] and pc[1].list then
			local rows = { }
			for i, v in ipairs(pc[1].list) do rows[i] = pc[1]:GetRowValue(1, i, v) end
			popup.rows, popup.selIndex = rows, pc[1].selIndex or 0
		end
		if pc.listControl then
			local l = pc.listControl
			local rows = { }
			for i = 1, math.min(40, #l.list) do rows[i] = l:GetRowValue(1, i, l.list[i]) end
			popup.rows, popup.count, popup.selIndex, popup.saveEnabled = rows, #l.list, l.selIndex or 0, pc.save:IsEnabled() and true or false
			local tl = { }
			for i, x in ipairs(l.tooltip and l.tooltip.lines or { }) do tl[i] = x.text or "<sep>" end
			-- `pairs(value.sources)`: the order of this process (R13)
			local raw = { }
			for i, x in ipairs(tl) do raw[i] = x end
			popup.tipRaw = raw
			table.sort(tl)
			popup.tip = tl
		end
		if pc.edit then popup.edit = pc.edit.buf popup.saveEnabled = pc.save and pc.save:IsEnabled() and true or false end
	end
	return d, notes, popup
end
local function center(c)
	local x, y = c:GetPos()
	local w, h = c:GetSize()
	cursorX, cursorY = math.floor(x + w / 2), math.floor(y + h / 2)
end
local function calcsCursor(st)
	local b = main.modes.BUILD
	if not b or not b.configTab then return end
	local t = b.configTab
	-- a step of its own: the next frame draws at the new offset
	if st.scrollTop then t.controls.scrollBar:SetOffset(0) end
	if st.scrollVar or st.scrollBlock then
		local c = st.scrollVar and t.varControls[st.scrollVar] or t.customModsBlockControls[st.scrollBlock]
		local _, y = c:GetPos()
		t.controls.scrollBar:SetOffset(t.controls.scrollBar.offset + y - 300)
	end
	if st.cursorVar then
		center(t.varControls[st.cursorVar])
	elseif st.cursorDrop then -- { var, 1-based row of the open list }
		local c = t.varControls[st.cursorDrop[1]]
		local x, y = c:GetPos()
		local w, h = c:GetSize()
		cursorX, cursorY = math.floor(x + w / 2), y + h + 2 + (st.cursorDrop[2] - 1) * (h - 4) + 4
	elseif st.cursorBlock then -- { 1-based block, part }
		local names = { delete = "deleteBtn", title = "titleEdit", add = "addModBtn", check = "enableCheck", text = "textEdit" }
		center(t.customModsBlockControls[st.cursorBlock[1]].controls[names[st.cursorBlock[2]]])
	elseif st.cursorSection then
		for _, s in ipairs(t.sectionList) do
			if s.label == st.cursorSection then
				for _, c in ipairs(t.controls) do
					if c._className == "ButtonControl" and c.anchor.other == s then
						local x, y = c:GetPos()
						cursorX, cursorY = x + 8, y + 8
					end
				end
			end
		end
	elseif st.cursorBuild then
		center(b.controls[st.cursorBuild])
	elseif st.cursorTab then
		center(t.controls[st.cursorTab])
	elseif st.cursorNotes then
		center(b.notesTab.controls[st.cursorNotes])
	elseif st.cursorPopup then -- a control of the top popup; a number = an array key
		center(main.popups[1].controls[tonumber(st.cursorPopup) or st.cursorPopup])
	elseif st.cursorList then -- { list's popup key, the list's own control }
		center(main.popups[1].controls[tonumber(st.cursorList[1]) or st.cursorList[1]].controls[st.cursorList[2]])
	elseif st.cursorRow then -- { list's popup key, 1-based row }
		local l = main.popups[1].controls[tonumber(st.cursorRow[1]) or st.cursorRow[1]]
		local r = l:GetRowRegion()
		local x, y = l:GetPos()
		cursorX, cursorY = x + r.x + 20, y + r.y + (st.cursorRow[2] - 1) * l.rowHeight + 8
	elseif st.cursorRel then
		cursorX, cursorY = cursorX + st.cursorRel[1], cursorY + st.cursorRel[2]
	end
end

-- 4. Scenario loop.
local names = json.decode(readAll(base .. "/scenes/config/index.json"))
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
		self.sc = { name = name, def = json.decode(readAll(base .. "/scenes/config/" .. name .. ".json"), 1, json.null), step = 0, dumps = { }, shots = { }, perf = { } }
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
		-- U3d: what TreeTab.lua:2006 asks of Data/StatDescriptions, for crates/pob-ui/tests/fixtures/legion_stat_lines.json
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
	if st.wipeCache then wipeGlobalCache() end -- a probe of the comparison tooltip (R-U7)
	if st.cursor then cursorX, cursorY = st.cursor[1], st.cursor[2] end
	calcsCursor(st)
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
		local ok, d = pcall(dump, st)
		sc.dumps[#sc.dumps + 1] = { step = sc.step, state = ok and d or ("dump error: " .. tostring(d)) }
	end
	if st.rowsOut then
		-- every row of the Mod Browser's list
		local l = main.popups[1].controls.listControl
		local rows = { }
		for i, v in ipairs(l.list) do rows[i] = l:GetRowValue(1, i, v) end
		writeAll(OUT .. st.rowsOut .. ".json", json.encode(rows) .. "\n")
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
-- U7: the state of the real window (the same fields as pob_ui::tabs::config::debug_input_state).
function runner:RealState()
	local b = main.modes.BUILD
	local t = b.configTab
	local pos = function(c) local x, y = c:GetPos() local w, h = c:GetSize() return { x = math.floor(x + w / 2), y = math.floor(y + h / 2) } end
	local i = t.configSets[t.activeConfigSetId].input
	local stat = { }
	for n, l in ipairs(b.controls.statBox.list) do stat[n] = tostring(l[1] or "nil") .. "|" .. tostring(l[2] or "nil") end
	return { ready = true, events = realEvents, configReal = { viewMode = b.viewMode, modeBtn = pos(b.controls.modeConfig), checkPos = pos(t.varControls.buffArcaneSurge), editPos = pos(t.varControls.multiplierMapModEffect),
		check = t.varControls.buffArcaneSurge.state and true or false, buf = t.varControls.multiplierMapModEffect.buf, inputCheck = i.buffArcaneSurge and true or false, inputEdit = i.multiplierMapModEffect or json.null,
		scroll = t.controls.scrollBar.offset, statBox = stat } }
end
function runner:OnExit()
end
