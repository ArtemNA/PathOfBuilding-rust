#@ SimpleGraphic
-- R-abyss-conquest: LuaJIT truth for the Abyss conquests of BuildAllDependsAndPaths (PassiveSpec.lua:1133-1150, :1277-1293), desktop lua51.dll (UCRT libm, MINEFIELD-RUST R2),
-- the whole original PoB against synthetic builds: a public golden with Abyss jewels in its tree sockets (scenes/tree/abyss-synth.json, no personal data).
--   pwsh tools/sg-oracle/run.ps1 -Script abyss-probe.lua -Timeout 300 -> fixtures/abyss/abyss-synth.json; copy it to crates/pob-calc/tests/fixtures/abyss-synth.json
-- Per scenario: every spec node with `conqueredBy` (seed, conqueror, the components, type, dn, sd, modKey, modList in list order, keystoneMod), then wipeGlobalCache +
-- calcs.buildOutput(build, "MAIN") with every scalar of the output (`%.17g`). Harness as tree-runner.lua.
ConExecute("set vid_mode 8")
ConExecute("set vid_resizable 3")
ConExecute("set vid_last 1280,800,40,40,0")
local json = require("dkjson")
local base = GetScriptPath()
local UPSTREAM = dofile(base .. "/upstream.lua")
local POB = UPSTREAM.root .. "src/"
local POB_ROOT = UPSTREAM.root
local USER = base .. "/user"
local OUT = base .. "/out/abyss/"
local realOpen, realLines = io.open, io.lines
local function readAll(p) local f = assert(realOpen(p, "rb")) local s = f:read("*a") f:close() return s end
local function writeAll(p, s) local f = assert(realOpen(p, "wb")) f:write(s) f:close() end
local scriptArgs = select("#", ...) >= 2 and tostring(select(2, ...)) or nil
arg = { }

for _, d in ipairs({ "out", "out/abyss", "user", "user/Path of Building", "user/Path of Building/Builds" }) do
	MakeDir(base .. "/" .. d)
