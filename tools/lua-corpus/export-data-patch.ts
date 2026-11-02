// R5.2b: data patch for MINEFIELD-RUST R9. The pob-ts converter wrote math.huge/nan as JSON `null` and lost the sign of -0 in
// data/mod-cache.json and data/mod-parser.json. LuaJIT (docker, PoB boot) walks the REAL tables -- Data/ModCache.lua (as PoB loads it)
// and the 19 ModParser tables + 2 aux tables (upvalues of the inner parseMod, like pob-ts tools/convert/lua/modparser-dump.lua),
// including the upvalues of the closures kept in them -- and lists every inf/-inf/nan/-0 with its JSON path. Only the places where
// the data file does NOT already carry the exact value are written to data-patches/mod-numbers.json.
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-data-patch.ts   -> data-patches/mod-numbers.json (deterministic)
// Path: JSON keys are strings, array indices 0-based integers; mod-cache: [<line>, "mods", i, ...]; mod-parser: ["tables"|"aux", ...].
// The mod JSON shape is pob-ts tools/convert/extract/mods.ts normalizeMod (tags = contiguous array part) + mod-parser.ts convertValue
// (an array+hash Lua table keeps its array part under "list", MINEFIELD D3; a closure value is {fn, up}).
import '../upstream.ts'
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)

type PathEl = string | number
type Found = { f: 'mod-cache' | 'mod-parser'; p: (PathEl | { x: string })[]; v: string }
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'data-patches/mod-numbers.json')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const roots = resolveRoots()

