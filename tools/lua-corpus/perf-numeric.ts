// R1.1: speed of the same numeric operations on the same inputs in LuaJIT (docker, jit.on, os.clock inside the
// chunk -- docker boot excluded) and in pob-ts (node, performance.now). Output: tools/.scratch/perf-numeric.json.
// Run from the pob-rust root:  node tools/lua-corpus/perf-numeric.ts
import '../upstream.ts'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { encodeNum, tonumberCorpus, tostringCorpus } from 'file:///C:/pob-work/pob-ts/test/lua/corpus.ts'
import { luajitEval } from 'file:///C:/pob-work/pob-ts/tools/oracle/luajit-eval.ts'
import { dockerAvailable } from 'file:///C:/pob-work/pob-ts/tools/oracle/luajit.ts'
import { resolveRoots } from 'file:///C:/pob-work/pob-ts/tools/oracle/paths.ts'
import { luaFormat, luaTonumber, luaTostring } from 'file:///C:/pob-work/pob-ts/src/lua/string.ts'
import { luaRound } from 'file:///C:/pob-work/pob-ts/src/lua/number.ts'

const HERE = dirname(fileURLToPath(import.meta.url))
const OUT_FILE = resolve(HERE, '../.scratch/perf-numeric.json')
const RUNS = 5
const MIN_MS = 200
const CAL_MS = 300 // calibrate a bit above MIN_MS so every timed run clears MIN_MS

// Inputs. tostring: the whole tostring corpus. tonumber: the whole tonumber corpus (hex-transported).
// format/round/pow/sin: the finite members of the tostring corpus with |v| < 1e15 (so %.2f stays short);
// pow uses |v| so `a ^ 0.9` is never NaN.
const tsAll = tostringCorpus()
const tnStrs = tonumberCorpus()
const fin = tsAll.filter((v) => Number.isFinite(v) && Math.abs(v) < 1e15)
const toHex = (s: string): string => [...s].map((c) => c.charCodeAt(0).toString(16).padStart(2, '0')).join('')

// --- LuaJIT side -----------------------------------------------------------------------

const LUA = `
local function dec(s)
  if s == "nan" then return math.abs(0 / 0) end
  if s == "-nan" then return -math.abs(0 / 0) end
  if s == "inf" then return math.huge end
  if s == "-inf" then return -math.huge end
  return tonumber(s) * 1.0
end
local function unhex(s)
  return (s:gsub("%x%x", function(h) return string.char(tonumber(h, 16)) end))
end
local function lines(s) local t = {} for ln in s:gmatch("[^\\n]+") do t[#t + 1] = ln end return t end
local ALL, FIN, TN = {}, {}, {}
for i, s in ipairs(lines(DATA_ALL)) do ALL[i] = dec(s) end
for i, s in ipairs(lines(DATA_FIN)) do FIN[i] = dec(s) end
for i, h in ipairs(lines(DATA_TN)) do TN[i] = unhex(h:sub(2)) end
local ABS = {}
for i = 1, #FIN do ABS[i] = math.abs(FIN[i]) end

local sformat, tostr, tonum, msin = string.format, tostring, tonumber, math.sin
local pround = round
assert(type(pround) == "function", "PoB global round() missing")

local function op_tostring(vs, reps) local acc = 0 for _ = 1, reps do for i = 1, #vs do acc = acc + #tostr(vs[i]) end end return acc end
local function op_format(vs, reps) local acc = 0 for _ = 1, reps do for i = 1, #vs do acc = acc + #sformat("%.2f", vs[i]) end end return acc end
local function op_tonumber(vs, reps)
  local acc = 0
  for _ = 1, reps do for i = 1, #vs do local v = tonum(vs[i]) if v then acc = acc + 1 end end end
  return acc
end
local function op_round(vs, reps) local acc = 0 for _ = 1, reps do for i = 1, #vs do acc = acc + pround(vs[i], 2) end end return acc end
local function op_pow(vs, reps) local acc = 0 for _ = 1, reps do for i = 1, #vs do acc = acc + vs[i] ^ 0.9 end end return acc end
local function op_sin(vs, reps) local acc = 0 for _ = 1, reps do for i = 1, #vs do acc = acc + msin(vs[i]) end end return acc end

local function measure(name, fn, vs)
  local reps = 1
  while true do
    local t = os.clock()
    fn(vs, reps)
    if (os.clock() - t) * 1000 >= CAL_MS then break end
    reps = reps * 2
  end
  local runs = {}
  for k = 1, RUNS do
    local t = os.clock()
    fn(vs, reps)
    local dt = os.clock() - t
    runs[k] = string.format("%.6f", dt)
  end
  OUT[#OUT + 1] = table.concat({ name, tostring(#vs), tostring(reps), table.concat(runs, ",") }, "\\t")
end

jit.on()
measure("tostring", op_tostring, ALL)
measure("format", op_format, FIN)
measure("tonumber", op_tonumber, TN)
measure("round", op_round, FIN)
measure("pow", op_pow, ABS)
measure("sin", op_sin, FIN)
OUT[#OUT + 1] = "#meta\\t" .. tostring(jit.version) .. "\\t" .. tostring(jit.status())
`

