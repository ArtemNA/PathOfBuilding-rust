// R1.5: Lua-pattern (find/match/gmatch/gsub) and bit.* corpora answered by the DESKTOP LuaJIT (PoB runtime/lua51.dll via
// tools/luajit-win) -> crates/pob-lua/tests/fixtures/luajit-{patterns-modparser,patterns-fuzz,bit}.json. The docker LuaJIT
// (Alpine/musl) evaluates the SAME chunks only for a desktop <-> docker diff (printed, not stored). Truth = LuaJIT, never JS.
// Usage (PowerShell, Node 24), from the pob-rust root:  node tools/lua-corpus/export-patterns.ts [--perf]
//   --perf also measures speed on both engines -> tools/.scratch/perf-patterns.json
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { mulberry32 } from 'file:///C:/pob-work/pob-ts/tools/oracle/fuzz.ts'
import { luajitEval } from 'file:///C:/pob-work/pob-ts/tools/oracle/luajit-eval.ts'
import { dockerAvailable } from 'file:///C:/pob-work/pob-ts/tools/oracle/luajit.ts'
import { resolveRoots } from 'file:///C:/pob-work/pob-ts/tools/oracle/paths.ts'
import { BIT_OPS, BIT_UNARY, genBitInputs, genFuzzCases } from './patterns-fuzz.ts'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const POB_RUNTIME = resolve(process.env.POB_ROOT ?? 'C:/pob-work/project_15/PathOfBuilding', 'runtime')
const FX = join(ROOT, 'crates/pob-lua/tests/fixtures')
const TOOL = join(ROOT, 'tools/luajit-win')
const MAX_BYTES = 1.5 * 1024 * 1024
const PERF = process.argv.includes('--perf')

const lock = JSON.parse(readFileSync(join(ROOT, 'data.lock'), 'utf8')) as { pobSha: string; manifestSha256: string }
const roots = resolveRoots()
const dll = createHash('sha256').update(readFileSync(join(POB_RUNTIME, 'lua51.dll'))).digest('hex')

// --- transport helpers --------------------------------------------------------------------
// Byte strings are JS strings with char codes < 256. JSON token: "s"+text when printable ASCII, else "x"+hex.
const bytes = (s: string): string => Buffer.from(s, 'utf8').toString('latin1')
const hex = (b: string): string => Buffer.from(b, 'latin1').toString('hex')
const unhexB = (h: string): string => Buffer.from(h, 'hex').toString('latin1')
const tk = (b: string): string => (/^[\x20-\x7e]*$/.test(b) ? 's' + b : 'x' + hex(b))
const showB = (b: string): string => JSON.stringify(b.replace(/[^\x20-\x7e]/g, (c) => `\\x${c.charCodeAt(0).toString(16).padStart(2, '0')}`))

// Lua prelude shared by every chunk (ASCII only: luajit-win reads the chunk as UTF-8 and prints lossily).
const PRELUDE = `
local function unhex(h) return (h:gsub("%x%x", function(x) return string.char(tonumber(x, 16)) end)) end
local function hex(s) return (s:gsub(".", function(c) return string.format("%02x", c:byte()) end)) end
local function pack(...) return { n = select("#", ...), ... } end
local function tok(v)
  local t = type(v)
  if v == nil then return "Z" elseif t == "number" then return "N" .. string.format("%.17g", v)
  elseif t == "string" then return "S" .. hex(v) elseif t == "boolean" then return v and "B1" or "B0" end
  error("type " .. t)
end
local function errmsg(e) return hex((tostring(e):gsub('^%[string "[^"]*"%]:%d+: ', ""))) end
local function toks(g, from) local o = {} for i = from, g.n do o[#o + 1] = tok(g[i]) end return table.concat(o, ",") end
`
const mkChunk = (data: Record<string, string>, body: string): string =>
  `${PRELUDE}\n${Object.entries(data).map(([k, v]) => `local ${k} = [==[\n${v}\n]==]`).join('\n')}\n${body}`