end
writeAll(USER .. "/Path of Building/Settings.xml", readAll(base .. "/scenes/tree/Settings.xml"))

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
function OpenURL() end
local redirect = { ["changelog.txt"] = POB_ROOT .. "changelog.txt", ["help.txt"] = POB_ROOT .. "help.txt" }
local function inCheckout(name)
	return type(name) == "string" and name:gsub("\\", "/"):lower():sub(1, #POB_ROOT) == POB_ROOT:lower()
end
function io.open(name, mode, ...)
	local src = srcRelative(name)
	if (src or inCheckout(name)) and mode and mode:match("[wa+]") then
		return nil, "write refused: " .. name
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
		error("abyss-probe: image handle metatable has no Load to redirect")
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

local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end

-- Sorted keys, so that two takes are byte-identical where the data is.
local function encode(v)
	local t = type(v)
	if t == "table" then
		if v[1] ~= nil or v.__array then
			local o = { }
			for i, x in ipairs(v) do o[i] = encode(x) end
			return "[" .. table.concat(o, ",") .. "]"
		end
		local keys = { }
		for k in pairs(v) do keys[#keys + 1] = k end
		table.sort(keys)
		local o = { }
		for i, k in ipairs(keys) do o[i] = json.encode(k) .. ":" .. encode(v[k]) end
		return "{" .. table.concat(o, ",") .. "}"
	end
	return json.encode(v)
end

-- The edit of the golden; crates/pob-calc/tests/abyss_luajit.rs does the same.
local function plainReplace(s, from, to)
	local a, b = s:find(from, 1, true)
	assert(a, "no " .. from)
	return s:sub(1, a - 1) .. to .. s:sub(b + 1)
end
local function scenarioXml(golden, sc)
	local x = readAll(POB_ROOT .. golden):gsub("\r\n", "\n")
	if sc.tree then x = plainReplace(x, 'treeVersion="3_13"', 'treeVersion="' .. sc.tree .. '"') end
	for _, j in ipairs(sc.items or { }) do
		local open = '<Item id="' .. j.id .. '"'
		local a = assert(x:find(open, 1, true), open)
		local gt = assert(x:find(">", a, true))
		local e = assert(x:find("</Item>", gt, true))
		x = x:sub(1, gt) .. "\n" .. j.text .. "\n" .. x:sub(e)
	end
	for _, s in ipairs(sc.sockets or { }) do
		x = plainReplace(x, '<Socket nodeId="' .. s.node .. '" itemId="' .. s.from .. '"/>', '<Socket nodeId="' .. s.node .. '" itemId="' .. s.to .. '"/>')
	end
	return x
end

local function dumpScenario(key)
	local build = main.modes.BUILD
	local spec = build.spec
	local fmt = modLib.formatSourceMod
	local ids = { }
	for id, n in pairs(spec.nodes) do if n.conqueredBy then ids[#ids + 1] = id end end
	table.sort(ids)
	local nodes = { __array = true }
	for i, id in ipairs(ids) do
		local n = spec.nodes[id]
		local cb = n.conqueredBy
		local sd, mods = { __array = true }, { __array = true }
		for k, l in ipairs(n.sd or { }) do sd[k] = l end
		for k, m in ipairs(n.modList) do mods[k] = fmt(m) end
		local comps = { __array = true }
		for k, c in ipairs(cb.modification or { }) do
			local rolls = { __array = true }
			for r, v in ipairs(c.rolls) do rolls[r] = RAWF("%.17g", v) end
			comps[k] = { t = c.type, id = c.id, rolls = rolls }
		end
		nodes[i] = { id = id, alloc = n.alloc and true or false, type = n.type, dn = n.dn, sd = sd, mods = mods, modKey = n.modKey, asc = n.ascendancyName, sprite = n.icon,
			ks = n.keystoneMod and fmt(n.keystoneMod) or nil,
			cb = { id = RAWF("%.17g", cb.id), type = cb.conqueror.type, cid = tostring(cb.conqueror.id), mod = cb.modification and comps or nil } }
	end
	wipeGlobalCache()
	local ok, env = pcall(build.calcsTab.calcs.buildOutput, build, "MAIN")
	local nums, strs, bools = { }, { }, { }
	if ok then
		for k, v in pairs(env.player.output) do
			local t = type(v)
			if t == "number" then nums[k] = RAWF("%.17g", v) elseif t == "string" then strs[k] = v elseif t == "boolean" then bools[k] = v end
		end
	end
	local jewelsPairs = { __array = true }
	for id in pairs(spec.jewels) do jewelsPairs[#jewelsPairs + 1] = id end
	local alloc = 0
	for _ in pairs(spec.allocNodes) do alloc = alloc + 1 end
	return { key = key, treeVersion = spec.treeVersion, alloc = alloc, jewelsPairs = jewelsPairs, nodes = nodes, calcError = (not ok) and tostring(env) or nil, output = { nums = nums, strs = strs, bools = bools } }
end

function runner:OnInit()
	local ok, err = pcall(function()
		RenderInit("DPI_AWARE")
		SetDPIScaleOverridePercent(100)
		L:OnInit()
		main.controls.checkUpdate.shown = false
		main.SetManifestBranch = function() end
		self.def = json.decode(readAll(base .. "/scenes/tree/abyss-synth.json"))
		-- -ScriptArgs <key>: that scenario alone (the `pairs(stats)` tally of MINEFIELD-RUST R13)
		if scriptArgs then
			local only = { }
			for _, sc in ipairs(self.def.scenarios) do if sc.key == scriptArgs then only[1] = sc end end
			self.def.scenarios = only
		end
		self.idx, self.frames, self.out, self.perf = 0, 0, { __array = true }, { }
	end)
	if not ok then fail("OnInit", err) end
end

function runner:Step()
	self.frames = self.frames + 1
	if launch.promptMsg then error("prompt: " .. launch.promptMsg) end
	if self.wait then
		L:OnFrame()
		self.wait = self.wait - 1
		if self.wait > 0 or main.modes.BUILD.buildFlag then return end
		assert(main.mode == "BUILD" and main.modes.BUILD.dbFileName, "not opened: " .. self.current)
		local t1 = os.clock()
		self.out[#self.out + 1] = dumpScenario(self.current)
		self.perf[#self.perf + 1] = { key = self.current, loadMs = (t1 - self.t0) * 1000, outputMs = (os.clock() - t1) * 1000 }
		self.wait = nil
		return
	end
	if self.frames < 5 or main.mode ~= "LIST" and self.idx == 0 then return L:OnFrame() end
	self.idx = self.idx + 1
	local sc = self.def.scenarios[self.idx]
	if not sc then
		writeAll(OUT .. "abyss-perf.json", json.encode(self.perf, { indent = true }))
		writeAll(OUT .. "abyss-synth.json", encode({ producer = { generator = "tools/sg-oracle/lua/abyss-probe.lua", pobSha = UPSTREAM.sha, luajit = jit.version }, golden = self.def.golden, builds = self.out }) .. "\n")
		self.finished = true
		SetDPIScaleOverridePercent(0)
		Exit()
		return
	end
	writeAll(main.buildPath .. sc.key .. ".xml", scenarioXml(self.def.golden, sc))
	self.t0 = os.clock()
	main:SetMode("BUILD", main.buildPath .. sc.key .. ".xml", sc.key)
	self.current = sc.key
	self.wait = 4
	L:OnFrame()
end

function runner:OnFrame()
	if self.finished then return end
	local ok, err = pcall(self.Step, self)
	if not ok then fail("OnFrame", err) end
end

function runner:OnKeyDown() end
function runner:OnKeyUp() end
function runner:OnChar() end
function runner:OnExit() end
