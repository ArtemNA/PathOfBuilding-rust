// R1: the libm corpus of luajit-numeric.json re-evaluated in the Windows desktop LuaJIT (PoB runtime/lua51.dll, MSVC/UCRT)
// via tools/luajit-win -> crates/pob-lua/tests/fixtures/desktop-libm.json. CRT-dependent math (sin/cos/tan/pow) differs between
// the docker image (Alpine/musl) and the desktop; the desktop is the user's truth (MINEFIELD-RUST R2).
// Usage (PowerShell, Node 24): node tools/lua-corpus/export-libm-desktop.ts
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const POB_RUNTIME = resolve(process.env.POB_ROOT ?? 'C:/pob-work/project_15/PathOfBuilding', 'runtime')
const FIXTURE_IN = join(ROOT, 'crates/pob-lua/tests/fixtures/luajit-numeric.json')
const FIXTURE_OUT = join(ROOT, 'crates/pob-lua/tests/fixtures/desktop-libm.json')
const TOOL = join(ROOT, 'tools/luajit-win')

const rows: string[][] = JSON.parse(readFileSync(FIXTURE_IN, 'utf8')).libm
const lit = (t: string): string =>
  t === 'nan' ? '(0/0)' : t === '-nan' ? '(-(0/0))' : t === 'inf' ? 'math.huge' : t === '-inf' ? '(-math.huge)' : t === '-0.0' ? '(-0.0)' : t

// Same shape as the docker LIBM_BODY: every function evaluated in a hot loop (300 passes) so the desktop JIT compiles it;
// the last pass is reported, the first (interpreted) pass is compared against it and any difference aborts.
const lua = [
  'local F = {}',
  'F["^"] = function(a, b) return a ^ b end',
  'F.pow = function(a, b) return math.pow(a, b) end',
  ...['sqrt', 'log', 'sin', 'cos', 'tan', 'ceil', 'floor', 'abs'].map((f) => `F.${f} = function(a) return math.${f}(a) end`),
  'local R = {',
  ...rows.map((r) => `{"${r[0]}", ${lit(r[1]!)}, ${r[2] === '' ? '0' : lit(r[2]!)}},`),
  '}',
  'local function enc(v) if v ~= v then return "nan" end if v == math.huge then return "inf" end if v == -math.huge then return "-inf" end' +
    ' if v == 0 then return (1 / v < 0) and "-0.0" or "0" end return string.format("%.17g", v) end',
  'local first, O = {}, {}',
  'for i, r in ipairs(R) do first[i] = enc(F[r[1]](r[2], r[3])) end',
  'for w = 1, 300 do for i, r in ipairs(R) do O[i] = enc(F[r[1]](r[2], r[3])) end end',
  'for i = 1, #R do if O[i] ~= first[i] then error("interp/jit differ at row " .. i) end end',
  'return jit.version .. "\\n" .. table.concat(O, "\\n")',
].join('\n')

const tmp = mkdtempSync(join(tmpdir(), 'pob-libm-'))
try {
  writeFileSync(join(tmp, 'libm.lua'), lua)
  execFileSync('cargo', ['build', '-q', '--release', '--manifest-path', join(TOOL, 'Cargo.toml')], { stdio: 'inherit' })
  const exe = join(TOOL, 'target/release/luajit-win.exe')
  const out = execFileSync(exe, [join(tmp, 'libm.lua')], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, PATH: `${POB_RUNTIME};${process.env.PATH ?? ''}` },
  }).split('\n')
  const version = out.shift()!
  if (out.length !== rows.length) throw new Error(`${rows.length} rows -> ${out.length} answers`)
  const dll = createHash('sha256').update(readFileSync(join(POB_RUNTIME, 'lua51.dll'))).digest('hex')
  const lines = rows.map((r, i) => JSON.stringify([r[0], r[1], r[2], out[i]]))
  const producer = { runtime: 'PathOfBuilding/runtime/lua51.dll', lua51Sha256: dll, luajit: version }
  writeFileSync(FIXTURE_OUT, `{"format":1,"producer":${JSON.stringify(producer)},"libm":[\n${lines.join(',\n')}\n]}\n`)
  const differ = rows.filter((r, i) => r[3]!.slice(1) !== out[i]).length
  console.log(`desktop-libm: ${rows.length} rows, ${version}, lua51.dll ${dll.slice(0, 12)}; differ from docker (musl): ${differ}`)
} finally {
  rmSync(tmp, { recursive: true, force: true })
}