// --- engines ------------------------------------------------------------------------------
let tmpRoot: string | undefined
function desktop(lua: string): string {
  tmpRoot ??= mkdtempSync(join(tmpdir(), 'pob-patterns-'))
  writeFileSync(join(tmpRoot, 'chunk.lua'), lua)
  const out = execFileSync(join(TOOL, 'target/release/luajit-win.exe'), [join(tmpRoot, 'chunk.lua')], {
    encoding: 'utf8', maxBuffer: 512 * 1024 * 1024, env: { ...process.env, PATH: `${POB_RUNTIME};${process.env.PATH ?? ''}` },
  })
  return out
}
function docker(lua: string): string {
  const raw = luajitEval<unknown>(lua, { roots })
  if (typeof raw !== 'string') throw new Error(`docker: expected string, got ${typeof raw}`)
  return raw
}
function both(lua: string): { d: string; k: string } {
  const d = desktop(lua)
  return { d, k: docker(lua) }
}
function diffLines(name: string, d: string[], k: string[], describe: (i: number) => string): number {
  let n = 0
  const ex: string[] = []
  const len = Math.max(d.length, k.length)
  for (let i = 0; i < len; i += 1) {
    if (d[i] !== k[i]) { n += 1; if (ex.length < 6) ex.push(`  #${i} ${describe(i)}\n     desktop=${(d[i] ?? '<none>').slice(0, 120)}\n     docker =${(k[i] ?? '<none>').slice(0, 120)}`) }
  }
  console.log(`[diff] ${name}: desktop vs docker differ in ${n} of ${len} rows`)
  for (const e of ex) console.log(e)
  return n
}
function writeFixture(name: string, head: string[], sections: [string, string[]][]): number {
  const text = [
    '{', head.join(',\n') + ',',
    sections.map(([k, rows]) => `"${k}": [\n${rows.join(',\n')}\n]`).join(',\n'), '}', '',
  ].join('\n')
  const b = Buffer.byteLength(text, 'utf8')
  if (b > MAX_BYTES) throw new Error(`STOP: ${name} is ${b} bytes > 1.5 MB`)
  writeFileSync(join(FX, name), text, { encoding: 'utf8' })
  return b
}
const ENCODING = '"encoding": "byte strings as tokens: \\"s\\"+ASCII text or \\"x\\"+hex of the bytes; numbers as JSON numbers (positions) or \\"n\\"+%.17g in fuzz results; null = nil; indexes are 0-based, start/end are Lua 1-based"'

// --- build the desktop tool once, get the luajit version -----------------------------------
if (!dockerAvailable()) { console.error('STOP: docker is not available (infrastructure) -- no workaround allowed'); process.exit(2) }
execFileSync('cargo', ['build', '-q', '--release', '--manifest-path', join(TOOL, 'Cargo.toml')], { stdio: 'inherit' })
const luajit = desktop('return jit.version')
const producer = JSON.stringify({ runtime: 'PathOfBuilding/runtime/lua51.dll', lua51Sha256: dll, luajit, pobSha: lock.pobSha, dataManifestSha256: lock.manifestSha256 })
const header = (extra: string[]): string[] => ['"format": 1', `"producer": ${producer}`, ENCODING, ...extra]
mkdirSync(FX, { recursive: true })
const summary: Record<string, unknown> = { luajit, lua51Sha256: dll.slice(0, 12) }

