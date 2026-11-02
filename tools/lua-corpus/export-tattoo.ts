// R-tattoo: data patch `data-patches/tattoo-passives.json` -- the RAW `Data/TattooPassives.lua` table (what PassiveTree.lua:80
// `require`s before PassiveTree.lua:758-786 processes it) taken from LuaJIT (docker, PoB boot). pob-ts has no such data file. The table is
// read with a fresh `loadfile` -- `require` would hand back the copy the booted tree already processed (types, split `sd`, modLists). The
// processing itself is ported in Rust (pob-tree `tattoo.rs`), not exported.
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-tattoo.ts   -> data-patches/tattoo-passives.json (deterministic)
// Shape: as export-legion.ts -- a Lua table whose keys are exactly 1..n -> JSON array; string keys only -> JSON object (keys sorted); an empty
// table -> []; sparse integer keys only -> object keyed by the decimal integer; anything else is an error. Numbers travel as `%.17g`
// strings and are written as the shortest JS repr (exact round trip); inf/nan/-0 are an error (none expected).
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
const OUT = resolve(ROOT, 'data-patches/tattoo-passives.json')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const roots = resolveRoots()

const LUA = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local raw = assert(loadfile("Data/TattooPassives.lua"))()
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
return { jit = jit.version, bad = bad, tattoo = enc(raw, "") }
`

const res = luajitEval<{ jit: string; bad: string[]; tattoo: Json }>(LUA, { roots })
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
const tattoo = dec(res.tattoo) as { [k: string]: Json }
const bytes = (s: string): Buffer => Buffer.from(s, 'utf8')
const sortKeys = (o: { [k: string]: Json }): string[] => Object.keys(o).sort((a, b) => Buffer.compare(bytes(a), bytes(b)))
const canon = (v: Json): string => {
  if (Array.isArray(v)) return `[${v.map(canon).join(',')}]`
  if (v !== null && typeof v === 'object') {
    const o = v as { [k: string]: Json }
    return `{${sortKeys(o).map((k) => `${JSON.stringify(k)}:${canon(o[k]!)}`).join(',')}}`
  }
  if (typeof v === 'number' && !Number.isFinite(v)) throw new Error('non-finite number')
  return JSON.stringify(v)
}

const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8')) as { manifestSha256: string; pobSha: string }
const producer = { tool: 'tools/lua-corpus/export-tattoo.ts', luajit: res.jit, pobSha: lock.pobSha, source: 'Data/TattooPassives.lua' }
const parts = ['groups', 'nodes']
if (JSON.stringify(Object.keys(tattoo).sort()) !== JSON.stringify(parts)) throw new Error(`top-level keys ${Object.keys(tattoo)}`)
const nodes = tattoo.nodes as { [k: string]: Json }
if (Array.isArray(nodes)) throw new Error('nodes is an array')
// one node per line (keys byte-sorted: the order Rust keeps -- `pairs(tattoo.nodes)` over string keys is R13, see pob-tree tattoo.rs)
const text =
  `{"format":1,"producer":${canon(producer)},"manifestSha256":"${lock.manifestSha256}","pobSha":"${lock.pobSha}",\n` +
  `"groups":${canon(tattoo.groups!)},\n` +
  `"nodes":{\n${sortKeys(nodes).map((k) => `${JSON.stringify(k)}:${canon(nodes[k]!)}`).join(',\n')}\n}\n}\n`
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, text, 'utf8')

type Node = { [k: string]: Json }
const list = Object.values(nodes) as Node[]
const count = (f: (n: Node) => boolean): number => list.filter(f).length
const keys = new Set<string>()
for (const n of list) for (const k of Object.keys(n)) keys.add(k)
console.log(`LuaJIT ${res.jit}: nodes ${list.length} (name ${count((n) => n.name !== undefined)}, reminderText ${count((n) => n.reminderText !== undefined)}, ks ${count((n) => n.ks === true)}, m ${count((n) => n.m === true)}, not ${count((n) => n.not === true)})`)
console.log(`node keys: ${[...keys].sort().join(' ')}`)
console.log(`sha256 ${createHash('sha256').update(text).digest('hex')}  ${OUT}  (${Buffer.byteLength(text)} bytes)`)