const LUA = String.raw`
local dkjson = require("dkjson")
local function up(fn, name) local i = 1 while true do local n, v = debug.getupvalue(fn, i) if not n then return nil end if n == name then return v end i = i + 1 end end
local inner = assert(up(modLib.parseMod, "parseMod"), "inner parseMod upvalue not found")
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function key(s) if s:find("[\128-\255]") then return { x = hex(s) } end return s end
local TABLES = { "formList", "modNameList", "modFlagList", "preFlagList", "modTagList", "specialModList", "jewelFuncList",
  "clusterJewelSkills", "unsupportedModList", "preSkillNameList", "skillNameList", "suffixTypes", "penTypes", "flagTypes",
  "dmgTypes", "regenTypes", "degenTypes", "costTypes", "baseCostTypes" }
local named, src = {}, {}
for _, n in ipairs(TABLES) do src[n] = assert(up(inner, n), n) named[src[n]] = n end
-- aux tables (gemIdLookup, conquerorList) are upvalues of some closures stored in the 19 tables (modparser-dump.lua AUX_NAMES)
local aux, auxSet = {}, {}
local function findAux(f, seen)
  if type(f) == "table" then
    if seen[f] then return end
    seen[f] = true
    for _, v in pairs(f) do findAux(v, seen) end
  elseif type(f) == "function" then
    if seen[f] then return end
    seen[f] = true
    local i = 1
    while true do
      local n, v = debug.getupvalue(f, i)
      if not n then break end
      if (n == "gemIdLookup" or n == "conquerorList") and type(v) == "table" and not named[v] then aux[n] = v auxSet[v] = true end
      if type(v) == "function" or (type(v) == "table" and not named[v] and not auxSet[v]) then findAux(v, seen) end
      i = i + 1
    end
  end
end
local seen = {}
for _, n in ipairs(TABLES) do findAux(src[n], seen) end
assert(aux.gemIdLookup and aux.conquerorList, "aux tables not found")

local found, bad = {}, {}
local FILE
local function copy(p, extra) local o = {} for i = 1, #p do o[i] = p[i] end if extra ~= nil then o[#o + 1] = extra end return o end
local function special(x)
  if x ~= x then return "nan" elseif x == math.huge then return "inf" elseif x == -math.huge then return "-inf" end
  if x == 0 and 1 / x < 0 then return "-0" end
end
local function looksMod(t) return type(t.name) == "string" and t.flags ~= nil and t.keywordFlags ~= nil end
local function hasSpecial(v, seen2)
  seen2 = seen2 or {}
  if type(v) == "number" then return special(v) ~= nil end
  if type(v) ~= "table" or seen2[v] then return false end
  seen2[v] = true
  for k, x in pairs(v) do if hasSpecial(k, seen2) or hasSpecial(x, seen2) then return true end end
  return false
end
local walk
local function walkMod(m, p, inUp)
  for _, k in ipairs({ "value", "flags", "keywordFlags", "div", "mult" }) do
    if m[k] ~= nil then walk(m[k], copy(p, k), inUp) end
  end
  local i = 1
  while m[i] ~= nil do walk(m[i], copy(copy(p, "tags"), i - 1), inUp) i = i + 1 end
  for k, v in pairs(m) do
    if type(k) == "number" and (k >= i or k < 1 or k ~= math.floor(k)) and hasSpecial(v) then
      bad[#bad + 1] = { why = "special number in an orphan tag", p = p }
    end
  end
end
walk = function(v, p, inUp)
  local t = type(v)
  if t == "number" then
    local s = special(v)
    if s then found[#found + 1] = { f = FILE, p = p, v = s } end
  elseif t == "function" then
    if inUp then return end
    local i = 1
    while true do
      local n, u = debug.getupvalue(v, i)
      if not n then break end
      if n ~= "_ENV" and (type(u) == "number" or (type(u) == "table" and not named[u] and not auxSet[u])) then
        walk(u, copy(copy(p, "up"), n), true)
      end
      i = i + 1
    end
  elseif t == "table" then
    if looksMod(v) then return walkMod(v, p, inUp) end
    local nums, maxn, hashN = 0, 0, 0
    for k in pairs(v) do
      if type(k) == "number" and k >= 1 and k == math.floor(k) then nums = nums + 1 if k > maxn then maxn = k end else hashN = hashN + 1 end
    end
    local contiguous = nums > 0 and nums == maxn
    if contiguous and hashN == 0 then
      for i = 1, maxn do walk(v[i], copy(p, i - 1), inUp) end
    else
      if contiguous then
        local arrP = copy(p, "list")
        for i = 1, maxn do walk(v[i], copy(arrP, i - 1), inUp) end
      end
      for k, val in pairs(v) do
        local isInt = type(k) == "number" and k >= 1 and k == math.floor(k)
        if type(k) == "string" then walk(val, copy(p, key(k)), inUp)
        elseif isInt then if not contiguous then walk(val, copy(p, string.format("%d", k)), inUp) end
        elseif hasSpecial(k) or hasSpecial(val) then bad[#bad + 1] = { why = "special number under a non-string key", p = p } end
      end
    end
  end
end

FILE = "mod-cache"
local cache = dofile("Data/ModCache.lua")
local nCache = 0
for line, e in pairs(cache) do
  nCache = nCache + 1
  if type(e[1]) == "table" then for i, m in ipairs(e[1]) do walk(m, { key(line), "mods", i - 1 }, false) end end
  if hasSpecial(e[2]) then bad[#bad + 1] = { why = "special number in extra", p = { key(line) } } end
end
FILE = "mod-parser"
local nParser = 0
for _, n in ipairs(TABLES) do
  for k, v in pairs(src[n]) do
    nParser = nParser + 1
    if type(k) ~= "string" then bad[#bad + 1] = { why = "non-string key in " .. n } else walk(v, { "tables", n, key(k) }, false) end
  end
end
for n, t in pairs(aux) do for k, v in pairs(t) do if type(k) == "string" then walk(v, { "aux", n, key(k) }, false) end end end
return { jit = jit.version, nCache = nCache, nParser = nParser, found = found, bad = bad }
`