// --- A: ModParser patterns x mod-cache lines -----------------------------------------------
const mp = (JSON.parse(readFileSync(join(ROOT, 'data/mod-parser.json'), 'utf8')) as { tables: Record<string, Record<string, unknown>> }).tables
// plain flag as passed by ModParser.lua (scan(): 6660-6775; jewelFuncList: find(pattern,1) at 6643 -> nil). Tables only ever
// indexed (never find/match'ed) -- clusterJewelSkills, unsupportedModList, regenTypes, degenTypes, dmgTypes -- get nil.
const PLAIN: Record<string, 't' | 'f' | 'n'> = {
  penTypes: 't', modNameList: 't', baseCostTypes: 't', costTypes: 't', modFlagList: 't', suffixTypes: 't', flagTypes: 'f',
}
const uniq = new Map<string, { b: string; plain: 't' | 'f' | 'n'; tables: string[] }>()
for (const [table, obj] of Object.entries(mp)) {
  const plain = PLAIN[table] ?? 'n'
  for (const key of Object.keys(obj)) {
    const b = bytes(key)
    const id = plain + hex(b)
    const e = uniq.get(id)
    if (e) e.tables.push(table)
    else uniq.set(id, { b, plain, tables: [table] })
  }
}
const pats = [...uniq.values()].sort((x, y) => Buffer.compare(Buffer.from(x.b, 'latin1'), Buffer.from(y.b, 'latin1')) || x.plain.localeCompare(y.plain))
// NB the card says "keys of mod-cache.json" but the file is an array of {line, mods, extra}: use the unique non-empty `line`s.
const cache = [...new Set((JSON.parse(readFileSync(join(ROOT, 'data/mod-cache.json'), 'utf8')) as { line: string }[]).map((e) => e.line).filter((l) => l !== ''))]
// Re-sample (orchestrator decision): paren-free lines (dense hits) + 40 lines WITH "(" (range branch, e.g. (10-15)%).
// Only ~140 of the 23 207 lines contain "(" so the earlier 121-line sample had ~0 of them; the 40 are the first 40 of a
// seed-0x1a7e5e13 shuffle of the "("-lines. Line count of the paren-free part: --lines=N (default 380, min 115).
const shuffle = (xs: string[], n: number): string[] => {
  const r = mulberry32(0x1a7e5e13)
  const a = [...xs]
  for (let i = 0; i < n; i += 1) { const j = i + Math.floor(r() * (a.length - i)); [a[i], a[j]] = [a[j]!, a[i]!] }
  return a.slice(0, n)
}
const LP = Number(process.argv.find((x) => x.startsWith('--lines='))?.slice(8) ?? 380)
const rawLines = [...shuffle(cache.filter((l) => !l.includes('(')), LP), ...shuffle(cache.filter((l) => l.includes('(')), 40)].map(bytes)
const L = rawLines.length

const A_BODY = `
local P, K, LINES, LOW = {}, {}, {}, {}
for ln in DATA_P:gmatch("[^\\n]+") do local k, h = ln:match("^(%a) ?(.*)$"); P[#P + 1] = unhex(h); K[#K + 1] = k end
for ln in DATA_L:gmatch("[^\\n]+") do LINES[#LINES + 1] = unhex(ln); LOW[#LOW + 1] = LINES[#LINES]:lower() end
local out, errs = {}, {}
for pi = 1, #P do
  local pat, k = P[pi], K[pi]
  local plain = nil
  if k == "t" then plain = true elseif k == "f" then plain = false end
  local mark = #out
  local ok, err = pcall(function()
    for li = 1, #LOW do
      local a, b, c1, c2, c3, c4, c5 = LOW[li]:find(pat, 1, plain)
      if a then out[#out + 1] = (pi - 1) .. " " .. (li - 1) .. " " .. a .. " " .. b .. " " .. toks(pack(c1, c2, c3, c4, c5), 1):gsub(",?Z", "") end
    end
  end)
  if not ok then for j = #out, mark + 1, -1 do out[j] = nil end; errs[#errs + 1] = (pi - 1) .. " " .. errmsg(err) end
end
local lo = {}
for i = 1, #LOW do lo[i] = hex(LOW[i]) end
return table.concat({ jit.version, #LOW, table.concat(lo, "\\n"), #errs, table.concat(errs, "\\n"), table.concat(out, "\\n") }, "\\n")`
// NB: pack(c1..c5) keeps n=5 with trailing nils; the gsub above drops the "Z" tokens (nil captures are always trailing).

const dataA = { DATA_P: pats.map((p) => `${p.plain} ${hex(p.b)}`).join('\n'), DATA_L: rawLines.map(hex).join('\n') }
const chunkA = mkChunk(dataA, A_BODY)

