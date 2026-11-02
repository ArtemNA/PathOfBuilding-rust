// R2.1: snapshot the ModStore corpora of pob-ts (test/mods/cases.ts, cases-tags.ts, fuzzer seeds) answered by REAL LuaJIT
// (docker, PoB boot) into committed JSON fixtures for the Rust crate pob-mods. Truth = LuaJIT; TS answers are only a self-check.
// Run from the pob-rust root:
//   node tools/lua-corpus/export-modstore.ts          -> crates/pob-mods/tests/fixtures/modstore-*.json
//   node tools/lua-corpus/export-modstore.ts perf     -> tools/.scratch/perf-modstore.json
// The Lua half is pob-ts tools/oracle/lua/modstore-oracle.lua read VERBATIM, except its RAWF line: the wasm oracle fetches the
// unshimmed printf from compat54's `s_format` upvalue; desktop LuaJIT has no such upvalue and string.format IS the raw printf.
import { POB as UPSTREAM_POB } from '../upstream.ts'
import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { CASES: PLAIN } = await import(`${P}/test/mods/cases.ts`)
const { CASES: TAGGED } = await import(`${P}/test/mods/cases-tags.ts`)
const { runCaseOnTs } = await import(`${P}/test/mods/harness.ts`)
const { generateModStoreCase } = await import(`${P}/tools/oracle/modstore-fuzz.ts`)
const { buildLuaCode, compareRuns, oracleRunSchema } = await import(`${P}/tools/oracle/modstore-oracle.ts`)
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
type Json = null | boolean | number | string | Json[] | { [k: string]: Json }
type Case = { id: string; queries: { method: string }[] } & Record<string, Json>

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-mods/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const POB_TS = 'C:/pob-work/pob-ts'
const MAX_BYTES = 1_450_000 // < 1.5 MB per fixture part
const FUZZ_SEEDS = 800
const PER_BATCH = 134 // fuzz cases per docker run -> 6 runs + 1 for the two small corpora
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()

// --- LuaJIT chunk -----------------------------------------------------------------------------------------------
const RAWF_ORIG = 'local RAWF = assert(upval(string.format, "s_format"), "compat54 s_format upvalue missing")'
const RAWF_LJ = 'local RAWF = upval(string.format, "s_format") or string.format'
const snippet = (c: Case): { src: string; call: string } => {
  const code = buildLuaCode(c as never)
  const i = code.lastIndexOf('\nreturn run(')
  const src = code.slice(0, i).replace(/\r\n/g, '\n')
  if (!src.includes(RAWF_ORIG)) throw new Error('modstore-oracle.lua changed: RAWF line not found')
  return { src: src.replace(RAWF_ORIG, RAWF_LJ), call: code.slice(i + '\nreturn '.length).trimEnd() }
}
/** Wraps the oracle snippet in a function (its top-level locals stay local) and exposes the internals the perf mode needs. */
const wrap = (src: string): string =>
  `local dkjson = require("dkjson")\nlocal function mk()\n${src}\nreturn run, runQuery, buildStore, OPS, needStore, toFloat, buildCfg\nend\n` +
  'local run, runQuery, buildStore, OPS, needStore, toFloat, buildCfg = mk()\n'

function luaBatch(cases: Case[]): { jit: string; cases: ({ results: Json[] } | { error: string })[] } {
  const calls = cases.map((c) => `function() return ${snippet(c).call} end`)
  const code =
    wrap(snippet(cases[0]!).src) +
    `local CASES = {\n${calls.join(',\n')}\n}\nlocal out = {}\n` +
    'for i, f in ipairs(CASES) do local ok, r = pcall(f); if ok then out[i] = r else out[i] = { error = tostring(r) } end end\n' +
    'return { jit = jit.version, cases = out }'
  return luajitEval(code, { roots: resolveRoots() })
}

