// R5.1: LuaJIT answers for ModParser.lua's INTERNAL parseMod (upvalue of modLib.parseMod, P5-ARCH 2.6) on three line corpora:
//   cache = the 23 207 lines of data/mod-cache.json, tree = stats lines of the goldens' passive trees (3_13, 3_29=latest) incl. the
//   "line .. ' ' .. next" combinations PassiveTree:ProcessStats tries (found by running ProcessStats' loop in LuaJIT itself, both with the
//   bare parser and with ModCache preloaded), item = ModCache misses of the 6 golden builds' items (pob-ts Item.parse, as oracle.test.ts).
// Per line: inner(line,1) and the wrapper logic ModParser.lua:7000-7018 over inner (no cache) + "wrapper == real Data/ModCache.lua entry".
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-modparser.ts [perf]
//   -> crates/pob-parser/tests/fixtures/modparser-NN.json  /  tools/.scratch/perf-modparser.json
// Strings are BYTE strings (JS chars < 256 = UTF-8 bytes), escaped \u00XX when non-ASCII. Numbers are {"n":"%.17g"} (flags: plain ints).
import { POB as UPSTREAM_POB } from '../upstream.ts'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { GOLDEN_BUILDS, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { loadBuildXml } = await import(`${P}/src/build/index.ts`)
const { Item } = await import(`${P}/src/items/item.ts`)
const { parseModStats, resetParseModStats } = await import(`${P}/src/mods/parseMod.ts`)
type J = null | boolean | number | string | J[] | { [k: string]: J }
type Rec = Record<string, any> // eslint-disable-line

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-parser/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_450_000
const roots = resolveRoots()
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const sha = (b: string | Buffer): string => createHash('sha256').update(b).digest('hex')
const byteCmp = (a: string, b: string): number => Buffer.compare(Buffer.from(a, 'utf8'), Buffer.from(b, 'utf8'))
const bytesOf = (s: string): string => Buffer.from(s, 'utf8').toString('latin1')
const unBytes = (s: string): string => Buffer.from(s, 'latin1').toString('utf8')

// --- LuaJIT chunk: shared preamble ------------------------------------------------------------------------------
const PRE = String.raw`
local dkjson = require("dkjson")
local floor = math.floor
local function up(fn, name) local i = 1 while true do local n, v = debug.getupvalue(fn, i) if not n then return nil end if n == name then return v end i = i + 1 end end
local inner = assert(up(modLib.parseMod, "parseMod"), "inner parseMod upvalue not found")
local function fmt17(v)
  local f = v * 1.0
  if f ~= f then return "nan" elseif f == math.huge then return "inf" elseif f == -math.huge then return "-inf" end
  if f == 0 then return (1 / f < 0) and "-0" or "0" end
  return string.format("%.17g", f)
end
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function encStr(s) if s:find("[\128-\255]") then return { x = hex(s) } end return s end
local function keyname(k)
  local t = type(k)
  if t == "string" then assert(not k:find("[\128-\255]"), "non-ASCII table key") return k
  elseif t == "number" then return "#" .. fmt17(k) else return "?" .. tostring(k) end
end
local enc
enc = function(v, d)
  d = (d or 0) + 1
  assert(d < 40, "enc: too deep")
  local t = type(v)
  if t == "string" then return encStr(v) elseif t == "number" then return { n = fmt17(v) } elseif t == "boolean" then return v
  elseif t == "table" then
    local a, h, any, i = {}, {}, false, 1
    while v[i] ~= nil do a[i] = enc(v[i], d) i = i + 1 end
    for k, val in pairs(v) do
      if not (type(k) == "number" and k >= 1 and k < i and k == floor(k)) then h[keyname(k)] = enc(val, d) any = true end
    end
    local o = {}
    if i > 1 then o.a = a end
    if any then o.h = h end
    if i == 1 and not any then o.h = {} end
    return o
  end
  return { x = hex("<" .. t .. ">") }
end
local MODKEYS = { name = true, type = true, value = true, flags = true, keywordFlags = true, source = true }
local function encMod(m)
  if type(m) ~= "table" then return enc(m) end
  local o, tags, i, other, any = {}, {}, 1, {}, false
  for k in pairs(MODKEYS) do if m[k] ~= nil then o[k] = enc(m[k]) end end
  while m[i] ~= nil do tags[i] = enc(m[i]) i = i + 1 end
  o.tags = tags
  for k, val in pairs(m) do
    if not MODKEYS[k] and not (type(k) == "number" and k >= 1 and k < i and k == floor(k)) then other[keyname(k)] = enc(val) any = true end
  end
  if any then o.other = other end
  return o
end
local function encList(ml)
  if ml == nil then return nil end
  if type(ml) ~= "table" then return enc(ml) end
  local out = {}
  for i, m in ipairs(ml) do out[i] = encMod(m) end
  return out
end
local function fmtList(ml)
  if type(ml) ~= "table" then return nil end
  local out = {}
  for i, m in ipairs(ml) do
    local ok, s = pcall(modLib.formatMod, m)
    out[i] = encStr((ok and tostring(s) or ("ERROR: " .. tostring(s))):gsub("(function: )0x%x+", "%1<addr>"))
  end
  return out
end
local canon
canon = function(v)
  local t = type(v)
  if t == "nil" then return "N" elseif t == "number" then return "n" .. fmt17(v) elseif t == "string" then return "s" .. #v .. ":" .. v
  elseif t == "boolean" then return v and "T" or "F" elseif t == "table" then
    local items = {}
    for k, val in pairs(v) do items[#items + 1] = canon(k) .. "=" .. canon(val) end
    table.sort(items)
    return "{" .. table.concat(items, ",") .. "}"
  end
  return t
end
local function wrapped(line) -- ModParser.lua:7000-7018 over inner, bypassing the cache
  local ml, ex = inner(line, 1)
  if ml and ex then ml, ex = inner(line, 2) end
  return ml, ex
end
local function errmsg(e) return encStr((tostring(e):gsub("^eval:%d+: ", ""))) end
local function loadCache() return dofile("Data/ModCache.lua") end
local LINES = dkjson.decode(LINES_JSON)
`

const ANSWER_LUA = String.raw`
local CACHE = loadCache()
local res = {}
for idx, line in ipairs(LINES) do
  local r = {}
  local ok, ml, ex = pcall(inner, line, 1)
  if ok then r.i1 = { mods = encList(ml), extra = ex ~= nil and enc(ex) or nil, fm = fmtList(ml) } else r.i1 = { err = errmsg(ml) } end
  local ok2, ml2, ex2 = pcall(wrapped, line)
  if ok2 then
    r.w = { mods = encList(ml2), extra = ex2 ~= nil and enc(ex2) or nil }
    local c = CACHE[line]
    if c then
      r.ceq = canon(ml2) .. "|" .. canon(ex2) == canon(c[1]) .. "|" .. canon(c[2])
      if not r.ceq then r.cache = { mods = encList(c[1]), extra = c[2] ~= nil and enc(c[2]) or nil, fm = fmtList(c[1]) } end
    end
  else r.w = { err = errmsg(ml2) } end
  res[idx] = r
end
return { jit = jit.version, n = #LINES, res = res }
`

// ProcessStats' line loop (PassiveTree.lua:803-859) verbatim minus the node bookkeeping; collects every string handed to parseMod.
const SIM_LUA = String.raw`
local NODES = dkjson.decode(NODES_JSON)
local function simulate(parse, seen)
  for _, orig in ipairs(NODES) do
    local sd, mods = {}, {}
    for i, l in ipairs(orig) do sd[i] = l end
    local i = 1
    while sd[i] do
      if sd[i]:match("\n") then
        local line, il = sd[i], i
        table.remove(sd, i)
        for l in line:gmatch("[^\n]+") do table.insert(sd, il, l) il = il + 1 end
      end
      local line = sd[i]
      seen[line] = true
      local list, extra = parse(line)
      if not list or extra then
        local endI = i + 1
        while sd[endI] do
          local comb = line
          for ci = i + 1, endI do comb = comb .. " " .. sd[ci] end
          seen[comb] = true
          list, extra = parse(comb)
          if list and not extra then for ci = i + 1, endI do mods[ci] = true end break end
          endI = endI + 1
        end
      end
      mods[i] = true
      i = i + 1
      while mods[i] do i = i + 1 end
    end
  end
end
local function memo(f) local m = {} return function(l) local r = m[l] if not r then local ok, a, b = pcall(f, l) r = ok and { a, b } or { false } m[l] = r end return r[1] or nil, r[2] end end
local s1, s2 = {}, {}
simulate(memo(wrapped), s1)
for k, v in pairs(loadCache()) do modLib.parseModCache[k] = v end
simulate(memo(function(l) return modLib.parseMod(l, true) end), s2) -- the real wrapper, ModCache preloaded (what PoB itself runs)
local function keys(t) local o = {} for k in pairs(t) do o[#o + 1] = encStr(k) end return o end
return { sim1 = keys(s1), sim2 = keys(s2) }
`

const PERF_LUA = String.raw`
jit.on()
local CACHE = loadCache()
local good = {}
for _, l in ipairs(LINES) do if pcall(inner, l, 1) then good[#good + 1] = l end end
local function median(a) table.sort(a) return a[math.ceil(#a / 2)] end
local function bench(f, runs)
  f() collectgarbage()
  local t = {}
  for r = 1, runs do collectgarbage() local c = os.clock() f() t[r] = (os.clock() - c) * 1e6 / #good end
  return t
end
local innerUs = bench(function() for i = 1, #good do inner(good[i], 1) end end, 7)
local wrapUs = bench(function() for i = 1, #good do wrapped(good[i]) end end, 7)
for k, v in pairs(CACHE) do modLib.parseModCache[k] = v end
local hits = 0
for i = 1, #good do if CACHE[good[i]] then hits = hits + 1 end end
local cachedUs = bench(function() for i = 1, #good do modLib.parseMod(good[i]) end end, 7)
return { jit = jit.version, status = { jit.status() }, lines = #LINES, good = #good, cacheHits = hits, innerUs = innerUs, wrappedNoCacheUs = wrapUs, wrapperWithCacheUs = cachedUs,
  innerMedian = median(innerUs), wrappedMedian = median(wrapUs), cachedMedian = median(cachedUs) }
`

// --- docker transport -------------------------------------------------------------------------------------------
function luaBytes(s: string): string {
  let o = '"'
  for (const b of Buffer.from(s, 'utf8')) o += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return o + '"'
}
const asciiJson = (v: unknown): string => JSON.stringify(v).replace(/[\u007f-\uffff]/g, (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'))
const lua = (pre: string, body: string): string => `local LINES_JSON = ${luaBytes(pre)}\n${PRE}\n${body}`
const run = (code: string): Rec => luajitEval(code, { roots }) as Rec
/** Lua byte-string leaf ({x:hex} or plain) -> byte string. */
const lstr = (e: J): string => (typeof e === 'string' ? e : Buffer.from((e as Rec).x, 'hex').toString('latin1'))

// --- fixture value conversion (Lua enc -> deterministic JSON) -------------------------------------------------------
function cv(e: J): J {
  if (e === null || typeof e !== 'object') return typeof e === 'string' ? e : e
  if (Array.isArray(e)) return e.length === 0 ? {} : (() => { throw new Error('unexpected array in enc') })()
  const o = e as Rec
  if ('x' in o) return lstr(o as J)
  if ('n' in o) return { n: o.n }
  const out: Rec = {}
  if (Array.isArray(o.a)) out.a = o.a.map(cv)
  if (o.h && !Array.isArray(o.h)) out.h = Object.fromEntries(Object.entries(o.h).map(([k, v]) => [k, cv(v as J)]))
  return out
}
const intOf = (e: Rec | undefined): J => {
  if (e === undefined) return null
  const m = /^-?\d+$/.exec(e.n)
  return m && e.n !== '-0' && Number.isSafeInteger(Number(e.n)) ? Number(e.n) : { n: e.n }
}
function cvMod(m: Rec): J {
  if (!('tags' in m)) return cv(m as J)
  const o: Rec = { name: m.name === undefined ? undefined : cv(m.name), type: m.type === undefined ? undefined : cv(m.type), value: m.value === undefined ? undefined : cv(m.value),
    flags: m.flags === undefined ? undefined : intOf(m.flags), keywordFlags: m.keywordFlags === undefined ? undefined : intOf(m.keywordFlags),
    source: m.source === undefined ? undefined : cv(m.source), tags: (Array.isArray(m.tags) ? m.tags : []).map(cv) }
  if (m.other) o.other = Object.fromEntries(Object.entries(m.other).map(([k, v]) => [k, cv(v as J)]))
  return o
}
function cvSide(s: Rec | undefined, withFm: boolean): J {
  if (s === undefined) return null
  if (s.err !== undefined) return ['ERROR', lstr(s.err)]
  const o: Rec = { mods: s.mods === undefined ? null : Array.isArray(s.mods) ? s.mods.map(cvMod) : cv(s.mods), extra: s.extra === undefined ? null : cv(s.extra) }
  if (withFm && Array.isArray(s.fm) && s.fm.length) o.fm = s.fm.map(lstr)
  return o
}
function ser(v: J): string {
  if (typeof v === 'string') return JSON.stringify(v).replace(/[\u007f-\uffff]/g, (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'))
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  return `{${Object.keys(v).filter((k) => v[k] !== undefined).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)).map((k) => `${JSON.stringify(k)}:${ser(v[k]!)}`).join(',')}}`
}

// --- corpora -----------------------------------------------------------------------------------------------------
type Src = 'cache' | 'tree' | 'item'
const sources = new Map<string, Set<Src>>()
const add = (line: string, s: Src): void => { (sources.get(line) ?? sources.set(line, new Set()).get(line)!).add(s) }
const cacheJson = JSON.parse(readFileSync(resolve(ROOT, 'data/mod-cache.json'), 'utf8')) as { line: string; mods: unknown; extra: string | null }[]
const cacheLines = new Set(cacheJson.map((e) => e.line))

function treeSds(): { sds: string[][]; singles: Map<string, Set<string>> } {
  const sds: string[][] = []
  const singles = new Map<string, Set<string>>()
  for (const v of ['3_13', '3_29']) {
    const t = JSON.parse(readFileSync(resolve(ROOT, `data/tree/${v}.json`), 'utf8')) as { nodes: Record<string, Rec> }
    const set = new Set<string>()
    const effects = new Set<number>()
    const push = (stats: string[]): void => { sds.push(stats); for (const s of stats) for (const l of s.split('\n')) if (l !== '') set.add(l) }
    for (const n of Object.values(t.nodes)) {
      if (Array.isArray(n.stats) && n.stats.length) push(n.stats)
      for (const e of n.masteryEffects ?? []) if (!effects.has(e.effect)) { effects.add(e.effect); push(e.stats) } // PassiveTree.lua:557-566: first occurrence only
    }
    singles.set(v, set)
  }
  return { sds, singles }
}

function itemMisses(): Set<string> {
  const out = new Set<string>()
  for (const b of GOLDEN_BUILDS) {
    const model = loadBuildXml(readFileSync(b.xmlPath(roots), 'utf8'))
    for (const im of model.items) {
      resetParseModStats()
      Item.parse(im.raw, { id: im.id, variant: im.variant, variantAlt: im.variantAlt, variantAlt2: im.variantAlt2, variantAlt3: im.variantAlt3, variantAlt4: im.variantAlt4, variantAlt5: im.variantAlt5, modRanges: im.modRanges })
      for (const l of parseModStats().missLines) out.add(l)
    }
  }
  return out
}

// --- main ----------------------------------------------------------------------------------------------------------
function answerAll(lines: string[], batch: number, reverse: boolean): { jit: string; res: Rec[] } {
  const order = lines.map((_, i) => i)
  if (reverse) order.reverse()
  const res: Rec[] = new Array(lines.length)
  let jit = ''
  for (let i = 0; i < order.length; i += batch) {
    const idx = order.slice(i, i + batch)
    const r = run(lua(asciiJson(idx.map((k) => lines[k]!)), ANSWER_LUA))
    jit = r.jit
    if (r.n !== idx.length || r.res.length !== idx.length) throw new Error(`docker batch size mismatch ${r.n}/${r.res.length} vs ${idx.length}`)
    idx.forEach((k, j) => { res[k] = r.res[j] })
    console.log(`docker batch ${Math.floor(i / batch) + 1}/${Math.ceil(order.length / batch)}`)
  }
  return { jit, res }
}

function perf(): void {
  const lines = cacheJson.map((e) => e.line)
  const r = run(lua(asciiJson(lines), PERF_LUA))
  const out = { luajit: r.jit, jitStatus: r.status, image: LUAJIT_IMAGE, corpus: 'data/mod-cache.json lines', lines: r.lines, linesTimed: r.good, cacheHits: r.cacheHits, runs: 7,
    note: 'us/line, os.clock, jit.on(), 1 warm-up pass discarded, collectgarbage before each run; lines that raise are not timed; inner = parseMod(line,1); wrappedNoCache = ModParser.lua:7000-7018 logic over inner; wrapperWithCache = modLib.parseMod with ModCache preloaded (hit + copyTable)',
    innerUsPerLine: { median: r.innerMedian, runs: r.innerUs }, wrappedNoCacheUsPerLine: { median: r.wrappedMedian, runs: r.wrappedNoCacheUs }, wrapperWithCacheUsPerLine: { median: r.cachedMedian, runs: r.wrapperWithCacheUs } }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-modparser.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(out, null, 2))
}

if (!dockerAvailable()) throw new Error('docker not available')
if (process.argv[2] === 'perf') { perf(); process.exit(0) }

for (const l of cacheLines) add(l, 'cache')
const { sds, singles } = treeSds()
const sim = run(lua('[]', `local NODES_JSON = ${luaBytes(asciiJson(sds))}\n${SIM_LUA}`))
const treeAll = new Set<string>([...singles.values()].flatMap((s) => [...s]))
for (const k of ['sim1', 'sim2'] as const) for (const e of sim[k] as J[]) treeAll.add(unBytes(lstr(e)))
for (const l of treeAll) add(l, 'tree')
const items = itemMisses()
for (const l of items) add(l, 'item')
const lines = [...sources.keys()].sort(byteCmp)
const cnt = (f: (s: Set<Src>) => boolean): number => [...sources.values()].filter(f).length
const stats: Rec = {
  cacheLines: cacheLines.size, treeSingles: Object.fromEntries([...singles].map(([v, s]) => [v, { distinct: s.size, notInCache: [...s].filter((l) => !cacheLines.has(l)).length }])),
  treeSim: { sim1BareParser: sim.sim1.length, sim2CacheLoaded: sim.sim2.length, treeDistinctAll: treeAll.size, treeNotInCache: [...treeAll].filter((l) => !cacheLines.has(l)).length },
  itemMisses: items.size, itemNotInCache: [...items].filter((l) => !cacheLines.has(l)).length,
  union: lines.length, onlyCache: cnt((s) => s.size === 1 && s.has('cache')), onlyTree: cnt((s) => s.size === 1 && s.has('tree')), onlyItem: cnt((s) => s.size === 1 && s.has('item')),
  cacheTree: cnt((s) => s.has('cache') && s.has('tree')), cacheItem: cnt((s) => s.has('cache') && s.has('item')), treeItem: cnt((s) => s.has('tree') && s.has('item')), all3: cnt((s) => s.size === 3),
}
console.log(JSON.stringify(stats, null, 1))

const luaLines = lines // JS (Unicode) strings; docker side decodes UTF-8 bytes
const a = answerAll(luaLines, 3000, false)
const SAME = process.env['R51_RUN2'] === 'same' // default: different batching + reversed order (also catches state leaks between lines); 'same': identical partition
const b = SAME ? answerAll(luaLines, 3000, false) : answerAll(luaLines, 2111, true)
const diffs = luaLines.filter((_, i) => ser(a.res[i] as J) !== ser(b.res[i] as J))
if (process.env['R51_SHOWDIFF']) luaLines.forEach((l, i) => { if (ser(a.res[i] as J) !== ser(b.res[i] as J)) console.error(`DIFF ${JSON.stringify(l)}
 run1 ${ser(cvSide(a.res[i]!.i1, true))}
 run2 ${ser(cvSide(b.res[i]!.i1, true))}
 w1 ${ser(cvSide(a.res[i]!.w, false))}
 w2 ${ser(cvSide(b.res[i]!.w, false))}`) })
if (diffs.length) { console.error(`STOP: ${diffs.length} lines differ between two docker runs (D13):\n${diffs.slice(0, 50).map((l) => JSON.stringify(l)).join('\n')}`); process.exit(2) }
if (a.jit !== b.jit) throw new Error('LuaJIT versions differ')

let digest = ''
try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8')) as { manifestSha256: string }
const producer = { image: digest || LUAJIT_IMAGE, luajit: a.jit, pobSha: git(UPSTREAM_POB), dataManifestSha256: lock.manifestSha256 }
const rows = luaLines.map((line, i) => {
  const r = a.res[i]!
  return ser({ line: bytesOf(line), sources: ['cache', 'tree', 'item'].filter((s) => sources.get(line)!.has(s as Src)), inner1: cvSide(r.i1, true), wrapped: cvSide(r.w, false), cacheEq: r.ceq === undefined ? undefined : r.ceq })
})
const head = `{"format":1,"encoding":"strings are byte strings (UTF-8 bytes as chars <256, \\\\u00XX when non-ASCII); numbers {\\"n\\":\\"%.17g\\"}, flags plain ints; tables {a:[array part],h:{hash part}}; mods:null = nil modList; ERROR = [\\"ERROR\\",msg]","producer":${ser(producer as J)},"lines":[\n`
const tail = '\n]}\n'
const parts: string[][] = [[]]
let size = head.length + tail.length
for (const l of rows) {
  const add2 = Buffer.byteLength(l) + 2
  if (size + add2 > MAX_BYTES && parts.at(-1)!.length) { parts.push([]); size = head.length + tail.length }
  parts.at(-1)!.push(l)
  size += add2
}
mkdirSync(FIX, { recursive: true })
if (existsSync(FIX)) for (const f of readdirSync(FIX)) if (/^modparser-\d+\.json$/.test(f)) rmSync(resolve(FIX, f))
const files = parts.map((p, i) => {
  const text = head + p.join(',\n') + tail
  const file = `modparser-${String(i + 1).padStart(2, '0')}.json`
  writeFileSync(resolve(FIX, file), text, 'utf8')
  return { file, lines: p.length, bytes: Buffer.byteLength(text), sha256: sha(text) }
})

// distribution
const dist: Rec = { error: 0, parsedFull: 0, partialModsAndExtra: 0, unparsedExtraOnly: 0, nilNil: 0 }
const cacheNe: { line: string; parser: string; cache: string }[] = []
let cacheEqN = 0, cacheNeN = 0
const distBySrc: Record<string, Rec> = {}
luaLines.forEach((line, i) => {
  const r = a.res[i]!, w = r.w
  const kind = w.err !== undefined ? 'error' : w.mods !== undefined && w.extra === undefined ? 'parsedFull' : w.mods !== undefined ? 'partialModsAndExtra' : w.extra !== undefined ? 'unparsedExtraOnly' : 'nilNil'
  dist[kind]++
  for (const s of sources.get(line)!) { distBySrc[s] ??= {}; distBySrc[s][kind] = (distBySrc[s][kind] ?? 0) + 1 }
  if (r.ceq === true) cacheEqN++
  if (r.ceq === false) {
    cacheNeN++
    if (cacheNe.length < 5) cacheNe.push({ line, parser: `${JSON.stringify((r.i1.fm ?? []).map(lstr))} extra=${JSON.stringify(w.extra === undefined ? null : lstr(w.extra))}`, cache: `${JSON.stringify((r.cache.fm ?? []).map(lstr))} extra=${JSON.stringify(r.cache.extra === undefined ? null : lstr(r.cache.extra))}` })
  }
})
const inner1VsWrappedDiffer = luaLines.filter((_, i) => ser(cvSide(a.res[i]!.i1, false)) !== ser(cvSide(a.res[i]!.w, false))).length
console.log(JSON.stringify({ producer, files, dist, distBySrc, cacheEqual: cacheEqN, cacheNotEqual: cacheNeN, cacheNotEqualExamples: cacheNe, inner1VsWrappedDiffer, errorsSample: luaLines.filter((_, i) => a.res[i]!.w.err !== undefined).slice(0, 5) }, null, 1))