function parseA(raw: string): { version: string; low: string[]; errs: string[]; hits: string[] } {
  const r = raw.split('\n')
  const nL = Number(r[1])
  const low = r.slice(2, 2 + nL)
  const nE = Number(r[2 + nL])
  const errs = nE > 0 ? r.slice(3 + nL, 3 + nL + nE) : []
  const hits = r.slice(3 + nL + (nE > 0 ? nE : 0)).filter((s) => s !== '')
  return { version: r[0]!, low, errs, hits }
}
{
  const { d, k } = both(chunkA)
  const a = parseA(d)
  const b = parseA(k)
  const diffs = diffLines('A lowered lines', a.low, b.low, (i) => showB(unhexB(a.low[i]!)))
  const sa = new Set(a.hits)
  const sb = new Set(b.hits)
  const onlyD = a.hits.filter((h) => !sb.has(h))
  const onlyK = b.hits.filter((h) => !sa.has(h))
  console.log(`[diff] A hits: desktop ${a.hits.length}, docker ${b.hits.length}; only-desktop ${onlyD.length}, only-docker ${onlyK.length}; errors desktop ${a.errs.length} docker ${b.errs.length}`)
  for (const h of [...onlyD.slice(0, 3), ...onlyK.slice(0, 3)]) console.log('   ', h)
  const hitRow = (h: string): string => {
    const f = h.split(' ')
    const caps = (f[4] ?? '').split(',').filter((t) => t !== '').map((t) => (t[0] === 'N' ? Number(t.slice(1)) : tk(unhexB(t.slice(1)))))
    return `[${f[0]},${f[1]},${f[2]},${f[3]}${caps.length ? ',' + caps.map((c) => JSON.stringify(c)).join(',') : ''}]`
  }
  const bytesA = writeFixture('luajit-patterns-modparser.json', header([
    `"calls": ${pats.length * L}`, `"hitCount": ${a.hits.length}`,
  ]), [
    ['errors', a.errs.map((e) => { const [i, m] = e.split(' '); return `[${i},${JSON.stringify(tk(unhexB(m ?? '')))}]` })],
    ['patterns', pats.map((p) => `[${JSON.stringify(tk(p.b))},${p.plain === 't' ? 'true' : p.plain === 'f' ? 'false' : 'null'},${JSON.stringify(p.tables.join(','))}]`)],
    ['lines', a.low.map((h) => JSON.stringify(tk(unhexB(h))))],
    ['hits', a.hits.map(hitRow)],
  ])
  summary.A = { patterns: pats.length, lines: L, calls: pats.length * L, hitCount: a.hits.length, linesWithHit: new Set(a.hits.map((h) => h.split(' ')[1])).size, errors: a.errs.length, bytes: bytesA, diffs: { lines: diffs, onlyDesktop: onlyD.length, onlyDocker: onlyK.length } }
}