// --- deterministic JSON (sorted keys, -0 as "-0.0", dkjson [] kept) --------------------------------------------
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
function ser(v: Json): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  return `{${Object.keys(v).filter((k) => v[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(v[k]!)}`).join(',')}}`
}

interface Row { c: Case; results: Json[] }
const countErrors = (rows: Row[]): number => rows.reduce((n, r) => n + r.results.filter((x) => (x as { k: string }).k === 'error').length, 0)

/** Runs `cases` in chunks of `per` per docker run; any Lua-side failure of a case is infrastructure -> throw. */
function answer(cases: Case[], per: number, jits: Set<string>): Row[] {
  const rows: Row[] = []
  for (let i = 0; i < cases.length; i += per) {
    const part = cases.slice(i, i + per)
    const res = luaBatch(part)
    jits.add(res.jit)
    part.forEach((c, j) => {
      const r = res.cases[j]!
      if ('error' in r) throw new Error(`LuaJIT died on case ${c.id}: ${r.error}`)
      rows.push({ c, results: r.results })
    })
    console.log(`docker batch ${i / per + 1}: ${part.length} cases`)
  }
  return rows
}

function selfCheck(name: string, rows: Row[]) {
  let mismatches = 0
  let mismatchesIgnoringErrorText = 0
  let queries = 0
  for (const { c, results } of rows) {
    const lj = oracleRunSchema.parse({ results })
    const ts = runCaseOnTs(c as never)
    const cmp = compareRuns(lj, ts)
    mismatchesIgnoringErrorText += compareRuns(lj, ts, { errorsByKind: true }).mismatches.length
    queries += cmp.total
    mismatches += cmp.mismatches.length
    for (const m of cmp.mismatches) {
      console.error(`MISMATCH ${name} ${c.id} q#${m.index} ${JSON.stringify(c.queries[m.index])}\n  luajit ${JSON.stringify(m.expected)}\n  ts     ${JSON.stringify(m.actual)}`)
    }
  }
  return { cases: rows.length, queries, mismatches, mismatchesIgnoringErrorText, errors: countErrors(rows) }
}

// --- writing ----------------------------------------------------------------------------------------------------
function writeParts(base: string, rows: Row[], producer: Json): { file: string; cases: number; queries: number; bytes: number }[] {
  const head = `{"format":1,"producer":${ser(producer)},"cases":[\n`
  const tail = '\n]}\n'
  const lines = rows.map((r) => ser({ case: r.c as Json, results: r.results }))
  const parts: string[][] = [[]]
  let size = head.length + tail.length
  for (const l of lines) {
    const add = Buffer.byteLength(l) + 2
    if (size + add > MAX_BYTES && parts.at(-1)!.length > 0) { parts.push([]); size = head.length + tail.length }
    parts.at(-1)!.push(l)
    size += add
  }
  mkdirSync(FIX, { recursive: true })
  let off = 0
  return parts.map((p, i) => {
    const file = resolve(FIX, parts.length === 1 ? `${base}.json` : `${base}-${String(i + 1).padStart(2, '0')}.json`)
    writeFileSync(file, head + p.join(',\n') + tail, 'utf8')
    const part = rows.slice(off, off + p.length)
    off += p.length
    return { file, cases: p.length, queries: part.reduce((n, r) => n + r.results.length, 0), bytes: statSync(file).size }
  })
}

// --- perf -------------------------------------------------------------------------------------------------------
const RUNS = 7
const MIN_MS = 200
const median = (a: number[]): number => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)]!
const stat = (a: number[]) => ({ median: median(a), min: Math.min(...a), max: Math.max(...a), runs: a })
const REPEAT = 20 // TS queries-only: each Sum/More/Flag query repeated REPEAT times, minus the same case without queries

const PERF_LUA = `
jit.on()
local DEC, QS, STORES = {}, {}, {}
for i, p in ipairs(PAYLOADS) do DEC[i] = dkjson.decode(p) end
local NQ = 0
for _, p in ipairs(DEC) do NQ = NQ + #p.queries end
local function full()
  local t = os.clock()
  for i = 1, #DEC do run(DEC[i]) end
  return os.clock() - t
end
collectgarbage()
full() -- warm-up (JIT traces), not recorded
local fullRuns = {}
for r = 1, ${RUNS} do collectgarbage(); fullRuns[r] = full() end
-- queries only (Sum / More / Flag) on stores built once
local nsel = 0
for i, raw in ipairs(DEC) do
  local payload = toFloat(raw)
  local stores = {}
  for _, spec in ipairs(payload.stores) do stores[spec.id] = buildStore(spec, stores) end
  for _, op in ipairs(payload.ops or {}) do OPS[op.op](needStore(stores, op.store, "op"), op, stores) end
  local sel = {}
  for _, q in ipairs(payload.queries) do
    if q.method == "Sum" or q.method == "More" or q.method == "Flag" then sel[#sel + 1] = q end
  end
  nsel = nsel + #sel
  QS[i], STORES[i] = sel, stores
end
local function pass() for i = 1, #QS do local sel, st = QS[i], STORES[i]; for j = 1, #sel do runQuery(sel[j], st) end end end
pass() pass()
local qRuns, qPasses = {}, {}
for r = 1, ${RUNS} do
  collectgarbage()
  local t, passes = os.clock(), 0
  repeat pass(); passes = passes + 1 until os.clock() - t >= ${MIN_MS / 1000}
  qRuns[r], qPasses[r] = os.clock() - t, passes
end
return { jit = jit.version, nq = NQ, nsel = nsel, fullRuns = fullRuns, qRuns = qRuns, qPasses = qPasses }
`

