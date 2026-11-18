-- Second dump entry (same stream grammar as dump.lua, read by the same Rust parser): the values that are not reachable
-- from `data` and so are not in dump.lua's roots.
--   versions        { list = treeVersionList, latest = latestTreeVersion } (GameVersions.lua, post-boot globals)
--   configOptions   Modules/ConfigOptions.lua run once more in the booted state (it reads data.* at load time; its
--                   apply/tooltipFunc closures carry chunk name "Modules/ConfigOptions.lua", like LoadModule's)
--   tree:<ver>      TreeData/<ver>/tree.lua for every treeVersionList entry, run as a plain chunk
--   tree:legion     TreeData/legion/tree-legion.lua
-- Each root gets fresh ids state for its own tables (ids keep counting up, so they stay unique in the file) and the
-- writer drops its references afterwards: one passive tree at a time is alive, not all of them.
--
-- Env: POB_DUMP_OUT = output file (absolute). Writing anywhere else is an error (the upstream checkout is read-only).
-- Kept separate from dump.lua on purpose (owned by another change); the writer below is the same code. When both are
-- merged this file's roots become extra `root(...)` calls at the end of dump.lua.

local OUT = assert(os.getenv("POB_DUMP_OUT"), "POB_DUMP_OUT not set")
package.path = "./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua"
package.cpath = "../runtime/?.dll"

local s_format, s_byte, t_concat, t_sort = string.format, string.byte, table.concat, table.sort
local type, next, rawget, tostring, pairs0 = type, next, rawget, tostring, pairs
local d_getinfo, d_getupvalue = debug.getinfo, debug.getupvalue
local floor, huge = math.floor, math.huge
local clock = os.clock

local io_open = io.open
io.open = function(path, mode)
	if mode and mode:find("[wa+]") and path ~= OUT then
		error("data-dump: refusing to write '" .. tostring(path) .. "' (upstream is read-only)")
	end
	return io_open(path, mode)
end

-- Deterministic `pairs` (MINEFIELD D13) for the whole run, as the old pipeline had it while ConfigOptions.lua loaded.
do
	local getmetatable = getmetatable
	local function rank(k)
		local tk = type(k)
		if tk == "number" then return 0 elseif tk == "string" then return 1 elseif tk == "boolean" then return 2 end
		return 3
	end
	local function less(a, b)
		local ra, rb = rank(a), rank(b)
		if ra ~= rb then return ra < rb end
		if ra == 2 then return (a and 1 or 0) < (b and 1 or 0) end
		return a < b
	end
	pairs = function(t)
		local mt = getmetatable(t)
		if mt and mt.__pairs then return pairs0(t) end
		local keys, n = {}, 0
		for k in next, t do
			if rank(k) == 3 then return pairs0(t) end
			n = n + 1
			keys[n] = k
		end
		if n > 1 then t_sort(keys, less) end
		local i = 0
		return function()
			while true do
				i = i + 1
				local k = keys[i]
				if k == nil then return nil end
				local v = rawget(t, k)
				if v ~= nil then return k, v end
			end
		end, t, nil
	end
end

------------------------------------------------------------------------------------------------------------------------
-- Writer (dump.lua's, plus a per-root reset)
local file = assert(io_open(OUT, "wb"))
local parts, np = {}, 0
local function emit(s)
	np = np + 1
	parts[np] = s
	if np >= 65536 then
		file:write(t_concat(parts, "", 1, np))
		parts, np = {}, 0
	end
end
local function flush()
	if np > 0 then file:write(t_concat(parts, "", 1, np)) end
	parts, np = {}, 0
end

local function str(s)
	emit("s" .. #s .. ":")
	emit(s)
end

local function num(v)
	if v ~= v then return emit("#nan;") end
	if v == huge then return emit("#inf;") end
	if v == -huge then return emit("#-inf;") end
	if v == 0 and 1 / v < 0 then return emit("#-0;") end
	if v == floor(v) and v > -9007199254740992 and v < 9007199254740992 then
		return emit(s_format("#%d;", v))
	end
	emit(s_format("#%.17g;", v))
end

local ids, emitted, nextId = {}, {}, 0
local stats = { tables = 0, functions = 0, refs = 0 }
local OPAQUE = {}
OPAQUE[_G] = "_G"
OPAQUE[package.loaded] = "package.loaded"

local function rank(k)
	local tk = type(k)
	if tk == "number" then return 0 elseif tk == "string" then return 1 elseif tk == "boolean" then return 2 end
	return 3
end
local function keyLess(a, b)
	local ra, rb = rank(a), rank(b)
	if ra ~= rb then return ra < rb end
	if ra == 2 then return (a and 1 or 0) < (b and 1 or 0) end
	if ra == 3 then
		local ia, ib = ids[a], ids[b]
		if ia and ib then return ia < ib end
		if ia or ib then return ia ~= nil end
		return tostring(a) < tostring(b)
	end
	return a < b
end

local function sortedKeys(t)
	local keys, n = {}, 0
	for k in next, t do
		n = n + 1
		keys[n] = k
	end
	t_sort(keys, keyLess)
	return keys, n
end

local function upvalues(f)
	local names, vals, n = {}, {}, 0
	while true do
		local name, v = d_getupvalue(f, n + 1)
		if name == nil then break end
		n = n + 1
		names[n], vals[n] = name, v
	end
	return names, vals, n
end

local deferred, nd = {}, 0
local function assign(v)
	local tv = type(v)
	if (tv ~= "table" and tv ~= "function") or ids[v] or (tv == "table" and OPAQUE[v]) then return end
	nextId = nextId + 1
	ids[v] = nextId
	if tv == "function" then
		local _, vals, n = upvalues(v)
		for i = 1, n do assign(vals[i]) end
		return
	end
	local keys, n = sortedKeys(v)
	for i = 1, n do
		local k = keys[i]
		if type(k) == "table" or type(k) == "function" then
			nd = nd + 1
			deferred[nd] = k
		end
		assign(rawget(v, k))
	end
end

local value
local function tbl(t)
	if OPAQUE[t] then
		emit("U")
		return str("opaque:" .. OPAQUE[t])
	end
	local id = ids[t]
	if emitted[t] then
		stats.refs = stats.refs + 1
		return emit("@" .. id .. ";")
	end
	emitted[t] = true
	stats.tables = stats.tables + 1
	emit("{" .. id .. ";")
	local keys, n = sortedKeys(t)
	for i = 1, n do
		local k = keys[i]
		value(k)
		value(rawget(t, k))
	end
	emit("}")
end

local function fn(f)
	local id = ids[f]
	if emitted[f] then
		stats.refs = stats.refs + 1
		return emit("@" .. id .. ";")
	end
	emitted[f] = true
	stats.functions = stats.functions + 1
	local info = d_getinfo(f, "S")
	if info.what == "C" then return emit("C" .. id .. ";") end
	emit("L" .. id .. ";")
	local src = info.source or "?"
	if s_byte(src, 1) == 64 then src = src:sub(2) end
	str(src)
	emit(info.linedefined .. ";" .. info.lastlinedefined .. ";")
	local names, vals, n = upvalues(f)
	emit(n .. ";")
	for i = 1, n do
		str(names[i])
		value(vals[i])
	end
end

value = function(v)
	local tv = type(v)
	if tv == "string" then return str(v)
	elseif tv == "number" then return num(v)
	elseif tv == "boolean" then return emit(v and "T" or "F")
	elseif tv == "table" then return tbl(v)
	elseif tv == "function" then return fn(v)
	elseif tv == "nil" then return emit("N")
	end
	emit("U")
	str(tv)
end

-- `fresh`: forget every table seen so far (ids keep counting up), so this root shares nothing with earlier ones and
-- earlier roots can be collected.
local function root(name, v, fresh)
	local t0 = clock()
	if fresh then
		ids, emitted = {}, {}
	end
	assign(v)
	local i = 1
	while i <= nd do
		assign(deferred[i])
		i = i + 1
	end
	deferred, nd = {}, 0
	str(name)
	value(v)
	flush()
	io.stderr:write(s_format("data-dump: %s in %.2f s (%d tables, %d functions, %d refs so far)\n", name, clock() - t0,
		stats.tables, stats.functions, stats.refs))
end

------------------------------------------------------------------------------------------------------------------------
-- Boot (ConfigOptions.lua needs the booted `data`)
local orig_loadfile = loadfile
arg = arg or {}
local t0 = clock()
dofile("HeadlessWrapper.lua")
if __mainObject__ and __mainObject__.promptMsg then
	error("data-dump: PoB boot failed: " .. tostring(__mainObject__.promptMsg))
end
io.stderr:write(s_format("data-dump: boot %.2f s\n", clock() - t0))

-- A data file run the way the old pipeline did: called with one fresh table; if it returns nothing, that table is it.
local function runFile(path)
	local f = assert(orig_loadfile(path))
	local a = {}
	local v = f(a)
	if v == nil then v = a end
	return v
end

assert(type(treeVersionList) == "table" and type(latestTreeVersion) == "string", "data-dump: no treeVersionList after boot")
root("versions", { list = treeVersionList, latest = latestTreeVersion }, true)
root("configOptions", runFile("Modules/ConfigOptions.lua"), true)
pairs = pairs0
for _, ver in ipairs(treeVersionList) do
	root("tree:" .. ver, runFile("TreeData/" .. ver .. "/tree.lua"), true)
	collectgarbage()
end
root("tree:legion", runFile("TreeData/legion/tree-legion.lua"), true)
file:close()
return s_format("ok %d tables, %d functions, %d refs", stats.tables, stats.functions, stats.refs)
