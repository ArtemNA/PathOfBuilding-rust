// R1.1: snapshot the numeric corpora of pob-ts (test/lua/corpus.ts) answered by REAL LuaJIT (docker) into a
// committed JSON fixture for the Rust crate pob-lua. Truth = LuaJIT (LESSONS 18, MINEFIELD O9), never the TS answers.
// Run from the pob-rust root:  node tools/lua-corpus/export-numeric.ts
// Corpus generators/encoders are imported from pob-ts; the Lua bodies below are copied VERBATIM from
// pob-ts test/lua/corpus.ts (line refs in comments) because they are not exported there.
import { execFileSync } from 'node:child_process'
import { mkdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  encodeNum, formatCorpus, knownLuajitQuirk, mathCorpus, tonumberCorpus, tostringCorpus,
  type CorpusMismatch, type FmtCase,
} from 'file:///C:/pob-work/pob-ts/test/lua/corpus.ts'
import { mulberry32 } from 'file:///C:/pob-work/pob-ts/tools/oracle/fuzz.ts'
import { luajitEval } from 'file:///C:/pob-work/pob-ts/tools/oracle/luajit-eval.ts'
import { dockerAvailable, LUAJIT_IMAGE } from 'file:///C:/pob-work/pob-ts/tools/oracle/luajit.ts'
import { resolveRoots } from 'file:///C:/pob-work/pob-ts/tools/oracle/paths.ts'
import { luaFormat, luaTonumber, luaTostring } from 'file:///C:/pob-work/pob-ts/src/lua/string.ts'
import { luaFloor, luaMax, luaMin, luaMod, luaModf, luaRound } from 'file:///C:/pob-work/pob-ts/src/lua/number.ts'

const HERE = dirname(fileURLToPath(import.meta.url))
const ROOT = resolve(HERE, '../..')
const OUT_FILE = resolve(ROOT, 'crates/pob-lua/tests/fixtures/luajit-numeric.json')
const POB_TS = 'C:/pob-work/pob-ts'

// --- Lua pieces ----------------------------------------------------------------

// pob-ts test/lua/corpus.ts:64-112 (verbatim)
const PRELUDE = `
local function upval(f, want)
  local i = 1
  while true do
    local n, v = debug.getupvalue(f, i)
    if not n then return nil end
    if n == want then return v end
    i = i + 1
  end
end
-- wasm oracle: the unshimmed C printf behind compat54's wrapper; real LuaJIT (docker, P3.6):
-- string.format itself, which has no such upvalue and needs no unshimming.
local RAWF = upval(string.format, "s_format") or string.format
-- LuaJIT has no math.type (everything is a double), no string.pack/unpack (Lua 5.3+): the
-- fallbacks build the two NaNs from fabs (clears the sign bit) and unary minus (sets it), and read
-- a NaN's sign from the formatter, which prints "-nan" for a sign-bit NaN on both runtimes.
local mtype = math.type or function() return "float" end
local HUGE = math.huge
local NAN_POS = string.unpack and string.unpack("<d", "\\0\\0\\0\\0\\0\\0\\248\\127") or math.abs(0 / 0)
local NAN_NEG = string.unpack and string.unpack("<d", "\\0\\0\\0\\0\\0\\0\\248\\255") or -math.abs(0 / 0)
local function nanIsNegative(f)
  if string.pack then return string.pack("<d", f):byte(8) >= 128 end
  return RAWF("%g", f):sub(1, 1) == "-"
end
local function dec(s)
  if s == "nan" then return NAN_POS end
  if s == "-nan" then return NAN_NEG end
  if s == "inf" then return HUGE end
  if s == "-inf" then return -HUGE end
  return tonumber(s) * 1.0
end
local function enc(v)
  local pre = (mtype(v) == "integer") and "i" or "f"
  local f = v * 1.0 -- NOT v + 0.0: adding zero turns -0.0 into +0.0
  if f ~= f then return pre .. (nanIsNegative(f) and "-nan" or "nan") end
  if f == HUGE then return pre .. "inf" end
  if f == -HUGE then return pre .. "-inf" end
  if f == 0 then return pre .. ((1 / f < 0) and "-0.0" or "0") end
  return pre .. RAWF("%.17g", f)
end
local function rows(s)
  local t = {}
  for ln in s:gmatch("[^\\n]+") do t[#t + 1] = ln end
  return t
end
local function unhex(s)
  return (s:gsub("%x%x", function(h) return string.char(tonumber(h, 16)) end))
end
`

