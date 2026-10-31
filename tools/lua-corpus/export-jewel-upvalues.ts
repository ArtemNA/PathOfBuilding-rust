// R5.5b0: data patch for the upvalues of jewel closures the pob-ts converter did not write. modparser-dump.lua (pob-ts) records the
// upvalues of every closure VALUE in the 19 ModParser tables, but a function that is itself an upvalue (or sits in a table that is
// an upvalue) is written as a bare `{fn}` -- one level only. R5.4 finding 3: the 11 getSimpleConv visitors (ModParser.lua:19, the `v`
// of the jewelOtherFuncs wrapper :6555) and the 2 getThreshold visitors (:6457) inside the :6582 list lost srcList/dst/type/remove/
// factor/srcType and attrib/baseMod/ipairs/value. LuaJIT (docker, PoB boot) walks the REAL tables in the converter's JSON shape,
// lists every function reached through an `up` (level >= 2) with its upvalue names, and serializes the upvalues of those in
// jewelFuncList whose definition is a visitor `visit` reads upvalues from (PATCH_LINES). Everything else that lacks upvalues is only
// counted (the jewelOtherFuncs factories are rebuilt by pob_parser::jewel::call_caps). The same run records, per patched entry, the
// result of calling the real `jewelFuncList[key].func(node, out, data)` on a synthetic scenario (goldens for the Rust test).
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-jewel-upvalues.ts  -> data-patches/mod-parser-upvalues.json
// Path/shape conventions are those of export-data-patch.ts (array indices 0-based, D3 `list`, hex-escaped non-ASCII keys).
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)

type PathEl = string | number
interface Rec { p: (PathEl | { x: string })[]; id: string; line: number; level: number; names: string[]; ser?: Json }
type Json = null | boolean | number | string | Json[] | { [k: string]: Json }
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'data-patches/mod-parser-upvalues.json')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const roots = resolveRoots()

