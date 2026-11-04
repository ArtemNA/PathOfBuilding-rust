#@ SimpleGraphic
-- U8d gate (Trade and Buy similar; the frame of import-runner.lua): runs the WHOLE original PoB of the checkout in the real SimpleGraphic, read in place (a relative write is a Lua error), against a TEST
-- build folder in the sandbox, and records state + screenshots for scripted scenarios (sg/scenes/trade/<name>.json).
-- No network: `LaunchSubScript`, `launch:DownloadPage` and `lcurl.safe` are replaced here: what a scene's "net" says is what comes back, on the `netDeliver` steps. `netReal` in every dump counts the attempts to reach curl.
--   pwsh tools/sg-oracle/run.ps1 -Script trade-runner.lua [-ScriptArgs <scenario,...>] -Timeout 300
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
local OUT = base .. "/out/trade/"
local shotDir = GetRuntimePath() .. "/SimpleGraphic/Screenshots/"
local realOpen, realLines = io.open, io.lines
local function readAll(p) local f = assert(realOpen(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(realOpen(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

for _, d in ipairs({ "out", "out/trade", "user", "user/Path of Building", "user/Path of Building/Builds" }) do
	MakeDir(base .. "/" .. d)
end
-- Every scene starts from the same files (a scene saves).
local function writeBuilds()
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
-- The fake transport (the same list as pob_net::ScriptedTransport in crates/pob-ui/tests/import.rs): a URL with several replies gives them in turn, the last one stays.
local netReplies, netQueue, netLog, netReal, netNextId = { }, { }, { }, 0, 1000
-- Nothing in the original may reach curl (TradeQueryGenerator.lua:381 with no QueryMods.lua; the sub-scripts have their own state and are replaced above).
package.loaded["lcurl.safe"] = setmetatable({ }, { __index = function(_, k)
	return function() netReal = netReal + 1 error("trade-runner: lcurl." .. tostring(k) .. " was reached") end
end })
local function netNormal(b)
	if not b then return "" end
	return (b:gsub("code_verifier=[^&]*", "code_verifier=*"))
end
local function netNext(url)
	local q = netReplies[url]
	if not q then
		for u, l in pairs(netReplies) do
			if u:sub(-1) == "*" and url:sub(1, #u - 1) == u:sub(1, -2) then q = l end
		end
	end
	if not q then return { error = "no recorded reply for " .. url } end
	if #q > 1 then return table.remove(q, 1) end
	return q[1]
end
local function netRecord(url, header, body)
	netLog[#netLog + 1] = url .. "|" .. (header or "") .. "|" .. netNormal(body)
end
-- `LaunchSubScript`: the sub-scripts other than `DownloadPage`, told apart by their text.
function LaunchSubScript(script, funcList, subList, ...)
	local a = { ... }
	netNextId = netNextId + 1
	local id = netNextId
	if script:find("Server started", 1, true) then
		netQueue[#netQueue + 1] = { kind = "oauth", id = id, url = a[1] }
	elseif script:find("OPT_POSTFIELDS", 1, true) then
		local url, fields = script:match("setopt_url%('([^']*)'%)"), script:match("OPT_POSTFIELDS, '([^']*)'")
		local body = (fields or "") .. a[1]
		netRecord(url, nil, body)
		netQueue[#netQueue + 1] = { kind = "post", id = id, url = url, archives = script:find("return page, res", 1, true) ~= nil }
	elseif script:find("INFO_REDIRECT_URL", 1, true) then
		netRecord(a[1], nil, nil)
		netQueue[#netQueue + 1] = { kind = "redirect", id = id, url = a[1] }
	else
		error("trade-runner: an unknown sub-script")
	end
	return id
end
local realGetTime, realIsKeyDown = GetTime, IsKeyDown
local cursorX, cursorY, nowMs, held = -100, -100, 0, { }
function GetCursorPos() return cursorX, cursorY end
function GetTime() return nowMs end
function IsKeyDown(k) return held[k] == true end
local clipboard, opened = nil, { }
function Copy(text) clipboard = text end
function Paste() return clipboard end
function OpenURL(url) opened[#opened + 1] = url end
local redirect = { ["changelog.txt"] = POB_ROOT .. "changelog.txt", ["help.txt"] = POB_ROOT .. "help.txt", ["LaunchServer.lua"] = POB .. "LaunchServer.lua" }
local writesRefused = { }
local function inCheckout(name)
	return type(name) == "string" and name:gsub("\\", "/"):lower():sub(1, #POB_ROOT) == POB_ROOT:lower()
end
function io.open(name, mode, ...)
	local src = srcRelative(name)
	local writing = mode and mode:match("[wa+]")
	if (src or inCheckout(name)) and writing then
		writesRefused[#writesRefused + 1] = name
		return nil, "write refused by trade-runner: " .. name
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
		error("trade-runner: image handle metatable has no Load to redirect")
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
-- Launch.lua:259-324 with the reply of the scene: what the script does with `perform`'s result, on `netDeliver`.
L.DownloadPage = function(self, url, callback, params)
	params = params or { }
	netRecord(url, params.header, params.body)
	netQueue[#netQueue + 1] = { kind = "page", url = url, callback = callback }
end
local osTimeReal, osTimeFixed = os.time, nil
function os.time(...)
	if osTimeFixed and select("#", ...) == 0 then return osTimeFixed end
	return osTimeReal(...)
end
local function netDeliverAll()
	local q = netQueue
	netQueue = { }
	for _, e in ipairs(q) do
		if e.kind == "page" then
			local r = netNext(e.url)
			local errMsg
			if r.error then errMsg = r.error
			elseif r.code ~= 200 then errMsg = "Response code: " .. r.code
			elseif #r.body == 0 then errMsg = "No data returned" end
			local cb = e.callback
			local perr = PCall(cb, { header = r.header or "", body = r.body or "" }, errMsg)
			if perr then L:ShowErrMsg("In download callback: %s", perr) end
		elseif e.kind == "post" then
			local r = netNext(e.url)
			if e.archives then L:OnSubFinished(e.id, r.body or "", r.code or 0)
			elseif r.code == 200 then L:OnSubFinished(e.id, r.body)
			else L:OnSubFinished(e.id, nil, r.body or "") end
		elseif e.kind == "redirect" then
			local r = netNext(e.url)
			local loc = r.header and r.header:match("[Ll]ocation: ([^\r\n]+)")
			if not loc or loc:match("poeurl%.com/") then L:OnSubFinished(e.id, nil, "Failed to resolve PoEURL link")
			else L:OnSubFinished(e.id, loc) end
		elseif e.kind == "oauth" then
			clipboard = e.url
			opened[#opened + 1] = e.url
			local r = netNext("oauth")
			if r.error then L:OnSubFinished(e.id, nil, r.error, nil, 0)
			else
				local o = r.oauth
				L:OnSubFinished(e.id, o.code, o.err, o.state or e.url:match("state=(%x+)"), o.port)
			end
		end
	end
end
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
-- What pob_ui::trade::TradeQuery::debug_dump gives.
local function g17(v) return string.format("%.17g", v) end
local function listCopy(l)
	local out = { }
	for i, v in ipairs(l or { }) do out[i] = v end
	return out
end
local function tradeDump(t)
	-- a dump is written at the end of the scene: what it holds must be a copy of now
	local d = { pbRealm = t.pbRealm, pbLeague = t.pbLeague, realmList = listCopy(t.realmDropList), leagueList = listCopy(t.itemsTab.leagueDropList), total = t:GetTotalPriceString() }
	d.results, d.sorted, d.itemIndex, d.lastQueries, d.rates, d.rowLists = { }, { }, { }, { }, { }, { }
	for row, list in pairs(t.resultTbl) do
		local items = { }
		for i, e in ipairs(list) do
			local ev
			if e.evaluation then
				ev = { }
				for k, x in ipairs(e.evaluation) do ev[k] = g17(x.weight) end
			end
			items[i] = { item = e.item_string, amount = e.amount, currency = e.currency, weight = e.weight, evaluation = ev }
		end
		d.results[tostring(row)] = items
	end
	for row, list in pairs(t.sortedResultTbl) do
		local l = { }
		for i, e in ipairs(list) do l[i] = e.index end
		d.sorted[tostring(row)] = l
	end
	for row, v in pairs(t.itemIndexTbl) do d.itemIndex[tostring(row)] = v end
	for row, v in pairs(t.lastQueries) do d.lastQueries[tostring(row)] = v end
	for realm, r in pairs(t.pbCurrencyConversion) do
		local c = { }
		for k, v in pairs(r) do if k ~= "timestamp" then c[k] = v end end
		d.rates[realm] = c
	end
	if t.controls then
		for k, c in pairs(t.controls) do
			local n = k:match("^resultDropdown(%d+)$")
			if n then
				local l = { }
				for i, v in ipairs(c.list) do l[i] = v end
				d.rowLists[n] = l
			end
		end
	end
	if t.statSortSelectionList then
		d.weights = { }
		for i, s in ipairs(t.statSortSelectionList) do d.weights[i] = s.stat .. "|" .. s.label .. "|" .. (s.weightMult and g17(s.weightMult) or "nil") end
	end
	d.modWeights = { }
	if t.tradeQueryGenerator and t.tradeQueryGenerator.modWeights then
		for i, w in ipairs(t.tradeQueryGenerator.modWeights) do d.modWeights[i] = w.tradeModId .. "|" .. g17(w.weight) .. "|" .. g17(w.meanStatDiff) .. "|" .. tostring(w.invert) end
	end
	return d
end
local function dump(st)
	local d = { mode = main.mode or "nil", popups = #main.popups, prompt = launch.promptMsg or "nil", clipboard = clipboard or "nil", opened = listCopy(opened) }
	local p = main.popups[1]
	if p then
		d.popupTitle = p.title
		d.popupRect = table.concat({ p:GetPos() }, ",") .. "," .. table.concat({ p:GetSize() }, ",")
		d.popup = controlsDump(p)
	end
	local b = main.modes.BUILD
	if b.itemsTab then
		d.trade = tradeDump(b.itemsTab.tradeQuery)
		d.authorized = main.api and main.api.authToken ~= nil
		d.unsaved = b.unsaved and true or false
		d.viewMode = b.viewMode
	end
	d.netLog = { }
	for i, l in ipairs(netLog) do d.netLog[i] = l end
	d.netReal = netReal
	if st.xml then
		d.xml = (b:SaveDB("code"):gsub("\r\n", "\n"))
	end
	d.writesRefused = table.concat(writesRefused, ",")
	return d
end

local function center(c)
	local x, y = c:GetPos()
	local w, h = c:GetSize()
	cursorX, cursorY = math.floor(x + w / 2), math.floor(y + h / 2)
end
local function cursorStep(st)
	local b = main.modes.BUILD
	if st.cursorBuild then center(b.controls[st.cursorBuild])
	elseif st.cursorCompare then center(b.compareTab.controls[st.cursorCompare])
	elseif st.compareItemButton then
		-- the buttons of a compact row of the Items view (TradeHelpers.lua:440-496, CompareTab.lua:3792-3793)
		local a = st.compareItemButton
		local t = b.compareTab
		local start = 312 + t.itemsColWidth + 10 - t.controls.itemsHScrollBar.offset + 310 + 6
		local dx = a[2] == "copy" and 94 or a[2] == "buy" and 30 or 158
		cursorX, cursorY = start + dx, 236 + 4 - t.scrollY + 20 * a[1] + 10
	elseif st.cursorItems then center(b.itemsTab.controls[st.cursorItems])
	elseif st.cursorPopup then center(main.popups[1].controls[tonumber(st.cursorPopup) or st.cursorPopup])
	elseif st.cursorPopupAt then -- { control, x from its left, y from its top }
		local x, y = main.popups[1].controls[st.cursorPopupAt[1]]:GetPos()
		cursorX, cursorY = x + st.cursorPopupAt[2], y + st.cursorPopupAt[3]
	elseif st.cursorPopupDrop then -- { control, 1-based row of the open list }
		local c = main.popups[1].controls[st.cursorPopupDrop[1]]
		local x, y = c:GetPos()
		local w, h = c:GetSize()
		cursorX, cursorY = math.floor(x + w / 2), y + h + 2 + (st.cursorPopupDrop[2] - 1) * (h - 4) + 4
	end
end

local names = json.decode(readAll(base .. "/scenes/trade/index.json"))
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
		wipeTable(opened)
		-- the module's upvalues: every scene of the port is a new program
		local buy = require("Classes.CompareBuySimilar")
		buy.lastRealmIdx, buy.lastListedIndex, buy.lastLeagueByRealm = nil, nil, nil
		main.onFrameFuncs["TradeQueryGenerator"], main.onFrameFuncs["TradeQueryRequests"] = nil, nil
		writeBuilds()
		self.sc = { name = name, def = json.decode(readAll(base .. "/scenes/trade/" .. name .. ".json"), 1, json.null), step = 0, dumps = { }, shots = { }, perf = { } }
		netReplies, netQueue, netLog, osTimeFixed = { }, { }, { }, nil
		main.api = new("PoEAPI"):PoEAPI(nil, nil, nil)
		main.lastRealm, main.lastLeague, main.lastAccountName = nil, nil, nil
		wipeTable(main.gameAccounts)
		historyList = nil
		for _, r in ipairs(self.sc.def.net or { }) do
			netReplies[r.url] = netReplies[r.url] or { }
			table.insert(netReplies[r.url], r)
		end
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
		-- R13: the orders of this process that decide results
		local realmOrder = { }
		if main.modes.BUILD.itemsTab then
			for k in pairs(main.modes.BUILD.itemsTab.tradeQuery.realmIds) do realmOrder[#realmOrder + 1] = k end
		end
		local modOrder = { }
		local modData = main.modes.BUILD.itemsTab and main.modes.BUILD.itemsTab.tradeQuery.tradeQueryGenerator and main.modes.BUILD.itemsTab.tradeQuery.tradeQueryGenerator.modData
		for ty, list in pairs(modData or { }) do
			local l = { }
			for k in pairs(list) do l[#l + 1] = k end
			modOrder[ty] = l
			-- `filterMods` (TradeQueryGenerator.lua:860-871): the same insertions into a new table, in this process
			if ty == "Eater" or ty == "Exarch" then
				local filtered, f = { }, { }
				for name, mod in pairs(list) do
					if not name:match(".*PinnaclePresence$") and not name:match(".*UniquePresence$") then filtered[name] = mod end
				end
				for k in pairs(filtered) do f[#f + 1] = k end
				modOrder[ty .. ":filtered"] = f
			end
		end
		local frameOrder = { }
		for k in pairs(main.onFrameFuncs) do frameOrder[#frameOrder + 1] = k end
		writeAll(OUT .. sc.name .. ".json", json.encode({ name = sc.name, initMs = self.initMs, perf = sc.perf, dumps = sc.dumps, shots = sc.shots, errors = self.errors, realmOrder = realmOrder, modOrder = modOrder, frameOrder = frameOrder },
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
	if st.compareImport then
		local b = main.modes.BUILD
		assert(b.compareTab:ImportBuild(readAll(main.buildPath .. st.compareImport[1] .. ".xml"), st.compareImport[2]), "import failed")
		self:Frame()
		return
	end
	if st.compareScrollX then
		main.modes.BUILD.compareTab.controls.itemsHScrollBar:SetOffset(st.compareScrollX)
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
	if st.token ~= nil then
		main.api.authToken = st.token or nil
		main.lastToken = st.token or nil
	end
	if st.apiTokens then
		main.api.authToken, main.api.refreshToken, main.api.tokenExpiry = st.apiTokens.auth, st.apiTokens.refresh, st.apiTokens.expiry
		main.lastToken, main.lastRefreshToken, main.tokenExpiry = st.apiTokens.auth, st.apiTokens.refresh, st.apiTokens.expiry
	end
	if st.osTime then osTimeFixed = st.osTime end
	if st.popupDropSel then
		-- which row a name has depends on `pairs(self.realmIds)` of the process (the first league list of a realm is not the sorted one)
		local c = main.popups[1].controls[st.popupDropSel[1]]
		for i, v in ipairs(c.list) do
			if v == st.popupDropSel[2] then c:SetSel(i) break end
		end
	end
	if st.displayItem then main.modes.BUILD.itemsTab:CreateDisplayItemFromRaw(st.displayItem) end
	if st.viewMode then main.modes.BUILD.viewMode = st.viewMode end
	if st.netDeliver then netDeliverAll() end
	if st.devMode ~= nil then launch.devMode = st.devMode end
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

-- The events of the real window (scene real-input, inject-trade-real.ps1).
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
-- The state of the real window (the same fields as pob_ui::trade::debug_input_state).
function runner:RealState()
	local b = main.modes.BUILD
	local pos = function(c) local x, y = c:GetPos() local w, h = c:GetSize() return { x = math.floor(x + w / 2), y = math.floor(y + h / 2) } end
	local t = { viewMode = b.viewMode, modeItems = pos(b.controls.modeItems), price = pos(b.itemsTab.controls.priceDisplayItem), popups = #main.popups }
	local p = main.popups[1]
	if p then
		t.title = p.title
		local c = p.controls
		for _, k in ipairs({ "fetchCountEdit:fetch", "StatWeightMultipliersButton:weights", "close:close", "ListControl:list", "Slider:slider", "cancel:cancel" }) do
			local name, field = k:match("(.+):(.+)")
			if c[name] then t[field] = pos(c[name]) end
		end
		if c.fetchCountEdit then t.fetchBuf = c.fetchCountEdit.buf end
		if c.SliderValue then t.sliderValue = c.SliderValue.label end
	end
	return { ready = true, events = realEvents, tradeReal = t }
end
function runner:OnExit()
end
