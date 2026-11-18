-- Origin: pob-ts tools/oracle/lua/modstore-oracle.lua at pob-ts commit 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into
-- pob-rust (R-INDEPENDENT-UPDATE A4) and owned here since; run by tools/lua-corpus/export-modstore.ts in desktop LuaJIT (docker).
-- The only change: the RAWF line (desktop LuaJIT has no compat54 `s_format` upvalue). Lines below are the original header.
-- P3.2: the ModStore micro-oracle, Lua half (MOD-CORE.md §9).
--
-- Read by tools/oracle/modstore-oracle.ts and executed inside the wasm oracle through
-- evalJSON; the TS side appends `return run(dkjson.decode([==[<payload>]==]))`, so `run`
-- is the only entry point and every store built here is local to one eval (MINEFIELD O4 --
-- nothing outside these stores is mutated, data.highPrecisionMods is only read).
--
-- Number transport, both ways (MINEFIELD L20 + O9):
--  * in: dkjson decodes JSON integers as Lua 5.4 *integers*, so every payload number is
--    turned into a float by `* 1.0` -- NOT `+ 0.0`, which erases the sign of -0.
--  * out: RAWF("%.17g", v). compat54.lua replaces string.format with a wrapper that pushes
--    every integral float through math.tointeger first, which prints -0.0 as "0"; RAWF is
--    that wrapper's own captured `s_format` upvalue, i.e. the unshimmed C printf.
-- Tags live in the array part of a mod (L9): createMod(..., unpack(tags)).

local dkjson = require("dkjson") -- boot.lua:60 keeps it local, it is not a global

local function upval(f, want)
	local i = 1
	while true do
		local n, v = debug.getupvalue(f, i)
		if not n then return nil end
		if n == want then return v end
		i = i + 1
	end
end
local RAWF = upval(string.format, "s_format") or string.format -- pob-rust: desktop LuaJIT has no compat54, string.format IS the raw printf
local HUGE = math.huge

--- One double as the exact 17-significant-digit text Number() reads back bit for bit.
local function fmt17(v)
	local f = v * 1.0
	if f ~= f then return "nan" end
	if f == HUGE then return "inf" end
	if f == -HUGE then return "-inf" end
	if f == 0 then return (1 / f < 0) and "-0" or "0" end
	return RAWF("%.17g", f)
end

--- Deep copy of the decoded payload with every number widened to a float (L20). Keys are
--- left alone: dkjson's array keys must stay integers for ipairs/#.
local function toFloat(x)
	if type(x) == "number" then return x * 1.0 end
	if type(x) ~= "table" then return x end
	local out = {}
	for k, v in pairs(x) do out[k] = toFloat(v) end
	return out
end

-- --- building -----------------------------------------------------------------

--- ModTools.lua:38-49: a nil in the source slot is not a string, so tagStart stays 1, the
--- number in slot 2 is then read as flags and slot 3 as keywordFlags -- tags start at slot 4
--- whether or not a source was given.
local function buildMod(spec)
	local tags = spec.tags
	local n = tags and #tags or 0
	return modLib.createMod(spec.name, spec.type, spec.value, spec.source,
		spec.flags or 0.0, spec.keywordFlags or 0.0, unpack(tags or {}, 1, n))
end

local function buildList(specs)
	local list = {}
	for i, spec in ipairs(specs) do list[i] = buildMod(spec) end
	return list
end

--- P3.4: the Item STUB the ItemCondition tag talks to. `FindModifierSubstring` is the reduced
--- form of Item.lua:289-312 -- any mod line that contains the substring (plain find, lowercased)
--- and is not the "<substring> modifier" line. No variants, no disabled lines, no tag exclusion
--- pattern: the real Item class arrives in P5, and the TS harness stubs it exactly the same way.
local function buildItem(spec)
	local modLines = spec.modLines or {}
	return {
		name = spec.name, type = spec.type, rarity = spec.rarity,
		corrupted = spec.corrupted, shaper = spec.shaper, elder = spec.elder,
		FindModifierSubstring = function(self, substring, itemSlotName)
			for _, line in ipairs(modLines) do
				local low = line:lower()
				if low:find(substring, 1, true) and not low:find(substring .. " modifier", 1, true) then
					return true
				end
			end
			return false
		end,
	}
end

local function buildItemList(list)
	local out = {}
	for slot, spec in pairs(list) do out[slot] = buildItem(spec) end
	return out
end

