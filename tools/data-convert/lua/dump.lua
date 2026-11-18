-- Generic lossless dump of PoB's data, run inside upstream's own LuaJIT (runtime/lua51.dll via tools/luajit-win) with the
-- working directory = <pobRoot>/src. Knows nothing about what pob-rust reads: it boots PoB headless exactly like
-- HeadlessWrapper users do and writes every value reachable from a few roots, closures included, to one binary-safe
-- stream. All interpretation (which tables, which fields, the data/*.json shapes) lives in Rust (tools/data-convert).
--
-- Env: POB_DUMP_OUT = output file (absolute). Nothing else is written: io.open for writing anywhere else is an error,
-- so the upstream checkout stays read-only.
--
-- Stream grammar (one value):
--   N                         nil (only as an upvalue value)
--   T | F                     true | false
--   #<text>;                  number: "%d" when integral and |v| < 2^53 (not -0), else "%.17g"; inf, -inf, nan, -0
--   s<len>:<bytes>            string, raw bytes (no escaping, exact)
--   {<id>;<k v>...}           table, keys sorted (numbers, strings, booleans; table keys last, by id)
--                             Ids come from a first pass that visits table-typed keys only after everything else, so
--                             they (and the whole file) do not depend on addresses or LuaJIT's hash order.
--   @<id>;                    a table or function emitted before (shared or cyclic)
--   L<id>;<src s><from>;<to>;<n>;<name s value>...   Lua function: chunk name (without "@"), line range, upvalues in order
--   C<id>;                    C function / fast function
--   U<type s>                 userdata, thread, cdata (opaque)
-- The file is a sequence of `<name s><value>` root records, written in this order:
--   data      `data` right after Modules/Data.lua returned (before anything else touches it, D2)
--   after     a table of globals after the full boot (modLib, ModFlag, ...) -- shares ids with `data`
--   modCache  Data/ModCache.lua loaded standalone (what is on disk, not what the boot parsed)
--   dataAfter `data` after the full boot, in its own id space (no `@id` into the roots above; ids keep counting up)

local OUT = assert(os.getenv("POB_DUMP_OUT"), "POB_DUMP_OUT not set")
package.path = "./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua"
package.cpath = "../runtime/?.dll"

local s_format, s_byte, t_concat, t_sort = string.format, string.byte, table.concat, table.sort
local type, next, rawget, tostring, pairs0 = type, next, rawget, tostring, pairs
local d_getinfo, d_getupvalue = debug.getinfo, debug.getupvalue
local floor, huge = math.floor, math.huge
local clock = os.clock

-- Read-only guard: the boot must not write into the upstream checkout.
local io_open = io.open
io.open = function(path, mode)
	if mode and mode:find("[wa+]") and path ~= OUT then
		error("data-dump: refusing to write '" .. tostring(path) .. "' (upstream is read-only)")
	end
	return io_open(path, mode)
end

-- Deterministic `pairs` while PoB builds `data` (MINEFIELD D13): LuaJIT's string hash order differs per process, and
-- Data.lua has last-writer-wins loops over shared tables. Same canonical order as the old pipeline (numbers, strings,
-- booleans); tables with other key types keep raw order.
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
-- Writer
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

local ids, nextId = {}, 0
local stats = { tables = 0, functions = 0, refs = 0 }
local OPAQUE = {} -- tables never walked (the global environment, package.loaded): written as U"opaque"
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
		return tostring(a) < tostring(b) -- unreachable after `assign` (every key gets an id); kept total
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

-- Pass 1: ids in a deterministic order. Table-typed keys are queued and visited after the rest of the root, so a table
-- that is both a key somewhere and a value elsewhere (gemForSkill's keys are data.skills entries) gets its id as a value.
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

-- Pass 2: write. The first occurrence of a table/function carries its definition, later ones are `@id`.
local emitted = {}
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

local function root(name, v)
	local t0 = clock()
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
-- Boot
local hookCount = 0
local orig_loadfile = loadfile
loadfile = function(name, ...)
	local f, err = orig_loadfile(name, ...)
	if f and type(name) == "string" and name:match("Modules[/\\]Data%.lua$") then
		return function(...)
			local r = { f(...) }
			hookCount = hookCount + 1
			if hookCount == 1 then root("data", data) end
			return unpack(r)
		end, err
	end
	return f, err
end

arg = arg or {} -- set by the standalone luajit executable; Main.lua reads it
local t0 = clock()
dofile("HeadlessWrapper.lua")
if __mainObject__ and __mainObject__.promptMsg then
	error("data-dump: PoB boot failed: " .. tostring(__mainObject__.promptMsg))
end
assert(hookCount == 1, "data-dump: Modules/Data.lua hook fired " .. hookCount .. " times, expected 1")
io.stderr:write(s_format("data-dump: boot %.2f s\n", clock() - t0))
pairs = pairs0

root("after", {
	modLib = modLib,
	ModFlag = ModFlag,
	KeywordFlag = KeywordFlag,
	SkillType = SkillType,
	colorCodes = colorCodes,
	defaultColorCodes = defaultColorCodes,
	latestTreeVersion = latestTreeVersion,
	treeVersionList = treeVersionList,
	launch = { versionNumber = launch and launch.versionNumber },
})
root("modCache", assert(orig_loadfile("Data/ModCache.lua"))())
-- `data` again, as the full boot left it (loading the latest tree adds tree-dependent uniques, MINEFIELD D7). A fresh id
-- space: every table is written with its post-boot content instead of an `@id` to the pre-boot one written above.
ids, emitted = {}, {}
root("dataAfter", data)
file:close()
return s_format("ok %d tables, %d functions, %d refs", stats.tables, stats.functions, stats.refs)
