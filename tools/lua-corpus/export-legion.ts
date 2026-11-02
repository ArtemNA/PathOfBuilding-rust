// R5.5c: data patch `data-patches/legion-passives.json` -- the RAW `Data/TimelessJewelData/LegionPassives.lua` table (what
// PassiveTree.lua:79 `require`s before PassiveTree.lua:723-787 processes it) taken from LuaJIT (docker, PoB boot). pob-ts has no such
// data file (its data/tree/legion.json holds sprites only). The table is read with a fresh `loadfile` -- `require` would hand back the
// copy the booted tree already processed (types, split `sd`, modLists). The processing itself is ported in Rust (pob-tree), not exported.
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-legion.ts   -> data-patches/legion-passives.json (deterministic)
// Shape: a Lua table whose keys are exactly 1..n -> JSON array; string keys only -> JSON object (keys sorted); an empty table -> [];
// sparse integer keys only -> object keyed by the decimal integer; anything else (mixed keys, non-integral number keys) is an error. Numbers travel as `%.17g` strings (the raw lj_strfmt) and are
// written as the shortest JS repr (exact round trip, serde_json float_roundtrip); inf/nan/-0 are an error (none expected).
import '../upstream.ts'
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)

type Json = null | boolean | number | string | Json[] | { [k: string]: Json }
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'data-patches/legion-passives.json')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const roots = resolveRoots()

const LUA = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local raw = assert(loadfile("Data/TimelessJewelData/LegionPassives.lua"))()
local bad = {}
local function enc(v, path)
  local t = type(v)
  if t == "number" then
    if v ~= v or v == math.huge or v == -math.huge or (v == 0 and 1 / v < 0) then bad[#bad + 1] = path .. ": special number" end
    return { ["$n"] = RAWF("%.17g", v) }
  elseif t == "string" or t == "boolean" then
    return v
  elseif t == "table" then
    local nInt, maxn, nStr, other = 0, 0, 0, 0
    for k in pairs(v) do
      if type(k) == "number" and k >= 1 and k == math.floor(k) then nInt = nInt + 1 if k > maxn then maxn = k end
      elseif type(k) == "string" then nStr = nStr + 1
      else other = other + 1 end
    end
    if other > 0 or (nInt > 0 and nStr > 0) then bad[#bad + 1] = path .. ": mixed table" return {} end
    if nStr == 0 and nInt ~= maxn then
      -- sparse integer keys (groups[1000000000]): an object keyed by the decimal integer
      local o = {}
      for k, x in pairs(v) do o[RAWF("%d", k)] = enc(x, path .. "/" .. k) end
      return { ["$o"] = o }
    end
    if nStr == 0 then
      local a = {}
      for i = 1, maxn do a[i] = enc(v[i], path .. "/" .. i) end
      return { ["$a"] = a }
    end
    local o = {}
    for k, x in pairs(v) do o[k] = enc(x, path .. "/" .. k) end
    return { ["$o"] = o }
  end
  bad[#bad + 1] = path .. ": " .. t
  return {}
end
return { jit = jit.version, bad = bad, legion = enc(raw, "") }
`

const res = luajitEval<{ jit: string; bad: string[]; legion: Json }>(LUA, { roots })
if (res.bad.length) throw new Error(`unmappable values: ${res.bad.slice(0, 20).join('; ')}`)
// Markers: {"$n": "%.17g"} number, {"$a": [...]} array, {"$o": {...}} object (dkjson writes an empty table as []).
const dec = (v: Json): Json => {
  if (Array.isArray(v)) return v.map(dec)
  if (v !== null && typeof v === 'object') {
    const o = v as { [k: string]: Json }
    if (typeof o.$n === 'string' && Object.keys(o).length === 1) return Number(o.$n)
    if (o.$o !== undefined && Object.keys(o).length === 1) {
      if (Array.isArray(o.$o)) throw new Error('an empty $o cannot occur (nStr > 0)')
      const inner = o.$o as { [k: string]: Json }
      return Object.fromEntries(Object.keys(inner).map((k) => [k, dec(inner[k]!)]))
    }
    if (o.$a !== undefined && Object.keys(o).length === 1) {
      if (!Array.isArray(o.$a)) throw new Error(`$a is not an array: ${JSON.stringify(o.$a).slice(0, 100)}`)
      return o.$a.map(dec)
    }
    throw new Error(`unexpected object ${JSON.stringify(o).slice(0, 200)}`)
  }
  return v
}
const legion = dec(res.legion) as { [k: string]: Json }
const bytes = (s: string): Buffer => Buffer.from(s, 'utf8')
const canon = (v: Json): string => {
  if (Array.isArray(v)) return `[${v.map(canon).join(',')}]`
  if (v !== null && typeof v === 'object') {
    const o = v as { [k: string]: Json }
    const keys = Object.keys(o).sort((a, b) => Buffer.compare(bytes(a), bytes(b)))
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canon(o[k]!)}`).join(',')}}`
  }
  if (typeof v === 'number' && !Number.isFinite(v)) throw new Error('non-finite number')
  return JSON.stringify(v)
}

const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8')) as { manifestSha256: string; pobSha: string }
const producer = { tool: 'tools/lua-corpus/export-legion.ts', luajit: res.jit, pobSha: lock.pobSha, source: 'Data/TimelessJewelData/LegionPassives.lua' }
const parts = ['additions', 'groups', 'nodes']
if (JSON.stringify(Object.keys(legion).sort()) !== JSON.stringify(parts)) throw new Error(`top-level keys ${Object.keys(legion)}`)
const text =
  `{"format":1,"producer":${canon(producer)},"manifestSha256":"${lock.manifestSha256}","pobSha":"${lock.pobSha}",\n` +
  parts.map((p) => (Array.isArray(legion[p]) ? `"${p}":[\n${(legion[p] as Json[]).map(canon).join(',\n')}\n]` : `"${p}":${canon(legion[p]!)}`)).join(',\n') +
  '\n}\n'
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, text, 'utf8')

// D13 survey: `pairs(stats)` with >= 2 keys is iterated by PassiveSpec.lua:1295 (GV additions) and :1189 (abyss).
type Entry = { id: string; stats?: { [k: string]: Json }; sd?: string[]; sortedStats?: string[] }
const multi = (list: Entry[]): string[] => list.filter((e) => e.stats && Object.keys(e.stats).length >= 2).map((e) => `${e.id}(${Object.keys(e.stats!).length})`)
const adds = legion.additions as unknown as Entry[]
const nodes = legion.nodes as unknown as Entry[]
console.log(`LuaJIT ${res.jit}: additions ${adds.length}, groups ${Object.keys(legion.groups as object).length}, nodes ${nodes.length}`)
console.log(`additions with >= 2 stats: ${multi(adds).length} ${multi(adds).slice(0, 12).join(' ')}`)
console.log(`nodes with >= 2 stats: ${multi(nodes).length}`)
console.log(`sha256 ${createHash('sha256').update(text).digest('hex')}  ${OUT}  (${Buffer.byteLength(text)} bytes)`)