const LUA = String.raw`
local PATCH_LINES = { [19] = true, [6457] = true }
local AUX_NAMES = { gemIdLookup = true, conquerorList = true }
local function up(fn, name) local i = 1 while true do local n, v = debug.getupvalue(fn, i) if not n then return nil end if n == name then return v end i = i + 1 end end
local inner = assert(up(modLib.parseMod, "parseMod"), "inner parseMod upvalue not found")
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function key(s) if s:find("[\128-\255]") then return { x = hex(s) } end return s end
local TABLES = { "formList", "modNameList", "modFlagList", "preFlagList", "modTagList", "specialModList", "jewelFuncList",
  "clusterJewelSkills", "unsupportedModList", "preSkillNameList", "skillNameList", "suffixTypes", "penTypes", "flagTypes",
  "dmgTypes", "regenTypes", "degenTypes", "costTypes", "baseCostTypes" }
local named, src = {}, {}
for _, n in ipairs(TABLES) do src[n] = assert(up(inner, n), n) named[src[n]] = n end

local bad, recs = {}, {}
local function copy(p, extra) local o = {} for i = 1, #p do o[i] = p[i] end if extra ~= nil then o[#o + 1] = extra end return o end
local function special(x)
  if x ~= x or x == math.huge or x == -math.huge then return true end
  return x == 0 and 1 / x < 0
end
local function looksMod(t) return type(t.name) == "string" and t.flags ~= nil and t.keywordFlags ~= nil end
local function ups(f)
  local t, i = {}, 1
  while true do
    local n, v = debug.getupvalue(f, i)
    if not n then break end
    if n ~= "_ENV" and v ~= nil then t[#t + 1] = { n = n, v = v } end
    i = i + 1
  end
  return t
end
local function shape(t)
  local nums, maxn, hashN = 0, 0, 0
  for k in pairs(t) do
    if type(k) == "number" and k >= 1 and k == math.floor(k) then nums = nums + 1 if k > maxn then maxn = k end else hashN = hashN + 1 end
  end
  return nums > 0 and nums == maxn, maxn, hashN
end

-- the converter's JSON shape of a value (functions: {fn, up} recursively, C functions {cfn}, the 19 tables {named})
local ser
local function serFn(f, d)
  local info = debug.getinfo(f, "S")
  if info.what == "C" then return { cfn = true } end
  local u = ups(f)
  local r = { fn = info.short_src .. ":" .. info.linedefined }
  if #u > 0 then
    local o = {}
    for _, e in ipairs(u) do
      if type(e.v) == "table" and named[e.v] then o[e.n] = { named = named[e.v] }
      elseif type(e.v) == "table" and AUX_NAMES[e.n] then o[e.n] = { named = e.n }
      else o[e.n] = ser(e.v, d + 1) end
    end
    r.up = o
  end
  return r
end
local function serTable(t, d)
  if named[t] then return { named = named[t] } end
  if looksMod(t) then
    local r = { name = t.name, type = ser(t.type, d + 1), flags = ser(t.flags, d + 1), keywordFlags = ser(t.keywordFlags, d + 1), tags = {} }
    if t.value ~= nil then r.value = ser(t.value, d + 1) end
    if t.source ~= nil then r.source = ser(t.source, d + 1) end
    local i = 1
    while t[i] ~= nil do r.tags[i] = ser(t[i], d + 1) i = i + 1 end
    for k in pairs(t) do
      if not (k == "name" or k == "type" or k == "value" or k == "flags" or k == "keywordFlags" or k == "source" or (type(k) == "number" and k >= 1 and k < i)) then
        bad[#bad + 1] = { why = "extra key in a mod: " .. tostring(k) }
      end
    end
    return r
  end
  local contiguous, maxn, hashN = shape(t)
  if contiguous and hashN == 0 then
    local r = {}
    for i = 1, maxn do r[i] = ser(t[i], d + 1) end
    return r
  end
  local r = {}
  if contiguous then
    r.list = {}
    for i = 1, maxn do r.list[i] = ser(t[i], d + 1) end
  end
  for k, v in pairs(t) do
    if type(k) == "string" then r[k] = ser(v, d + 1)
    elseif not (contiguous and type(k) == "number" and k >= 1 and k <= maxn) then bad[#bad + 1] = { why = "unsupported key " .. tostring(k) } end
  end
  if next(r) == nil then return {} end
  return r
end
ser = function(v, d)
  if d > 8 then bad[#bad + 1] = { why = "too deep" } return false end
  local tv = type(v)
  if tv == "number" then
    if special(v) then bad[#bad + 1] = { why = "special number" } return 0 end
    return { ["$n"] = string.format("%.17g", v) }
  elseif tv == "string" then
    if v:find("[\128-\255]") then bad[#bad + 1] = { why = "non-ASCII string" } end
    return v
  elseif tv == "boolean" then return v
  elseif tv == "function" then return serFn(v, d)
  elseif tv == "table" then return serTable(v, d)
  end
  bad[#bad + 1] = { why = "unsupported type " .. tv }
  return false
end

-- every function reached from the tables, with the JSON path it has in data/mod-parser.json; level = 1 for a value of a table,
-- +1 for each step through an upvalue
local patchKeys = {}
local stack, tstack = {}, {}
local curKey
local walk
local function onStack(s, v) for i = 1, #s do if s[i] == v then return true end end return false end
walk = function(v, p, level, inJewel)
  local t = type(v)
  if t == "function" then
    local info = debug.getinfo(v, "S")
    if info.what == "C" or onStack(stack, v) then return end
    local u = ups(v)
    local names = {}
    for i, e in ipairs(u) do names[i] = e.n end
    local rec = { p = p, id = info.short_src .. ":" .. info.linedefined, line = info.linedefined, level = level, names = names }
    if level >= 2 and inJewel and PATCH_LINES[info.linedefined] and #u > 0 then
      rec.ser = serFn(v, 0)
      patchKeys[curKey] = true
    end
    recs[#recs + 1] = rec
    if level >= (inJewel and 6 or 3) then return end
    stack[#stack + 1] = v
    for _, e in ipairs(u) do
      if type(e.v) == "function" or (type(e.v) == "table" and not named[e.v] and not AUX_NAMES[e.n]) then
        walk(e.v, copy(copy(p, "up"), e.n), level + 1, inJewel)
      end
    end
    stack[#stack] = nil
  elseif t == "table" then
    if named[v] or onStack(tstack, v) then return end
    if looksMod(v) then
      if v.value ~= nil then walk(v.value, copy(p, "value"), level, inJewel) end
      return
    end
    tstack[#tstack + 1] = v
    local contiguous, maxn, hashN = shape(v)
    if contiguous and hashN == 0 then
      for i = 1, maxn do walk(v[i], copy(p, i - 1), level, inJewel) end
    else
      if contiguous then
        local arrP = copy(p, "list")
        for i = 1, maxn do walk(v[i], copy(arrP, i - 1), level, inJewel) end
      end
      for k, val in pairs(v) do
        if type(k) == "string" then walk(val, copy(p, key(k)), level, inJewel) end
      end
    end
    tstack[#tstack] = nil
  end
end
local nParser = 0
for _, n in ipairs(TABLES) do
  for k, v in pairs(src[n]) do
    nParser = nParser + 1
    if type(k) ~= "string" then bad[#bad + 1] = { why = "non-string key in " .. n }
    else curKey = k walk(v, { "tables", n, key(k) }, 1, n == "jewelFuncList") end
  end
end

-- goldens: the real jewelFuncList[key].func(node, out, data) on a synthetic scenario
local MF, KF = ModFlag, KeywordFlag
local function M(name, ty, value, source, flags, kw, tags) return { name = name, type = ty, value = value, source = source, flags = flags or 0, keywordFlags = kw or 0, tags = tags or {} } end
local SCEN = {
  modSource = "Item:7:Jewel",
  pre = { M("Str", "BASE", 45, "Tree:0"), M("Dex", "BASE", 45, "Tree:0"), M("Int", "BASE", 45, "Tree:0") },
  nodes = {
    { type = "Normal", isTattoo = false, mods = {
      M("Str", "BASE", 10, "Tree:1"), M("Dex", "BASE", 12, "Tree:1", MF.Melee), M("Int", "BASE", 20, "Tree:1"),
      M("Life", "INC", 5, "Tree:1"), M("Life", "INC", 7, "Tree:1"), M("Life", "INC", -3, "Tree:1", 0, 0, { { type = "Condition", var = "Test" } }),
      M("EnergyShield", "INC", 7, "Tree:1"), M("Evasion", "INC", 9, "Tree:1"), M("PhysicalDamage", "INC", 10, "Tree:1"),
      M("ColdDamage", "INC", 8, "Tree:1"), M("LightningDamage", "INC", 3, "Tree:1"), M("ChaosDamage", "INC", 4, "Tree:1"),
      M("ElementalDamage", "INC", 2, "Tree:1"), M("FireDamage", "INC", 6, "Tree:1", 0, KF.Fire),
      M("FireResist", "BASE", 30, "Tree:1"), M("ColdResist", "BASE", 25, "Tree:1", 0, 0, { { type = "Condition", var = "Test" } }),
      M("LightningResist", "BASE", 15, "Tree:1"), M("ElementalResist", "BASE", 12, "Tree:1") } },
    { type = "Notable", isTattoo = true, mods = { M("Str", "BASE", 4, "Tree:2"), M("Dex", "BASE", 4, "Tree:2"), M("ColdResist", "BASE", 10, "Tree:2") } },
    { type = "Normal", isTattoo = false, mods = { M("Int", "BASE", 8, "Tree:3"), M("EnergyShield", "INC", 3, "Tree:3"), M("Life", "INC", 4, "Tree:3") } },
  },
}
local function mk(m) return modLib.createMod(m.name, m.type, m.value, m.source, m.flags, m.keywordFlags, unpack(m.tags)) end
local function runEntry(entry)
  local out = new("ModList"):ModList()
  for _, m in ipairs(SCEN.pre) do out:AddMod(mk(m)) end
  local data = { modSource = SCEN.modSource }
  for _, n in ipairs(SCEN.nodes) do
    local ml = new("ModList"):ModList()
    for _, m in ipairs(n.mods) do ml:AddMod(mk(m)) end
    entry.func({ type = n.type, isTattoo = n.isTattoo, modList = ml }, out, data)
  end
  entry.func(nil, out, data)
  local r = {}
  for i, m in ipairs(out) do r[i] = modLib.formatSourceMod(m) end
  return r
end
local keys = {}
for k in pairs(patchKeys) do keys[#keys + 1] = k end
table.sort(keys)
local goldens = {}
for _, k in ipairs(keys) do
  local ok, r = pcall(runEntry, src.jewelFuncList[k])
  goldens[#goldens + 1] = { key = k, ok = ok, out = ok and r or nil, err = (not ok) and tostring(r) or nil }
end
return { jit = jit.version, nParser = nParser, recs = recs, bad = bad, scenario = SCEN, goldens = goldens }
`