const res = luajitEval<{ jit: string; nCache: number; nParser: number; found: Found[]; bad: unknown[] }>(LUA, { roots })
if (res.bad.length) throw new Error(`unmappable positions: ${JSON.stringify(res.bad).slice(0, 600)}`)
const dec = (e: PathEl | { x: string }): PathEl => (typeof e === 'object' ? Buffer.from(e.x, 'hex').toString('utf8') : e)

// --- compare with the converter's JSON ------------------------------------------------------------------------------
const cache = JSON.parse(readFileSync(resolve(ROOT, 'data/mod-cache.json'), 'utf8')) as { line: string; mods: unknown }[]
const parser = JSON.parse(readFileSync(resolve(ROOT, 'data/mod-parser.json'), 'utf8')) as unknown
const byLine = new Map(cache.map((e) => [e.line, e]))
if (byLine.size !== cache.length) throw new Error('duplicate lines in mod-cache.json')

function resolveAt(f: Found['f'], path: PathEl[]): unknown {
  let cur: unknown
  let rest: PathEl[]
  if (f === 'mod-cache') {
    const e = byLine.get(path[0] as string)
    if (!e) throw new Error(`mod-cache: no line ${JSON.stringify(path[0])}`)
    cur = e
    rest = path.slice(1)
  } else {
    cur = parser
    rest = path
  }
  for (const k of rest) {
    const bad =
      typeof k === 'number' ? !Array.isArray(cur) || k >= cur.length : typeof cur !== 'object' || cur === null || Array.isArray(cur) || !(k in cur)
    if (bad) throw new Error(`${f}: path ${JSON.stringify(path)} does not resolve at ${JSON.stringify(k)}`)
    cur = (cur as Record<PathEl, unknown>)[k]
  }
  return cur
}

interface Patch { file: string; path: PathEl[]; value: string }
const patches: Patch[] = []
const seen = new Set<string>()
for (const x of res.found) {
  const path = x.p.map(dec)
  const id = `${x.f}\u0000${JSON.stringify(path)}`
  if (seen.has(id)) throw new Error(`duplicate position ${id}`)
  seen.add(id)
  const at = resolveAt(x.f, path)
  if (x.v === '-0') {
    if (Object.is(at, -0)) continue
    if (at !== 0) throw new Error(`${x.f} ${JSON.stringify(path)}: LuaJIT -0, data ${JSON.stringify(at)}`)
  } else {
    if (typeof at === 'object' && at !== null && (at as { $num?: string }).$num === x.v) continue
    if (at !== null) throw new Error(`${x.f} ${JSON.stringify(path)}: LuaJIT ${x.v}, data ${JSON.stringify(at)}`)
  }
  patches.push({ file: `${x.f}.json`, path, value: x.v })
}
const bytes = (s: string): Buffer => Buffer.from(s, 'utf8')
patches.sort((a, b) => Buffer.compare(bytes(`${a.file}${JSON.stringify(a.path)}`), bytes(`${b.file}${JSON.stringify(b.path)}`)))

const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8')) as { manifestSha256: string; pobSha: string }
const producer = { tool: 'tools/lua-corpus/export-data-patch.ts', luajit: res.jit, pobSha: lock.pobSha }
const text =
  `{"format":1,"producer":${JSON.stringify(producer)},"manifestSha256":"${lock.manifestSha256}","patches":[\n` +
  patches.map((p) => JSON.stringify(p)).join(',\n') +
  '\n]}\n'
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, text, 'utf8')
const count: Record<string, number> = {}
for (const p of patches) count[`${p.file} ${p.value}`] = (count[`${p.file} ${p.value}`] ?? 0) + 1
console.log(`LuaJIT walked ${res.nCache} cache entries, ${res.nParser} parser table entries; special numbers found: ${res.found.length}`)
console.log(`patches: ${patches.length}`, count)
console.log(`sha256 ${createHash('sha256').update(text).digest('hex')}  ${OUT}`)