// corpus.ts:115-117 (luaScript)
function luaScript(body: string, data: string[]): string {
  return `${PRELUDE}\nlocal DATA = [==[\n${data.join('\n')}\n]==]\nlocal OUT = {}\n${body}\nreturn table.concat(OUT, "\\n")`
}

// corpus.ts:206
const TOSTRING_BODY = 'for _, s in ipairs(rows(DATA)) do OUT[#OUT + 1] = RAWF("%.14g", dec(s)) end'

// corpus.ts:288-300
const FORMAT_BODY = `
for _, ln in ipairs(rows(DATA)) do
  local ch, fmt, spec = ln:match("^([^\\t]*)\\t([^\\t]*)\\t(.*)$")
  local k, rest = spec:sub(1, 1), spec:sub(2)
  local v
  if k == "n" then v = dec(rest) elseif k == "s" then v = rest
  elseif k == "b" then v = (rest == "1") elseif k == "z" then v = nil end
  local r
  if ch == "shim" then r = string.format(fmt, v)
  elseif ch == "snum" then r = RAWF(fmt, RAWF("%.14g", v))
  else r = RAWF(fmt, v) end
  OUT[#OUT + 1] = r
end`

// corpus.ts:352-356
const TONUMBER_BODY = `
for _, h in ipairs(rows(DATA)) do
  local v = tonumber(unhex(h:sub(2)))
  OUT[#OUT + 1] = (v == nil) and "nil" or enc(v)
end`

// corpus.ts:394-405
const MATH_BODY = `
local function ffloor(x) if x == 0 then return x end return math.floor(x) + 0.0 end
for _, ln in ipairs(rows(DATA)) do
  local sa, sb, sd = ln:match("^([^\\t]*)\\t([^\\t]*)\\t(.*)$")
  local a, b = dec(sa), dec(sb)
  local d = (sd ~= "nil") and tonumber(sd) or nil
  local ip, fp = math.modf(a)
  OUT[#OUT + 1] = table.concat({
    enc(math.min(a, b)), enc(math.max(a, b)), enc(a - ffloor(a / b) * b), enc(a % b),
    enc(ip), enc(fp), enc(round(a, d)), enc(floor(a, d)),
  }, "\\t")
end`

// New (R1.1): libm. Each row "fn\ta\tb" (b empty for unary fns). Each fn is computed twice: with jit.off()
// (interpreter) and with jit.on() after a >= 200-iteration warm-up (compiled trace). The compute loop is
// kept free of string work so it is JIT-compilable; encoding happens afterwards.
const LIBM_BODY = `
local ra, rb, idxOf, order = {}, {}, {}, {}
local n = 0
for _, ln in ipairs(rows(DATA)) do
  local fn, sa, sb = ln:match("^([^\\t]*)\\t([^\\t]*)\\t(.*)$")
  n = n + 1
  ra[n] = dec(sa)
  rb[n] = (sb ~= "") and dec(sb) or 0
  if not idxOf[fn] then idxOf[fn] = {}; order[#order + 1] = fn end
  local t = idxOf[fn]
  t[#t + 1] = n
end
local function build()
  return {
    ["^"] = function(a, b) return a ^ b end,
    pow = function(a, b) return math.pow(a, b) end,
    sqrt = function(a) return math.sqrt(a) end,
    log = function(a) return math.log(a) end,
    sin = function(a) return math.sin(a) end,
    cos = function(a) return math.cos(a) end,
    tan = function(a) return math.tan(a) end,
    ceil = function(a) return math.ceil(a) end,
    floor = function(a) return math.floor(a) end,
    abs = function(a) return math.abs(a) end,
  }
end
local function compute(f, idx, vals)
  for k = 1, #idx do local i = idx[k]; vals[i] = f(ra[i], rb[i]) end
end
local function pass(tag)
  local F = build()
  local vals, scratch = {}, {}
  for _, fn in ipairs(order) do
    local idx = idxOf[fn]
    if tag == "jit" then
      for w = 1, 200 do compute(F[fn], idx, scratch) end
    end
    compute(F[fn], idx, vals)
  end
  local res = {}
  for i = 1, n do res[i] = enc(vals[i]) end
  return res
end
jit.off()
jit.flush()
local interp = pass("interp")
local traces = 0
local attached = pcall(function() jit.attach(function() traces = traces + 1 end, "trace") end)
jit.on()
jit.flush()
local jitted = pass("jit")
if attached then pcall(function() jit.attach(nil) end) end
for i = 1, n do OUT[#OUT + 1] = interp[i] .. "\\t" .. jitted[i] end
OUT[#OUT + 1] = "#meta\\t" .. tostring(jit.version) .. "\\t" .. tostring(jit.status()) .. "\\t" .. (attached and traces or "n/a")`