const res = luajitEval<{ jit: string; nParser: number; recs: Rec[]; bad: unknown[]; scenario: Json; goldens: { key: string; ok: boolean; out?: string[]; err?: string }[] }>(LUA, { roots })
if (res.bad.length) throw new Error(`unmappable values: ${JSON.stringify(res.bad).slice(0, 600)}`)
const dec = (e: PathEl | { x: string }): PathEl => (typeof e === 'object' ? Buffer.from(e.x, 'hex').toString('utf8') : e)
const fixNums = (v: Json): Json => {
  if (Array.isArray(v)) return v.map(fixNums)
  if (v !== null && typeof v === 'object') {
    const o = v as { [k: string]: Json }
    if (Object.keys(o).length === 1 && typeof o.$n === 'string') return Number(o.$n)
    return Object.fromEntries(Object.entries(o).map(([k, x]) => [k, fixNums(x)]))
  }
  return v
}
const canon = (v: Json): string => {
  if (Array.isArray(v)) return `[${v.map(canon).join(',')}]`
  if (v !== null && typeof v === 'object') {
    const o = v as { [k: string]: Json }
    return `{${Object.keys(o).sort().map((k) => `${JSON.stringify(k)}:${canon(o[k])}`).join(',')}}`
  }
  return JSON.stringify(v)
}