// --- B: fuzz --------------------------------------------------------------------------------
const lowerAscii = (s: string): string => s.replace(/[A-Z]/g, (c) => c.toLowerCase())
const fcases = genFuzzCases(cache.map((c) => lowerAscii(bytes(c))).filter((s) => s.length <= 40 && /^[\x20-\x7e]*$/.test(s)))
const nt = (v: string | number | null): string => (v === null ? 'n' : String(v))
const fline = (c: (typeof fcases)[number]): string => {
  const e = c.extra
  const ex = c.op === 'gsub_s' ? ['x' + hex(e[0] as string), nt(e[1]!)] : c.op === 'gsub_f' ? [(e[0] as string)[0]!, nt(e[1]!)] : e.map((v) => nt(v))
  return [c.op, 'x' + hex(c.subject), 'x' + hex(c.pattern), ...ex].join(' ')
}
const B_BODY = `
local FN = { u = function(c) return c:upper() end, n = function() return nil end, f = function() return false end }
local TBL = { a = "X", ["1"] = "Y" }
local function num(w) if w == "n" then return nil end return tonumber(w) end
local res = {}
for ln in DATA:gmatch("[^\\n]+") do
  local f = {}
  for w in ln:gmatch("[^ ]+") do f[#f + 1] = w end
  local op, s, p = f[1], unhex(f[2]:sub(2)), unhex(f[3]:sub(2))
  local r
  if op == "find" then r = pack(pcall(string.find, s, p, num(f[4])))
  elseif op == "findp" then r = pack(pcall(string.find, s, p, num(f[4]), true))
  elseif op == "match" then r = pack(pcall(string.match, s, p, num(f[4])))
  elseif op == "gsub_s" then r = pack(pcall(string.gsub, s, p, unhex(f[4]:sub(2)), num(f[5])))
  elseif op == "gsub_t" then r = pack(pcall(string.gsub, s, p, TBL, num(f[4])))
  elseif op == "gsub_f" then r = pack(pcall(string.gsub, s, p, FN[f[4]], num(f[5])))
  elseif op == "gmatch" then
    r = pack(pcall(function()
      local groups, it = {}, string.gmatch(s, p)
      for _ = 1, 50 do
        local g = pack(it())
        if g.n == 0 or g[1] == nil then break end
        groups[#groups + 1] = toks(g, 1)
      end
      return table.concat(groups, "|")
    end))
  end
  if not r[1] then res[#res + 1] = "E" .. errmsg(r[2])
  elseif op == "gmatch" then res[#res + 1] = r[2]
  else res[#res + 1] = toks(r, 2) end
end
return table.concat(res, "\\n")`
const chunkB = mkChunk({ DATA: fcases.map(fline).join('\n') }, B_BODY)
const tokJson = (t: string): string => (t[0] === 'Z' ? 'null' : t[0] === 'B' ? (t[1] === '1' ? 'true' : 'false') : t[0] === 'N' ? JSON.stringify('n' + t.slice(1)) : JSON.stringify(tk(unhexB(t.slice(1)))))
{
  const { d, k } = both(chunkB)
  const rd = d.split('\n')
  if (rd.length !== fcases.length) throw new Error(`fuzz: ${fcases.length} cases -> ${rd.length} answers`)
  diffLines('B fuzz', rd, k.split('\n'), (i) => `${fline(fcases[i]!)}`.replace(/x([0-9a-f]*)/g, (_m, h: string) => showB(unhexB(h))))
  const perOp: Record<string, number> = {}
  let errCount = 0
  const recs = fcases.map((c, i) => {
    perOp[c.op] = (perOp[c.op] ?? 0) + 1
    const line = rd[i]!
    let res: string
    if (line[0] === 'E') { errCount += 1; res = `["ERR",${JSON.stringify(tk(unhexB(line.slice(1))))}]` }
    else if (c.op === 'gmatch') res = `[${line === '' ? '' : line.split('|').map((g) => `[${g.split(',').map(tokJson).join(',')}]`).join(',')}]`
    else res = `[${line.split(',').map(tokJson).join(',')}]`
    const input = [c.op, tk(c.subject), tk(c.pattern), ...c.extra.map((v, j) => (c.op === 'gsub_s' && j === 0 ? tk(v as string) : v))]
    return `[${JSON.stringify(input)},${res}]`
  })
  const b = writeFixture('luajit-patterns-fuzz.json', header([
    '"seed": "0x1a7e5e11"', '"gsubTable": {"a":"X","1":"Y"}',
    '"opsNote": "find/findp/match: extra=[init]; gmatch: []; gsub_s: [repl,n]; gsub_t: [n]; gsub_f: [mode(upper|nil|false),n]; findp = find with plain=true; gmatch result = array of capture arrays (max 50); gsub result = [string,count]; error = [\\"ERR\\",msg]"',
    `"cases": ${fcases.length}`,
  ]), [['cases', recs]])
  summary.B = { cases: fcases.length, perOp, errCount, bytes: b }
}