// --- corpus plumbing --------------------------------------------------------------

// corpus.ts:275-280
type FmtArg = FmtCase['arg']
function encodeArg(a: FmtArg): string {
  if (a.kind === 'n') return 'n' + encodeNum(a.v)
  if (a.kind === 's') return 's' + a.v
  if (a.kind === 'b') return a.v ? 'b1' : 'b0'
  return 'z'
}
// corpus.ts:268-273
function fmtChannel(c: FmtCase): string {
  const conv = c.fmt.charAt(c.fmt.length - 1)
  if (conv === 'd' || conv === 'i' || conv === 'x' || conv === 'X') return 'shim'
  if (conv === 's' && c.arg.kind === 'n') return 'snum'
  return 'raw'
}
// corpus.ts:346-347
const toHex = (s: string): string =>
  [...s].map((c) => c.charCodeAt(0).toString(16).padStart(2, '0')).join('')

const roots = resolveRoots()
let dockerRuns = 0
function evalRaw(body: string, data: string[]): string {
  dockerRuns += 1
  const raw = luajitEval<unknown>(luaScript(body, data), { roots })
  if (typeof raw !== 'string') throw new Error(`luajit eval: expected string, got ${typeof raw}`)
  return raw
}
function evalLines(body: string, data: string[]): string[] {
  const raw = evalRaw(body, data)
  const out = raw === '' ? [] : raw.split('\n')
  if (out.length !== data.length) throw new Error(`corpus: ${data.length} inputs -> ${out.length} answers`)
  return out
}

// --- libm corpus (deterministic, mulberry32 seed 0x51ed2701) --------------------------

const rnd = mulberry32(0x51ed2701)
const uni = (lo: number, hi: number): number => lo + (hi - lo) * rnd()
const pick = <T>(xs: T[]): T => xs[Math.floor(rnd() * xs.length)]!

type LibmRow = [fn: string, a: number, b: number | undefined]

function powPairs(): [number, number][] {
  const B_FIXED = [0.9, 0.5, 0.25, 1.5, 2, 3, -1, -0.5, 0, 1, 4, 5, 6, 7, 8, 9, 10]
  const B_INT = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, -1, -2, -3]
  const ps: [number, number][] = []
  const A_SPECIAL = [0, -0, 1, Infinity, NaN]
  const A_INT = Array.from({ length: 20 }, (_, i) => i + 2) // 0 and 1 are in A_SPECIAL
  for (const a of [...A_SPECIAL, ...A_INT]) for (const b of B_FIXED) ps.push([a, b])
  // evasion/5-like and plain randoms in [0, 20000]
  for (let i = 0; i < 12; i += 1) {
    const a = i % 2 === 0 ? Math.floor(uni(0, 100000)) / 5 : uni(0, 20000)
    for (const b of B_FIXED) ps.push([a, b])
  }
  // randoms in [0, 1]
  for (let i = 0; i < 10; i += 1) {
    const a = rnd()
    for (const b of B_FIXED) ps.push([a, b])
  }
  // negative bases with integer exponents
  for (let i = 0; i < 12; i += 1) {
    const a = -(i % 2 === 0 ? Math.floor(uni(1, 50)) : uni(0, 100))
    for (const b of B_INT) ps.push([a, b])
  }
  // random exponents in [-3, 3]
  while (ps.length < 1300) ps.push([pick([uni(0, 20000), rnd(), Math.floor(uni(0, 100000)) / 5]), uni(-3, 3)])
  return ps
}

