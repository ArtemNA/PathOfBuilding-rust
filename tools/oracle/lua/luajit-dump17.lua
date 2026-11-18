-- Origin: pob-ts tools/oracle/lua/luajit-dump17.lua at pob-ts commit 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into
-- pob-rust (R-INDEPENDENT-UPDATE A3) and owned here since; the producer of the golden snapshots (tools/oracle/snapshot.ts,
-- tools/make-goldens.ts). Lines below are the original header.
-- P2.6 oracle producer: adapted from project_15/pob-engine-wasm/tools/luajit-dump.lua (MIT).
-- Dumps mainOutput on both precision channels (%.17g exact + %.14g/dkjson,
-- mirrors tools/oracle/lua-snippets.ts's DUMP_STATS_17) plus `meta` (mirrors
-- src/lua/boot.lua's engine_outputJSON), under desktop LuaJIT in the
-- official PoB docker image (tools/oracle/luajit.ts).
--
-- Usage: luajit /bench/luajit-dump17.lua <build.xml> [<case.json>]
--
-- Optional 2nd arg: a case in tools/oracle/format.ts's caseSchema shape
-- ({ mutations: [...] }). Only `setConfig` is supported: build.configTab.
-- input[id] = value for every one (dkjson decodes JSON null to Lua nil,
-- which resets the field -- same as client.mjs's setConfig(id, null)),
-- applied all together, then one BuildModList() + wipeGlobalCache() +
-- BuildOutput() (test/config-luajit.mjs's WASM-side path). Any other op
-- aborts (exit <> 0) before anything is applied; an empty/absent case is a
-- plain load.
--
-- Prints one line of JSON to stdout (last line of output):
--   { num = {k = "<%.17g>"}, str = {}, bool = {}, nonFinite = {k = "nan"|"inf"|"-inf"},
--     num14 = {k = <raw number>}, meta = {allocNodes, items, socketGroups,
--     mainSocketGroup, treeVersion, className, ascendName} }
local buildPath, casePath = ...
assert(buildPath, "usage: luajit luajit-dump17.lua <path-to-build.xml> [<path-to-case.json>]")

dofile("HeadlessWrapper.lua")
if __mainObject__ and __mainObject__.promptMsg then
	error("FATAL boot failed: " .. tostring(__mainObject__.promptMsg))
end
if not build then
	error("FATAL: build mode not initialised")
end

local dkjson = require("dkjson")

-- C4c timeless-jewel LUTs: HeadlessWrapper's Inflate is a stub, so give
-- io.open/NewFileSearch a real handle for the .bin mounted at
-- POB_JEWEL_DATA_DIR (ensureJewelLuts) -- identical shim to luajit-dump.lua.
local jewelDataDir = os.getenv("POB_JEWEL_DATA_DIR")
local rawIoOpen = io.open
if jewelDataDir then
	io.open = function(path, mode)
		local fileName = path:match("Data/TimelessJewelData/([^/\\]+%.bin)$")
		if fileName and (not mode or mode:match("r")) then
			return rawIoOpen(jewelDataDir .. "/" .. fileName, mode)
		end
		return rawIoOpen(path, mode)
	end
end
local headlessNewFileSearch = NewFileSearch
function NewFileSearch(spec, findDirectories)
	if spec:match("Data/TimelessJewelData/.+%.bin$") then
		local file = io.open(spec, "rb")
		if file then
			file:close()
			return { GetFileName = function() return spec:match("([^/\\]+)$") end, GetFileModifiedTime = function() return 1 end }
		end
	end
	return headlessNewFileSearch(spec, findDirectories)
end

local f = assert(io.open(buildPath, "r"))
local xml = f:read("*a")
f:close()
loadBuildFromXML(xml, "dump")
if __mainObject__.promptMsg then
	error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg))
end

if casePath then
	local cf = assert(io.open(casePath, "r"), "could not open case JSON file: " .. tostring(casePath))
	local caseText = cf:read("*a")
	cf:close()
	local caseData, _, decodeErr = dkjson.decode(caseText)
	if caseData == nil then error("FATAL case JSON parse failed: " .. tostring(decodeErr)) end
	local mutations = caseData.mutations or {}
	for _, m in ipairs(mutations) do
		if m.op ~= "setConfig" then error("luajit: unsupported mutation op " .. tostring(m.op)) end
	end
	if #mutations > 0 then
		for _, m in ipairs(mutations) do build.configTab.input[m.id] = m.value end
		build.configTab:BuildModList()
		if __mainObject__.promptMsg then error("FATAL config apply failed: " .. tostring(__mainObject__.promptMsg)) end
		wipeGlobalCache()
		build.calcsTab:BuildOutput()
		if __mainObject__.promptMsg then error("FATAL recalc-after-config failed: " .. tostring(__mainObject__.promptMsg)) end
	end
end

local out = build.calcsTab.mainOutput
if not out then error("FATAL: no mainOutput after build load") end

local num, str, bool, nonFinite, num14 = {}, {}, {}, {}, {}
for k, v in pairs(out) do
	local tv = type(v)
	if tv == "number" then
		if v ~= v then nonFinite[k] = "nan"
		elseif v == math.huge then nonFinite[k] = "inf"
		elseif v == -math.huge then nonFinite[k] = "-inf"
		else
			num[k] = string.format("%.17g", v)
			num14[k] = v
		end
	elseif tv == "string" then str[k] = v
	elseif tv == "boolean" then bool[k] = v
	end
end

local nAlloc = 0
for _ in pairs(build.spec.allocNodes) do nAlloc = nAlloc + 1 end
local nItems = 0
for _ in pairs(build.itemsTab.items) do nItems = nItems + 1 end

print(dkjson.encode({
	num = num, str = str, bool = bool, nonFinite = nonFinite, num14 = num14,
	meta = {
		allocNodes = nAlloc, items = nItems, socketGroups = #build.skillsTab.socketGroupList,
		mainSocketGroup = build.mainSocketGroup, treeVersion = build.spec.treeVersion,
		className = build.spec.curClassName, ascendName = build.spec.curAscendClassName,
	},
}))