function perf(): void {
  const cases = Array.from({ length: 50 }, (_, i) => generateModStoreCase({ seed: i + 1 }) as unknown as Case)
  const payloads = cases.map((c) => snippet(c).call.replace(/^run\(dkjson\.decode\(/, '').replace(/\)\)$/, ''))
  const code = wrap(snippet(cases[0]!).src) + `local PAYLOADS = {\n${payloads.join(',\n')}\n}\n${PERF_LUA}`
  const lj = luajitEval<{ jit: string; nq: number; nsel: number; fullRuns: number[]; qRuns: number[]; qPasses: number[] }>(code, { roots: resolveRoots() })
  // pob-ts: same 50 cases, same query counts
  const nq = cases.reduce((n, c) => n + c.queries.length, 0)
  const timeLoop = (fn: () => void): number => {
    const t = performance.now(); let passes = 0
    do { fn(); passes++ } while (performance.now() - t < MIN_MS)
    return (performance.now() - t) / passes
  }
  const fullTs = (): number => { const t = performance.now(); for (const c of cases) runCaseOnTs(c as never); return performance.now() - t }
  fullTs(); fullTs()
  const tsFull = Array.from({ length: RUNS }, fullTs)
  const SEL = new Set(['Sum', 'More', 'Flag'])
  const heavy = cases.map((c) => ({ ...c, queries: c.queries.filter((q) => SEL.has(q.method)).flatMap((q) => Array(REPEAT).fill(q)) }))
  const base = cases.map((c) => ({ ...c, queries: [] }))
  const nsel = cases.reduce((n, c) => n + c.queries.filter((q) => SEL.has(q.method)).length, 0)
  const run = (cs: Case[]) => () => { for (const c of cs) runCaseOnTs(c as never) }
  run(heavy)(); run(base)()
  const tsQ = Array.from({ length: RUNS }, () => (timeLoop(run(heavy)) - timeLoop(run(base))) / (REPEAT * nsel)) // ms per query
  const ljFullUs = lj.fullRuns.map((s) => (s * 1e6) / cases.length)
  const tsFullUs = tsFull.map((ms) => (ms * 1e3) / cases.length)
  const ljQNs = lj.qRuns.map((s, r) => (s * 1e9) / (lj.qPasses[r]! * lj.nsel))
  const tsQNs = tsQ.map((ms) => ms * 1e6)
  if (lj.nq !== nq || lj.nsel !== nsel) throw new Error(`query counts differ lua ${lj.nq}/${lj.nsel} vs ts ${nq}/${nsel}`)
  const out = {
    luajit: lj.jit, node: process.version, cases: cases.length, queries: nq, selectedQueries: nsel, runs: RUNS,
    note: 'full = build stores + ops + all queries (+ result serialization) per case; queries-only = Sum/More/Flag via runQuery on prebuilt stores (LuaJIT) / (heavy - base)/REPEAT (TS), loops >= 200 ms; first warm-up pass discarded',
    fullUsPerCase: { luajit: stat(ljFullUs), ts: stat(tsFullUs) },
    queriesNsPerQuery: { luajit: stat(ljQNs), ts: stat(tsQNs) },
  }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-modstore.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(out, null, 2))
}

// --- main -------------------------------------------------------------------------------------------------------
if (!dockerAvailable()) throw new Error('docker not available')
if (process.argv[2] === 'perf') {
  perf()
} else {
  const jits = new Set<string>()
  const small = answer([...PLAIN, ...TAGGED] as Case[], 100, jits)
  const plain = small.slice(0, PLAIN.length)
  const tagged = small.slice(PLAIN.length)
  const fuzzCases = Array.from({ length: FUZZ_SEEDS }, (_, i) => generateModStoreCase({ seed: i + 1 }) as unknown as Case)
  const fuzz = answer(fuzzCases, PER_BATCH, jits)
  if (jits.size !== 1) throw new Error(`several LuaJIT versions: ${[...jits]}`)
  let digest = ''
  try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
  const producer: Json = {
    generator: `pob-ts ${git(POB_TS)}`, image: digest || LUAJIT_IMAGE, luajit: [...jits][0]!,
    pobSha: git(UPSTREAM_POB),
  }
  const report: Record<string, unknown> = {}
  const emit = (name: string, base: string, rows: Row[]): void => {
    report[name] = { ...selfCheck(name, rows), files: writeParts(base, rows, producer).map((f) => ({ ...f, file: f.file.replace(`${ROOT}\\`, '') })) }
  }
  emit('plain', 'modstore-cases', plain)
  emit('tagged', 'modstore-cases-tags', tagged)
  // seeds 1..25 (the pob-ts fuzz.test.ts "607") are the head of the fuzz files below -- not emitted twice (R2.1 review).
  // emit('fuzz-seeds-1-25', 'modstore-fuzz-seeds1-25', fuzz.slice(0, 25)) // the pob-ts fuzz.test.ts corpus (the "607")
  emit('fuzz-1-800', 'modstore-fuzz', fuzz)
  console.log(JSON.stringify({ producer, report }, null, 2))
}