const SPECIALS = [0, -0, 1, -1, Infinity, -Infinity, NaN, 5e-324, Number.MAX_VALUE, 0.5, 2, 1e-7]
function scaled(): number {
  return (rnd() < 0.5 ? -1 : 1) * rnd() * 10 ** (Math.floor(rnd() * 21) - 6)
}

function unaryInputs(fn: string): number[] {
  const vs: number[] = [...SPECIALS]
  if (fn === 'sin' || fn === 'cos' || fn === 'tan') {
    for (const n of [1, 6, 12, 16, 40, 72]) for (let k = 0; k < n; k += 1) vs.push((2 * Math.PI * k) / n)
    vs.push(1e6, 1e15, -1e6, -1e15, Math.PI, Math.PI / 2, -Math.PI / 2)
    for (let i = 0; i < 300; i += 1) vs.push(uni(-10, 10))
    while (vs.length < 640) vs.push(uni(-10, 10))
  } else {
    while (vs.length < 640) vs.push(scaled())
  }
  return vs
}

const FNS_UNARY = ['sqrt', 'log', 'sin', 'cos', 'tan', 'ceil', 'floor', 'abs']

function libmCorpus(): LibmRow[] {
  const rows: LibmRow[] = []
  const ps = powPairs()
  for (const [a, b] of ps) rows.push(['^', a, b])
  for (const [a, b] of ps) rows.push(['pow', a, b])
  for (const fn of FNS_UNARY) for (const a of unaryInputs(fn)) rows.push([fn, a, undefined])
  return rows
}

// --- run LuaJIT --------------------------------------------------------------------

if (!dockerAvailable()) {
  console.error('STOP: docker is not available (infrastructure) -- no workaround allowed')
  process.exit(2)
}

const tsVals = tostringCorpus()
const tsOut = evalLines(TOSTRING_BODY, tsVals.map(encodeNum))
const fmtCases = formatCorpus()
const fmtOut = evalLines(FORMAT_BODY, fmtCases.map((c) => `${fmtChannel(c)}\t${c.fmt}\t${encodeArg(c.arg)}`))
const tnStrs = tonumberCorpus()
const tnOut = evalLines(TONUMBER_BODY, tnStrs.map((s) => 'x' + toHex(s)))
const mathCases = mathCorpus()
const mathOut = evalLines(MATH_BODY, mathCases.map((c) => `${encodeNum(c.a)}\t${encodeNum(c.b)}\t${c.dec === undefined ? 'nil' : String(c.dec)}`))
const libm = libmCorpus()
const libmRaw = evalRaw(LIBM_BODY, libm.map(([fn, a, b]) => `${fn}\t${encodeNum(a)}\t${b === undefined ? '' : encodeNum(b)}`)).split('\n')
const metaLine = libmRaw.pop()!
if (!metaLine.startsWith('#meta\t')) throw new Error(`libm: missing meta line, got ${metaLine.slice(0, 60)}`)
if (libmRaw.length !== libm.length) throw new Error(`libm: ${libm.length} inputs -> ${libmRaw.length} answers`)
const [, jitVersion, jitStatus, traceCount] = metaLine.split('\t') as [string, string, string, string]

// --- assemble ----------------------------------------------------------------------

const git = (dir: string, ...args: string[]): string =>
  execFileSync('git', ['-C', dir, ...args], { encoding: 'utf8' }).trim()
const pobSha = git(roots.pobRoot, 'rev-parse', 'HEAD')
const tsSha = git(POB_TS, 'rev-parse', '--short', 'HEAD')