// --- C: bit.* ----------------------------------------------------------------------------------
const inputs = genBitInputs()
const numTok = (x: number): string => (Number.isNaN(x) ? 'nan' : x === Infinity ? 'inf' : x === -Infinity ? '-inf' : Object.is(x, -0) ? '-0' : String(x))
const bitRows: [string, string, string][] = []
for (const op of BIT_OPS) {
  inputs.forEach((a, i) => bitRows.push([op, numTok(a), BIT_UNARY.includes(op) ? '-' : numTok(inputs[(i * 7 + 3) % inputs.length]!)]))
}
const C_BODY = `
local function dec(s)
  if s == "nan" then return 0 / 0 elseif s == "inf" then return math.huge elseif s == "-inf" then return -math.huge end
  return tonumber(s)
end
local R = {}
for ln in DATA:gmatch("[^\\n]+") do
  local op, a, b = ln:match("^(%a+) (%S+) (%S+)$")
  R[#R + 1] = { bit[op], dec(a), (b ~= "-") and dec(b) or nil }
end
local function pass()
  local res = {}
  for i = 1, #R do
    local r = R[i]
    local ok, v
    if r[3] == nil then ok, v = pcall(r[1], r[2]) else ok, v = pcall(r[1], r[2], r[3]) end
    res[i] = ok and string.format("%.17g", v) or ("E" .. errmsg(v))
  end
  return res
end
jit.off(); jit.flush()
local interp = pass()
jit.on(); jit.flush()
for _ = 1, 300 do pass() end
local jitted = pass()
local o = {}
for i = 1, #R do o[i] = interp[i] .. "|" .. jitted[i] end
return table.concat(o, "\\n")`
const chunkC = mkChunk({ DATA: bitRows.map((r) => r.join(' ')).join('\n') }, C_BODY)
{
  const { d, k } = both(chunkC)
  const rd = d.split('\n')
  if (rd.length !== bitRows.length) throw new Error(`bit: ${bitRows.length} rows -> ${rd.length} answers`)
  const bad = rd.filter((x) => x.split('|')[0] !== x.split('|')[1]).length
  const errs = rd.filter((x) => x[0] === 'E' || x.includes('|E')).length
  const kd = k.split('\n')
  diffLines('C bit (interp|jit pair)', rd, kd, (i) => bitRows[i]!.join(' '))
  if (bad > 0) { console.error(`STOP: ${bad} bit rows where interpreter != JIT`); process.exit(3) }
  const b = writeFixture('luajit-bit.json', header([`"cases": ${bitRows.length}`, `"inputs": ${inputs.length}`,
    '"opsNote": "[op,a,b,result]; a,b decimal text or nan/inf/-inf/-0; b empty for bnot/tobit; result %.17g (interpreter pass == JIT pass, checked)"']),
  [['cases', bitRows.map((r, i) => JSON.stringify([r[0], r[1], r[2] === '-' ? '' : r[2], rd[i]!.split('|')[0]]))]])
  summary.C = { cases: bitRows.length, inputs: inputs.length, errors: errs, interpNeJit: bad, bytes: b }
}