// --- compare with the converter's JSON ------------------------------------------------------------------------------
const parser = JSON.parse(readFileSync(resolve(ROOT, 'data/mod-parser.json'), 'utf8')) as Json
function resolveAt(path: PathEl[]): Json {
  let cur: Json = parser
  for (const k of path) {
    const bad = typeof k === 'number' ? !Array.isArray(cur) || k >= cur.length : typeof cur !== 'object' || cur === null || Array.isArray(cur) || !(k in cur)
    if (bad) throw new Error(`path ${JSON.stringify(path)} does not resolve at ${JSON.stringify(k)}`)
    cur = (cur as unknown as Record<PathEl, Json>)[k]
  }
  return cur
}

interface UpPatch { fn: string; path: PathEl[]; up: Json }
const patches: UpPatch[] = []
const missing: Record<string, number> = {}
const missingJewel: Record<string, number> = {}
const anomalies: string[] = []
let level1 = 0
const deep: Record<string, number> = {}
for (const r of res.recs) {
  const path = r.p.map(dec)
  if (r.level >= 3) {
    // below a level-2 function whose own upvalues are not in the data: the path cannot be resolved there
    if (r.ser) throw new Error(`${JSON.stringify(path)}: a patch target at level ${r.level} (patch its level-2 ancestor instead)`)
    if (r.names.length) deep[r.id] = (deep[r.id] ?? 0) + 1
    continue
  }
  const node = resolveAt(path) as { fn?: string; up?: Json } | null
  if (!node || typeof node !== 'object' || node.fn !== r.id) throw new Error(`${JSON.stringify(path)}: LuaJIT ${r.id}, data ${JSON.stringify(node).slice(0, 120)}`)
  const upKeys = node.up && typeof node.up === 'object' && !Array.isArray(node.up) ? Object.keys(node.up).sort() : []
  if (r.level === 1) {
    level1++
    if (JSON.stringify(upKeys) !== JSON.stringify([...r.names].sort())) anomalies.push(`${JSON.stringify(path).slice(0, 120)} ${r.id}: data up ${upKeys} / LuaJIT ${r.names}`)
    continue
  }
  if (r.names.length === 0) continue
  if (upKeys.length) throw new Error(`${JSON.stringify(path)}: level ${r.level} function already carries up`)
  const k = `${r.id} [${r.names.join(',')}]`
  missing[k] = (missing[k] ?? 0) + 1
  if (path[1] === 'jewelFuncList') missingJewel[k] = (missingJewel[k] ?? 0) + 1
  if (r.ser) {
    const s = fixNums(r.ser) as { fn: string; up: Json }
    patches.push({ fn: s.fn, path, up: s.up })
  }
}
const bytes = (s: string): Buffer => Buffer.from(s, 'utf8')
patches.sort((a, b) => Buffer.compare(bytes(JSON.stringify(a.path)), bytes(JSON.stringify(b.path))))
for (const g of res.goldens) if (!g.ok) throw new Error(`LuaJIT scenario failed for ${g.key}: ${g.err}`)

const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8')) as { manifestSha256: string; pobSha: string }
const producer = { tool: 'tools/lua-corpus/export-jewel-upvalues.ts', luajit: res.jit, pobSha: lock.pobSha }
const goldens = res.goldens.map((g) => ({
  key: g.key,
  refs: patches.filter((p) => p.path[2] === g.key).map((p) => p.path),
  out: g.out,
}))
const text =
  `{"format":1,"producer":${JSON.stringify(producer)},"manifestSha256":"${lock.manifestSha256}","patches":[\n` +
  patches.map((p) => `{"fn":${JSON.stringify(p.fn)},"path":${JSON.stringify(p.path)},"up":${canon(p.up)}}`).join(',\n') +
  `\n],"goldens":{"scenario":${canon(res.scenario)},"entries":[\n` +
  goldens.map((g) => canon(g as unknown as Json)).join(',\n') +
  '\n]}}\n'
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, text, 'utf8')
console.log(`LuaJIT walked ${res.nParser} parser table entries, ${res.recs.length} function references (level 1: ${level1}); level-1 up mismatches: ${anomalies.length}`)
for (const a of anomalies.slice(0, 10)) console.log('  anomaly', a)
console.log('level>=2 functions with upvalues missing in the data (all tables):', Object.values(missing).reduce((a, b) => a + b, 0))
console.log('  by definition:', missing)
console.log('  of them in jewelFuncList:', missingJewel)
console.log('level>=3 functions with upvalues (inside level-2 functions, not resolvable in the data):', deep)
console.log(`patches: ${patches.length} (${[...new Set(patches.map((p) => p.fn))].join(', ')}); golden entries: ${goldens.length}`)
console.log(`sha256 ${createHash('sha256').update(text).digest('hex')}  ${OUT}`)
