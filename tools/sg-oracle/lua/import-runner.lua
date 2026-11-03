#@ SimpleGraphic
-- U8a gate (the Import and Party tabs; the frame of config-runner.lua with the dumps and the cursor steps of these tabs): runs the WHOLE original PoB of the checkout in the real SimpleGraphic,
-- read in place (a relative write is a Lua error), against a TEST build folder in the sandbox, and records state + screenshots for scripted scenarios (sg/scenes/import/<name>.json).
-- No network: `LaunchSubScript` returns nil, so `launch:DownloadPage` does nothing, and no scene presses a control that would need it.
--   pwsh tools/sg-oracle/run.ps1 -Script import-runner.lua [-ScriptArgs <scenario,...>] -Timeout 300
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua")
local POB = UPSTREAM.root .. "src/"
local POB_ROOT = UPSTREAM.root
local BUILDS = "../../../../builds/pc/3.29/" -- cwd = runtime/sg
local FIXTURE = "../../../../crates/pob-calc/tests/fixtures/party-synth/"
local USER = base .. "/user"
local OUT = base .. "/out/import/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local realOpen, realLines = io.open, io.lines
local function readAll(p) local f = assert(realOpen(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(realOpen(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

for _, d in ipairs({ "out", "out/import", "user", "user/Path of Building", "user/Path of Building/Builds" }) do
	MakeDir(base .. "/" .. d)
end
local LOCAL_BUILDS = dofile(base .. "/local-builds.lua") -- personal builds, not in git
-- Every scene starts from the same files (a scene saves).
local function writeBuilds()
	for _, b in ipairs(LOCAL_BUILDS.import or { }) do
		local src = BUILDS .. b .. ".xml"
		local f = realOpen(src, "rb")
		if f then f:close() else src = "../../fixtures/import/local-builds/" .. b .. ".xml" end
		writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(src))
	end
	for _, b in ipairs({ "Generals Perforate Zerker", "Dual Savior", "Dual Wield Cospris CoC", "Mirage Archer Toxic Rain", "OccVortex" }) do
		writeAll(USER .. "/Path of Building/Builds/" .. b .. ".xml", readAll(POB_ROOT .. "spec/TestBuilds/3.13/" .. b .. ".xml"))
	end
	writeAll(USER .. "/Path of Building/Builds/party-attack-full.xml", readAll(FIXTURE .. "party-attack-full.xml"))
end
writeBuilds()
writeAll(USER .. "/Path of Building/Settings.xml", readAll(base .. "/scenes/tree/Settings.xml"))

-- Environment: the checkout's src, read in place.
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
		writesRefused[#writesRefused + 1] = name
		return nil, "write refused by import-runner: " .. name
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
		error("import-runner: image handle metatable has no Load to redirect")
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

local function ts(v) return tostring(v) end
-- Every control of a tab (the same fields as pob_ui::tabs::import::dump_host).
local function controlsDump(tab)
	local out = { }
	for k, c in pairs(tab.controls) do
		local shown = c:IsShown() and true or false
		local o = { shown = shown, enabled = c:IsEnabled() and true or false }
		if shown then
			local x, y = c:GetPos()
			local w, h = c:GetSize()
			o.rect = ts(x) .. "," .. ts(y) .. "," .. ts(w) .. "," .. ts(h)
		end
		local cls = c._className
		if cls == "LabelControl" or cls == "ButtonControl" or cls == "SectionControl" then o.value = c:GetProperty("label")
		elseif cls == "EditControl" then o.value = c.buf
		elseif cls == "CheckBoxControl" then o.value = c.state and true or false
		elseif cls == "DropDownControl" then o.value = c.selIndex end
		out[tostring(k)] = o
	end
	return out
end
-- A code's XML (the two sides' zlib streams differ).
local function xmlOf(code)
	if not code or code == "" then return nil end
	local x = Inflate(common.base64.decode(code:gsub("-","+"):gsub("_","/")))
	return x and (x:gsub("\r\n", "\n")) or code
end
local function importDump(t)
	local list = { }
	for i, v in ipairs(t.controls.siteAccountHistory.list) do list[i] = v end
	return { controls = controlsDump(t), importCodeIn = t.controls.importCodeIn.buf, importCodeDetail = t.importCodeDetail or "", importCodeValid = t.importCodeValid and true or false, importCodeSite = t.importCodeSite,
		hasXml = t.importCodeXML ~= nil, hasJson = t.importCodeJson ~= nil, generated = xmlOf(t.controls.generateCodeOut.buf), mode = t.charImportMode, status = t.charImportStatus, history = list }
end
local function partyDump(t)
	-- `notesDesc.width` wraps its label on the first call and `pairs` may reach the labels below it first (R13): the pass that counts sees it wrapped
	controlsDump(t)
	return { controls = controlsDump(t), importCodeDetail = t.importCodeDetail or "", importCodeValid = t.importCodeValid and true or false, importCodeSite = t.importCodeSite, hasXml = t.importCodeXML ~= nil, modFlag = t.modFlag and true or false }
end
local function buildDump(b)
	local lines = { }
	for i, l in ipairs(b.controls.statBox.list) do
		local u = l.underline and ((l.underline[1] and "1" or "0") .. (l.underline[2] and "1" or "0")) or "-"
		lines[i] = string.format("%s|%s|%s|%s|%s|%s", tostring(l.height), tostring(l.align or "-"), tostring(l.x or "-"), tostring(l[1] or "nil"), tostring(l[2] or "nil"), u)
	end
	return { buildName = b.buildName or "nil", dbFile = b.dbFileName and true or false, unsaved = b.unsaved and true or false, viewMode = b.viewMode, characterLevel = b.characterLevel, statBox = lines,
		importSel = b.importTab.selControl == b.importTab.controls.importCodeIn }
end
local function dump(st)
	local d = { mode = main.mode or "nil", popups = #main.popups, prompt = launch.promptMsg or "nil", clipboard = xmlOf(clipboard) or "nil" }
	local p = main.popups[1]
	if p then d.popupTitle = p.title end
	if main.modes.BUILD.importTab and main.modes.BUILD.controls.statBox then
		local b = main.modes.BUILD
		d.build = buildDump(b)
		d.import = importDump(b.importTab)
		d.party = partyDump(b.partyTab)
	end
	if st.file then
		local f = realOpen(main.buildPath .. st.file, "rb")
		d.file = f and "saved" or "missing"
		if f then writeAll(OUT .. st.fileOut, f:read("*a")) f:close() end
	end
	d.writesRefused = table.concat(writesRefused, ",")
	return d
end

local function center(c)
	local x, y = c:GetPos()
	local w, h = c:GetSize()
	cursorX, cursorY = math.floor(x + w / 2), math.floor(y + h / 2)
end
local function tabOf(n)
	local b = main.modes.BUILD
	return n == "import" and b.importTab or b.partyTab
end
local function cursorStep(st)
	local b = main.modes.BUILD
	if st.cursorBuild then center(b.controls[st.cursorBuild])
	elseif st.cursorImport then center(b.importTab.controls[st.cursorImport])
	elseif st.cursorParty then center(b.partyTab.controls[st.cursorParty])
	elseif st.cursorPopup then center(main.popups[1].controls[tonumber(st.cursorPopup) or st.cursorPopup])
	elseif st.cursorDrop then -- { tab, control, 1-based row of the open list }
		local c = tabOf(st.cursorDrop[1]).controls[st.cursorDrop[2]]
		local x, y = c:GetPos()
		local w, h = c:GetSize()
		cursorX, cursorY = math.floor(x + w / 2), y + h + 2 + (st.cursorDrop[3] - 1) * (h - 4) + 4
	end
end

local names = json.decode(readAll(base .. "/scenes/import/index.json"))
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
		clipboard = nil
		main.lastExportWebsite = nil -- the session's: every scene of the port is a new session
		writeBuilds()
		self.sc = { name = name, def = json.decode(readAll(base .. "/scenes/import/" .. name .. ".json"), 1, json.null), step = 0, dumps = { }, shots = { }, perf = { } }
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
		launch.promptMsg = nil
		launch.devMode = false
		main.api.authToken = nil
		wipeTable(main.popups)
		if main.mode == "BUILD" then main.modes.BUILD.modFlag = false main.modes.BUILD.unsaved = false main:SetMode("LIST") end
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
		if GetAsyncCount() > 0 then sc.step = sc.step - 1 end
		self:Frame()
		return
	end
	if st.openNew then
		main:SetMode("BUILD", false, "Unnamed build")
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
	if st.perf then
		local t0 = os.clock()
		for _ = 1, st.perf do L:OnFrame() end
		sc.perf[#sc.perf + 1] = { what = "frame", frames = st.perf, ms = (os.clock() - t0) * 1000 / st.perf }
		return
	end
	if st.perfGenerate then
		-- ImportTab.lua:505 by itself
		local b = main.modes.BUILD
		local t0 = os.clock()
		for _ = 1, st.perfGenerate do common.base64.encode(Deflate(b:SaveDB("code"))) end
		sc.perf[#sc.perf + 1] = { what = "generate", ms = (os.clock() - t0) * 1000 / st.perfGenerate }
		return
	end
	if st.perfImport then
		-- ImportTab.lua:665-667 by itself, on what the field holds: Init + the frame
		local t = main.modes.BUILD.importTab
		-- `Init` makes a new tab with an empty field: the text is the one of the first time
		sc.perfXml = sc.perfXml or t.importCodeXML
		local xml = assert(sc.perfXml, "perfImport: no code in the field")
		local b = main.modes.BUILD
		local t0 = os.clock()
		b:Shutdown()
		b:Init(false, "Imported build", xml, false, nil)
		b.viewMode = "IMPORT"
		self:Frame()
		sc.perf[#sc.perf + 1] = { what = "import", ms = (os.clock() - t0) * 1000 }
		return
	end
	if st.token ~= nil then
		main.api.authToken = st.token or nil
		main.lastToken = st.token or nil
	end
	if st.devMode ~= nil then launch.devMode = st.devMode end
	if st.importMode then main.modes.BUILD.importTab.charImportMode = st.importMode end
	if st.setGenerated then main.modes.BUILD.importTab.controls.generateCodeOut:SetText(st.setGenerated) end
	if st.cursor then cursorX, cursorY = st.cursor[1], st.cursor[2] end
	cursorStep(st)
	if st.clipboard then clipboard = st.clipboard end
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
	if st.shot then sc.waitShot, sc.seen = st.shot, listShots() end
end

function runner:Finish()
	self.finished = true
	SetDPIScaleOverridePercent(0)
	Exit()
end

-- The events of the real window (scene real-input, inject-import-real.ps1).
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
-- The state of the real window (the same fields as pob_ui::tabs::import::debug_input_state).
function runner:RealState()
	local b = main.modes.BUILD
	local t, p = b.importTab, b.partyTab
	local pos = function(c) local x, y = c:GetPos() local w, h = c:GetSize() return { x = math.floor(x + w / 2), y = math.floor(y + h / 2) } end
	local shown = function(c, name) if c:IsShown() then return pos(c) end return { x = 0, y = 0 } end
	return { ready = true, events = realEvents, importReal = { viewMode = b.viewMode, modeImport = pos(b.controls.modeImport), modeParty = pos(b.controls.modeParty),
		generate = shown(t.controls.generateCode), account = shown(t.controls.siteAccountName), accountBuf = t.controls.siteAccountName.buf, hasCode = #t.controls.generateCodeOut.buf > 0,
		advanced = shown(p.controls.ShowAdvanceTools), advancedState = p.controls.ShowAdvanceTools.state and true or false, auras = shown(p.controls.editAuras), aurasBuf = p.controls.editAuras.buf, modFlag = p.modFlag and true or false } }
end
function runner:OnExit()
end