function luaScript(): string {
  const blk = (name: string, xs: string[]): string => `local ${name} = [==[\n${xs.join('\n')}\n]==]\n`
  return `local CAL_MS, RUNS = ${CAL_MS}, ${RUNS}\nlocal OUT = {}\n` +
    blk('DATA_ALL', tsAll.map(encodeNum)) + blk('DATA_FIN', fin.map(encodeNum)) +
    blk('DATA_TN', tnStrs.map((s) => 'x' + toHex(s))) + LUA + 'return table.concat(OUT, "\\n")'
}

interface Stat { median_ns: number; min_ns: number; max_ns: number; n: number; reps: number; runs_ms: number[] }

function stat(nsPerOp: number[], n: number, reps: number, runsMs: number[]): Stat {
  const s = [...nsPerOp].sort((a, b) => a - b)
  const median = s.length % 2 ? s[(s.length - 1) / 2]! : (s[s.length / 2 - 1]! + s[s.length / 2]!) / 2
  return { median_ns: median, min_ns: s[0]!, max_ns: s[s.length - 1]!, n, reps, runs_ms: runsMs }
}

if (!dockerAvailable()) {
  console.error('STOP: docker is not available (infrastructure) -- no workaround allowed')
  process.exit(2)
}
const roots = resolveRoots()
const raw = luajitEval<string>(luaScript(), { roots }).split('\n')
const metaLine = raw.pop()!
const luajit: Record<string, Stat> = {}
for (const ln of raw) {
  const [name, nStr, repsStr, runsStr] = ln.split('\t') as [string, string, string, string]
  const n = Number(nStr)
  const reps = Number(repsStr)
  const secs = runsStr.split(',').map(Number)
  luajit[name] = stat(secs.map((s) => (s * 1e9) / (n * reps)), n, reps, secs.map((s) => s * 1000))
}

// --- TS side -----------------------------------------------------------------------

let sink = 0
const absFin = fin.map(Math.abs)
const ops: [string, (reps: number) => void, number][] = [
  ['tostring', (reps) => { let a = 0; for (let r = 0; r < reps; r += 1) for (const v of tsAll) a += luaTostring(v).length; sink += a }, tsAll.length],
  ['format', (reps) => { let a = 0; for (let r = 0; r < reps; r += 1) for (const v of fin) a += luaFormat('%.2f', v).length; sink += a }, fin.length],
  ['tonumber', (reps) => { let a = 0; for (let r = 0; r < reps; r += 1) for (const s of tnStrs) if (luaTonumber(s) !== undefined) a += 1; sink += a }, tnStrs.length],
  ['round', (reps) => { let a = 0; for (let r = 0; r < reps; r += 1) for (const v of fin) a += luaRound(v, 2); sink += a }, fin.length],
  ['pow', (reps) => { let a = 0; for (let r = 0; r < reps; r += 1) for (const v of absFin) a += v ** 0.9; sink += a }, absFin.length],
  ['sin', (reps) => { let a = 0; for (let r = 0; r < reps; r += 1) for (const v of fin) a += Math.sin(v); sink += a }, fin.length],
]
const ts: Record<string, Stat> = {}
for (const [name, fn, n] of ops) {
  let reps = 1
  for (;;) {
    const t = performance.now()
    fn(reps)
    if (performance.now() - t >= CAL_MS) break
    reps *= 2
  }
  const ms: number[] = []
  for (let k = 0; k < RUNS; k += 1) {
    const t = performance.now()
    fn(reps)
    ms.push(performance.now() - t)
  }
  ts[name] = stat(ms.map((m) => (m * 1e6) / (n * reps)), n, reps, ms)
}

// --- report --------------------------------------------------------------------------

const minRun = Math.min(...[...Object.values(luajit), ...Object.values(ts)].flatMap((s) => s.runs_ms))
const meta = metaLine.split('\t')
mkdirSync(dirname(OUT_FILE), { recursive: true })
writeFileSync(OUT_FILE, JSON.stringify({ luajit, ts, meta: { luajit: meta[1], jitStatus: meta[2], node: process.version, runs: RUNS, minRunMs: minRun, sink } }, null, 1) + '\n')

const f = (s: Stat): string => `${s.median_ns.toFixed(1)} [${s.min_ns.toFixed(1)}-${s.max_ns.toFixed(1)}]`
console.log('op        n      LuaJIT ns/op median [min-max]      TS ns/op median [min-max]     TS/LuaJIT')
for (const name of Object.keys(luajit)) {
  const a = luajit[name]!
  const b = ts[name]!
  console.log(`${name.padEnd(9)} ${String(a.n).padEnd(6)} ${f(a).padEnd(32)} ${f(b).padEnd(30)} ${(b.median_ns / a.median_ns).toFixed(2)}x`)
}
console.log(`LuaJIT: ${meta[1]} jit.status=${meta[2]}; node ${process.version}; shortest timed run ${minRun.toFixed(1)} ms (need >= ${MIN_MS}); sink=${sink}`)
if (minRun < MIN_MS) {
  console.error(`STOP: a timed run was shorter than ${MIN_MS} ms`)
  process.exit(3)
}