const fx = {
  producer: { image: LUAJIT_IMAGE, luajit: jitVersion, pobSha, generator: `pob-ts test/lua/corpus.ts @ ${tsSha}` },
  tostring: tsVals.map((v, i) => [encodeNum(v), tsOut[i]!]),
  format: fmtCases.map((c, i) => [c.fmt, encodeArg(c.arg), fmtOut[i]!]),
  tonumber: tnStrs.map((s, i) => [toHex(s), tnOut[i]!]),
  math: mathCases.map((c, i) => {
    const f = mathOut[i]!.split('\t')
    // fields: min max mod(a-ffloor(a/b)*b) native% ip fp round floor -- native `%` (f[3]) is deliberately dropped
    return [encodeNum(c.a), encodeNum(c.b), c.dec === undefined ? 'nil' : String(c.dec), f[0]!, f[1]!, f[2]!, f[4]!, f[5]!, f[6]!, f[7]!]
  }),
  libm: libm.map(([fn, a, b], i) => {
    const [interp, jit] = libmRaw[i]!.split('\t') as [string, string]
    return [fn, encodeNum(a), b === undefined ? '' : encodeNum(b), interp, jit]
  }),
}

function section(name: string, rows: string[][]): string {
  return `  ${JSON.stringify(name)}: [\n${rows.map((r) => '    ' + JSON.stringify(r)).join(',\n')}\n  ]`
}
const text = [
  '{',
  '  "format": 1,',
  `  "producer": ${JSON.stringify(fx.producer)},`,
  [section('tostring', fx.tostring), section('format', fx.format), section('tonumber', fx.tonumber),
    section('math', fx.math), section('libm', fx.libm)].join(',\n'),
  '}',
  '',
].join('\n')

// --- sanity: counts, token prefixes, size -------------------------------------------

const counts = { tostring: fx.tostring.length, format: fx.format.length, tonumber: fx.tonumber.length, math: fx.math.length, libm: fx.libm.length }
const need = { tostring: 3200, format: 600, tonumber: 340, math: 560, libm: 2500 + 8 * 600 }
for (const k of Object.keys(need) as (keyof typeof need)[]) {
  if (k === 'math' ? counts[k] !== need[k] : counts[k] < need[k]) throw new Error(`count ${k}: ${counts[k]} < ${need[k]}`)
}
for (const fn of ['^', 'pow', ...FNS_UNARY]) {
  const c = fx.libm.filter((r) => r[0] === fn).length
  if (c < (fn === '^' || fn === 'pow' ? 1250 : 600)) throw new Error(`libm ${fn}: only ${c} rows`)
}
const badPrefix = [...fx.libm.flatMap((r) => [r[3]!, r[4]!]), ...fx.math.flatMap((r) => r.slice(3))]
  .filter((t) => !t.startsWith('f')).length
if (badPrefix !== 0) throw new Error(`${badPrefix} output tokens without the f prefix (math.type present?)`)
const bytes = Buffer.byteLength(text, 'utf8')
if (bytes > 1.5 * 1024 * 1024) throw new Error(`STOP: fixture is ${bytes} bytes > 1.5 MB`)

// --- self-check: pob-ts TS functions vs the LuaJIT answers (repeats P3.6) --------------

const decodeIn = (t: string): number => (t === 'nan' ? NaN : t === '-nan' ? -NaN : t === 'inf' ? Infinity : t === '-inf' ? -Infinity : Number(t))
const decodeOut = (t: string): number => decodeIn(t.slice(1))
const sameNum = (want: number, ours: number): boolean => (Number.isNaN(want) ? Number.isNaN(ours) : Object.is(want, ours))