// --- D: perf (both engines) ---------------------------------------------------------------------
if (PERF) {
  const spec = pats.map((p, i) => (p.tables.includes('specialModList') && p.plain === 'n' ? i + 1 : 0)).filter((i) => i > 0)
  const D_BODY = `
jit.on()
local P, K, LOW = {}, {}, {}
for ln in DATA_P:gmatch("[^\\n]+") do local k, h = ln:match("^(%a) ?(.*)$"); P[#P + 1] = unhex(h); K[#K + 1] = k end
for ln in DATA_L:gmatch("[^\\n]+") do LOW[#LOW + 1] = unhex(ln):lower() end
local PL = {}
for i = 1, #P do if K[i] == "t" then PL[i] = true elseif K[i] == "f" then PL[i] = false end end
local S = {}
for w in DATA_S:gmatch("%d+") do S[#S + 1] = tonumber(w) end
local OUT = {}
local function f_all(reps)
  local n = 0
  for _ = 1, reps do
    for pi = 1, #P do
      local pat, plain = P[pi], PL[pi]
      for li = 1, #LOW do if LOW[li]:find(pat, 1, plain) then n = n + 1 end end
    end
  end
  return n
end
local function f_scan(reps)
  local n = 0
  for _ = 1, reps do
    for li = 1, #LOW do
      local line = LOW[li]
      local lineLower = line:lower()
      local bestIndex, bestEndIndex, bestPattern, bestCaps = nil, nil, "", nil
      for si = 1, #S do
        local pattern = P[S[si]]
        local index, endIndex, cap1, cap2, cap3, cap4, cap5 = lineLower:find(pattern, 1, nil)
        if index and (not bestIndex or index < bestIndex or (index == bestIndex and (endIndex > bestEndIndex or (endIndex == bestEndIndex and #pattern > #bestPattern)))) then
          bestIndex, bestEndIndex, bestPattern, bestCaps = index, endIndex, pattern, { cap1, cap2, cap3, cap4, cap5 }
        end
      end
      if bestIndex then n = n + 1 end
    end
  end
  return n
end
local function f_gsub(reps)
  local n = 0
  for _ = 1, reps do for li = 1, #LOW do local r = LOW[li]:gsub("(%d+)", "%1"); n = n + #r end end
  return n
end
local function measure(name, fn, ops)
  local reps = 1
  while true do
    local t = os.clock(); fn(reps)
    if (os.clock() - t) * 1000 >= 300 then break end
    reps = reps * 2
  end
  local runs = {}
  for k = 1, 5 do local t = os.clock(); fn(reps); runs[k] = string.format("%.6f", os.clock() - t) end
  OUT[#OUT + 1] = table.concat({ name, tostring(ops), tostring(reps), table.concat(runs, ",") }, "\\t")
end
measure("full_pass_find", f_all, #P * #LOW)
measure("scan_like_specialModList", f_scan, #LOW * #S)
measure("gsub_digits", f_gsub, #LOW)
return jit.version .. "\\n" .. table.concat(OUT, "\\n")`
  const chunkD = mkChunk({ ...dataA, DATA_S: spec.join(' ') }, D_BODY)
  const med = (xs: number[]): number => [...xs].sort((x, y) => x - y)[Math.floor(xs.length / 2)]!
  const parse = (raw: string): Record<string, unknown> => {
    const [ver, ...rows] = raw.split('\n')
    const o: Record<string, unknown> = { luajit: ver }
    for (const r of rows) {
      const [name, ops, reps, runs] = r.split('\t') as [string, string, string, string]
      const secs = runs.split(',').map(Number)
      const msPerRep = secs.map((s) => (s * 1000) / Number(reps))
      const nsPerOp = secs.map((s) => (s * 1e9) / (Number(reps) * Number(ops)))
      o[name] = { opsPerRep: Number(ops), reps: Number(reps), runs: secs.length, msPerRep: { median: med(msPerRep), min: Math.min(...msPerRep), max: Math.max(...msPerRep) },
        nsPerOp: { median: med(nsPerOp), min: Math.min(...nsPerOp), max: Math.max(...nsPerOp) }, runSeconds: secs }
    }
    return o
  }
  const out = { desktop: parse(desktop(chunkD)), docker: parse(docker(chunkD)), patterns: pats.length, lines: L, scanPatterns: spec.length }
  const f = join(ROOT, 'tools/.scratch/perf-patterns.json')
  mkdirSync(dirname(f), { recursive: true })
  writeFileSync(f, JSON.stringify(out, null, 1) + '\n')
  summary.perf = out
}

if (tmpRoot) rmSync(tmpRoot, { recursive: true, force: true })
console.log(JSON.stringify(summary, null, 1))
for (const n of ['luajit-patterns-modparser.json', 'luajit-patterns-fuzz.json', 'luajit-bit.json']) {
  console.log(n, statSync(join(FX, n)).size, 'bytes')
}