--- P3.4: `actor.activeSkillList` as ModStore.lua:320-343 reads it -- skillTypes as a set,
--- disable inside skillFlags, buffList as a list of named buffs.
local function buildActiveSkillList(list)
	local out = {}
	for i, spec in ipairs(list) do
		local types = {}
		for _, n in ipairs(spec.skillTypes or {}) do types[n] = true end
		local buffList = {}
		for _, name in ipairs(spec.buffNames or {}) do buffList[#buffList + 1] = { name = name } end
		out[i] = { skillTypes = types, skillFlags = { disable = spec.disable }, buffList = buffList, skillData = spec.skillData or {} }
	end
	return out
end

local ACTOR_KEYS = { "output", "weaponData1", "weaponData2", "minionData", "ManaEfficiency" }

local function needStore(stores, id, what)
	local store = stores[id]
	if not store then error("modstore-oracle: unknown " .. what .. " store '" .. tostring(id) .. "'") end
	return store
end

local function buildStore(spec, stores)
	local parent = spec.parent and needStore(stores, spec.parent, "parent") or nil
	local store = new(spec.kind) -- Common.lua:157: new() takes no constructor arguments
	store[spec.kind](store, parent) -- = new("ModDB"):ModDB(parent)
	for k, v in pairs(spec.conditions or {}) do store.conditions[k] = v end
	for k, v in pairs(spec.multipliers or {}) do store.multipliers[k] = v end
	local actor = spec.actor
	if actor then
		-- ModStore.lua:42 -- with a parent, self.actor IS the parent's actor table (shared).
		for _, k in ipairs(ACTOR_KEYS) do
			if actor[k] ~= nil then store.actor[k] = actor[k] end
		end
		if actor.itemList then store.actor.itemList = buildItemList(actor.itemList) end
		if actor.activeSkillList then store.actor.activeSkillList = buildActiveSkillList(actor.activeSkillList) end
		for name, id in pairs(actor.actors or {}) do
			store.actor[name] = { modDB = needStore(stores, id, "actor") }
		end
	end
	for _, m in ipairs(spec.mods or {}) do store:AddMod(buildMod(m)) end
	return store
end

-- --- mutations (ModStore.lua:56-143) -------------------------------------------

local OPS = {
	NewMod = function(store, op) store:AddMod(buildMod(op.mod)) end,
	ReplaceMod = function(store, op) -- ModStore.lua:125
		local m = buildMod(op.mod)
		if not store:ReplaceModInternal(m) then store:AddMod(m) end
	end,
	ConvertMod = function(store, op) -- ModStore.lua:138
		local m = buildMod(op.mod)
		if not store:ConvertModInternal(op.oldName, m) then store:AddMod(m) end
	end,
	ScaleAddMod = function(store, op) store:ScaleAddMod(buildMod(op.mod), op.scale, op.replace) end,
	ScaleAddList = function(store, op) store:ScaleAddList(buildList(op.mods), op.scale, op.replace) end,
	CopyList = function(store, op) store:CopyList(buildList(op.mods)) end,
	AddList = function(store, op) store:AddList(buildList(op.mods)) end,
	MergeMod = function(store, op) store:MergeMod(buildMod(op.mod), op.skipNonAdditive) end,
	AddDB = function(store, op, stores) store:AddDB(needStore(stores, op.from, "source")) end,
	SetCondition = function(store, op) store.conditions[op.var] = op.value end,
	SetMultiplier = function(store, op) store.multipliers[op.var] = op.value end,
}

-- --- serialization ------------------------------------------------------------

local ser

local function serMod(m)
	local tags = {}
	for _, tag in ipairs(m) do tags[#tags + 1] = ser(tag) end
	return { k = "mod", v = {
		name = m.name, type = m.type, value = ser(m.value),
		flags = m.flags, keywordFlags = m.keywordFlags, source = ser(m.source), tags = tags,
	} }
end

--- Any Lua value the aggregations can return. A table that carries name+flags+keywordFlags is
--- a mod (§1); anything else becomes a string-keyed map, so an array arrives as "1".."n".
ser = function(v)
	local t = type(v)
	if t == "nil" then return { k = "nil" } end
	if t == "number" then return { k = "n", v = fmt17(v) } end
	if t == "boolean" then return { k = "b", v = v } end
	if t == "string" then return { k = "s", v = v } end
	if t == "table" then
		if v.name ~= nil and v.flags ~= nil and v.keywordFlags ~= nil then return serMod(v) end
		local out = {}
		for key, val in pairs(v) do
			out[type(key) == "number" and fmt17(key) or tostring(key)] = ser(val)
		end
		return { k = "t", v = out }
	end
	return { k = "s", v = tostring(v) }
end

--- ModDB: buckets by name in byte order, mods in bucket order (empty buckets included --
--- ReplaceModInternal/ConvertModInternal leave them behind, §2.3). ModList: the array part.
local function dumpStore(store)
	local out = {}
	if store.mods then
		local names = {}
		for name in pairs(store.mods) do names[#names + 1] = name end
		table.sort(names)
		for _, name in ipairs(names) do
			local bucket = {}
			for i, m in ipairs(store.mods[name]) do bucket[fmt17(i)] = serMod(m) end
			out[name] = { k = "t", v = bucket }
		end
	else
		for i, m in ipairs(store) do out[fmt17(i)] = serMod(m) end
	end
	return { k = "t", v = out }
end

-- --- queries (ModStore.lua:145-363) --------------------------------------------

local function names(q)
	local n = q.names
	return unpack(n, 1, #n)
end

local QUERIES = {
	Sum = function(store, q, cfg) return store:Sum(q.type, cfg, names(q)) end,
	More = function(store, q, cfg) return store:More(cfg, names(q)) end,
	Flag = function(store, q, cfg) return store:Flag(cfg, names(q)) end,
	Override = function(store, q, cfg) return store:Override(cfg, names(q)) end,
	List = function(store, q, cfg) return store:List(cfg, names(q)) end,
	Max = function(store, q, cfg) return store:Max(cfg, names(q)) end,
	Min = function(store, q, cfg) return store:Min(cfg, names(q)) end,
	Tabulate = function(store, q, cfg) return store:Tabulate(q.type, cfg, names(q)) end,
	HasMod = function(store, q, cfg) return store:HasMod(q.type, cfg, names(q)) end,
	Combine = function(store, q, cfg) return store:Combine(q.type, cfg, names(q)) end,
	GetCondition = function(store, q, cfg) return store:GetCondition(q.var, cfg, q.noMod) end,
	GetMultiplier = function(store, q, cfg) return store:GetMultiplier(q.var, cfg, q.noMod) end,
	GetStat = function(store, q, cfg) return store:GetStat(q.stat, cfg) end,
	EvalMod = function(store, q, cfg)
		return store:EvalMod(buildMod(q.mod), cfg, q.globalLimits and {} or nil)
	end,
}

--- cfg as ModCfg (§8): skillTypes travels as an array of numbers and becomes the set the tag
--- branch indexes; every other field is passed through (already float, L20).
local function buildCfg(cfg)
	if not cfg then return nil end
	local out = {}
	for k, v in pairs(cfg) do out[k] = v end
	if cfg.skillTypes then
		local set = {}
		for _, n in ipairs(cfg.skillTypes) do set[n] = true end
		out.skillTypes = set
	end
	-- P3.4: cfg.item is a stub Item (with its FindModifierSubstring method); cfg.skillGem stays a
	-- plain { name, tags } table, which is all calcLib.gemIsType reads.
	if cfg.item then out.item = buildItem(cfg.item) end
	return out
end

--- One query, pcall'd: where upstream throws (§2.4 source filter on a mod without source,
--- StatThreshold.statList, ...) the case records the error instead of dying (L18 bug-compat).
local function runQuery(q, stores)
	local ok, res = pcall(function()
		local store = needStore(stores, q.store, "query")
		if q.method == "Dump" then return dumpStore(store) end
		local fn = QUERIES[q.method]
		if not fn then error("modstore-oracle: unknown query method '" .. tostring(q.method) .. "'") end
		return ser(fn(store, q, buildCfg(q.cfg)))
	end)
	if ok then return res end
	-- a non-string error object would stringify to an address and break determinism
	return { k = "error", v = type(res) == "string" and res or ("non-string error: " .. type(res)) }
end

--- Entry point: stores in order (a parent must be defined before its children), then ops in
--- order, then queries. Ops are deliberately NOT pcall'd -- a broken case must fail loudly.
local function run(payload)
	payload = toFloat(payload)
	if payload.format ~= 1 then
		error("modstore-oracle: unsupported case format " .. tostring(payload.format))
	end
	local stores = {}
	for _, spec in ipairs(payload.stores) do stores[spec.id] = buildStore(spec, stores) end
	for _, op in ipairs(payload.ops or {}) do
		local fn = OPS[op.op]
		if not fn then error("modstore-oracle: unknown op '" .. tostring(op.op) .. "'") end
		fn(needStore(stores, op.store, "op"), op, stores)
	end
	local results = {}
	for i, q in ipairs(payload.queries) do results[i] = runQuery(q, stores) end
	return { results = results }
end