const mism: CorpusMismatch[] = []
for (const [tok, want] of fx.tostring) {
  const ours = luaTostring(decodeIn(tok!))
  if (ours !== want) mism.push({ corpus: 'tostring', input: tok!, expected: want!, actual: ours })
}
for (const [fmt, argTok, want] of fx.format) {
  const k = argTok!.charAt(0)
  const rest = argTok!.slice(1)
  const arg = k === 'n' ? decodeIn(rest) : k === 's' ? rest : k === 'b' ? rest === '1' : undefined
  const ours = luaFormat(fmt!, arg)
  if (ours !== want) mism.push({ corpus: 'format', input: `${fmt} <- ${argTok}`, expected: want!, actual: ours })
}
for (const [hex, want] of fx.tonumber) {
  const s = hex!.replace(/../g, (h) => String.fromCharCode(parseInt(h, 16)))
  const ours = luaTonumber(s)
  const ok = want === 'nil' ? ours === undefined : ours !== undefined && sameNum(decodeOut(want!), ours)
  if (!ok) mism.push({ corpus: 'tonumber', input: JSON.stringify(s), expected: want!, actual: String(ours) })
}
let mathChecks = 0
for (const r of fx.math) {
  const a = decodeIn(r[0]!)
  const b = decodeIn(r[1]!)
  const d = r[2] === 'nil' ? undefined : Number(r[2])
  const [ip, fp] = luaModf(a)
  const ours: [string, number][] = [
    ['min', luaMin(a, b)], ['max', luaMax(a, b)], ['mod', luaMod(a, b)], ['modf.ip', ip], ['modf.fp', fp],
    ['round', luaRound(a, d)], ['floor', luaFloor(a, d)],
  ]
  ours.forEach(([name, v], k) => {
    mathChecks += 1
    if (!sameNum(decodeOut(r[3 + k]!), v)) {
      mism.push({ corpus: `math.${name}`, input: `a=${r[0]} b=${r[1]} dec=${r[2]}`, expected: r[3 + k]!, actual: encodeNum(v) })
    }
  })
}
let quirks = 0
const real: CorpusMismatch[] = []
for (const m of mism) (knownLuajitQuirk(m) ? (quirks += 1) : real.push(m))

// --- libm info ----------------------------------------------------------------------

const JS: Record<string, (a: number, b: number) => number> = {
  '^': (a, b) => a ** b, pow: (a, b) => Math.pow(a, b), sqrt: Math.sqrt, log: Math.log, sin: Math.sin, cos: Math.cos,
  tan: Math.tan, ceil: Math.ceil, floor: Math.floor, abs: Math.abs,
}
let interpNeJit = 0
const interpNeJitEx: string[] = []
const jsNe: Record<string, number> = {}
const jsNeEx: string[] = []
let jsNeTotal = 0
for (const [fn, a, b, interp, jit] of fx.libm) {
  if (interp !== jit) {
    interpNeJit += 1
    if (interpNeJitEx.length < 5) interpNeJitEx.push(`${fn}(${a}${b ? ', ' + b : ''}) interp=${interp} jit=${jit}`)
  }
  const js = JS[fn!]!(decodeIn(a!), b ? decodeIn(b) : 0)
  if (!sameNum(decodeOut(interp!), js)) {
    jsNeTotal += 1
    jsNe[fn!] = (jsNe[fn!] ?? 0) + 1
    if (jsNeEx.length < 3) jsNeEx.push(`${fn}(${a}${b ? ', ' + b : ''}) luajit=${interp} js=${encodeNum(js)}`)
  }
}

// --- write -------------------------------------------------------------------------

mkdirSync(dirname(OUT_FILE), { recursive: true })
writeFileSync(OUT_FILE, text, { encoding: 'utf8' })

console.log(JSON.stringify({
  out: OUT_FILE, bytes: statSync(OUT_FILE).size, dockerRuns, luajit: jitVersion, jitStatus, traceEvents: traceCount, counts,
  self: { mathChecks, mismatches: mism.length, luajitQuirk: quirks, real: real.length, realExamples: real.slice(0, 10) },
  libm: { interpNeJit, interpNeJitEx, jsNeLuajit: jsNeTotal, byFn: jsNe, jsNeEx },
}, null, 1))
if (real.length > 0) {
  console.error(`STOP: ${real.length} non-quirk disagreements between pob-ts and the LuaJIT fixture`)
  process.exit(3)
}
