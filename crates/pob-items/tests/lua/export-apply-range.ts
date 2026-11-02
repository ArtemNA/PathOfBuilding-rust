// R-parse-perf: `itemLib.applyRange` on lines that hit `findScalableLine` hard, from REAL LuaJIT (docker, PoB boot). Synthetic lines only.
// Run from the pob-rust root (Node 24):  node crates/pob-items/tests/lua/export-apply-range.ts
//   -> crates/pob-items/tests/fixtures/apply-range-scalable.luajit.json
// Cases: `Unique ID: <hash>` with n runs of digits; every 40th key of data/mod-scalability.json as a line, with and without `+` and ranges,
// over a few scalars; literal `#` and `+#` forms.
import '../../../../tools/upstream.ts'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..')
const OUT = resolve(ROOT, 'crates/pob-items/tests/fixtures/apply-range-scalable.luajit.json')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')

type Case = [line: string, range: number, valueScalar: number | null, baseValueScalar: number | null]
const uniqueId = (n: number): string => 'Unique ID: ' + Array.from({ length: n }, (_, i) => `${i % 10}a`).join('')
const cases: Case[] = []
for (const n of [10, 11, 12, 13, 15]) {
  cases.push([uniqueId(n), 1, 1, null], [uniqueId(n), 1, 1.2, 1.22])
}
for (const line of ['+# to Strength', '#5 fire', '# of #', '+#', '#', '+(10-20) to #', '12 ++# y', '5+3 to 4']) {
  cases.push([line, 1, 1, null], [line, 0.5, 1.2, null])
}
const scalable = JSON.parse(readFileSync(resolve(ROOT, 'data/mod-scalability.json'), 'utf8')) as Record<string, unknown>
const keys = Object.keys(scalable).sort()
keys.forEach((k, i) => {
  if (i % 40 !== 0) return
  cases.push(
    [k.replaceAll('#', '7'), 1, 1, null],
    [k.replaceAll('#', '+12'), 1, 1.3, null],
    [k.replaceAll('#', '(10-20)'), 0.5, 1, 1.22],
    [k.replaceAll('#', '+(3-9)'), 1, 1, 1.22],
    [k.replaceAll('#', '2.5'), 1, 1.5, null],
  )
})

const lit = (s: string): string => JSON.stringify(s)
const num = (v: number | null): string => (v === null ? 'nil' : String(v))
const LUA = `local cases = {\n${cases.map((c) => `{ ${lit(c[0])}, ${c[1]}, ${num(c[2])}, ${num(c[3])} },`).join('\n')}\n}
local out = {}
for i = 1, ${cases.length} do
  local c = cases[i]
  local ok, r = pcall(itemLib.applyRange, c[1], c[2], c[3], c[4])
  out[i] = ok and r or { error = tostring(r) }
end
return { jit = jit.version, out = out }
`
const res = luajitEval<{ jit: string; out: (string | { error: string })[] }>(LUA, { roots: resolveRoots() })
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(
  OUT,
  JSON.stringify({ jit: res.jit, cases: cases.map((c, i) => ({ line: c[0], range: c[1], valueScalar: c[2], baseValueScalar: c[3], result: res.out[i] })) }, null, 1) + '\n',
)
const errors = res.out.filter((r) => typeof r !== 'string').length
console.log(`${cases.length} cases, ${errors} Lua errors, ${res.jit} -> ${OUT}`)
