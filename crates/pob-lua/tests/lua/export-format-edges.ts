// R1-perf: `tostring` / `string.format` of edge doubles from REAL LuaJIT, both of them: docker (PoB boot) and the desktop lua51.dll
// of the sg-oracle sandbox through tools/luajit-win. All values are synthetic, from a fixed seed.
// Run from the pob-rust root (Node 24):  node crates/pob-lua/tests/lua/export-format-edges.ts
//   -> crates/pob-lua/tests/fixtures/luajit-format-edges.json
// Values travel as raw bits (the Lua side rebuilds them through an ffi union), so no decimal parser stands between us and the double.
// Cases: specials, subnormals, neighbours of powers of ten, rounding carries, exact decimal ties for %.14g, %.Nf and at 10^9 and above,
// near ties in every decade, the ends of the rescale range, tiny and huge values, stat-like values, random bits.
//
// LuaJIT reads uninitialised stack on some %.14g values (lj_strfmt_num.c:465), so the answer can depend on the call before it. Every
// value is therefore asked after several preceding calls, JIT on and off, in both LuaJITs. A slot with more than one answer is written
// as the set of answers; the marking is this measurement alone, never a formula or a range.
import '../../../../tools/upstream.ts'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..')
const OUT = resolve(ROOT, 'crates/pob-lua/tests/fixtures/luajit-format-edges.json')
// The junction-free main checkout holds the sandbox; a worktree has neither the runtime nor the built tool.
const MAIN = process.env.POB_RUST_MAIN ?? 'C:/pob-work/pob-rust'
const RUNTIME = resolve(MAIN, 'tools/sg-oracle/runtime')
const EXE = resolve(MAIN, 'tools/luajit-win/target/release/luajit-win.exe')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
if (!existsSync(EXE) || !existsSync(join(RUNTIME, 'lua51.dll'))) throw new Error(`no desktop LuaJIT at ${EXE} / ${RUNTIME} -- STOP`)

// Fixed-seed SplitMix64.
let state = 0x5eed_0001n
const MASK = (1n << 64n) - 1n
function next64(): bigint {
  state = (state + 0x9e3779b97f4a7c15n) & MASK
  let z = state
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK
  return z ^ (z >> 31n)
}
const unit = (): number => Number(next64() >> 11n) / 2 ** 53
const below = (n: number): number => Math.floor(unit() * n)
const logUniform = (lo: number, hi: number): number => Math.exp(Math.log(lo) + unit() * (Math.log(hi) - Math.log(lo)))
const digits = (n: number): string => {
  let r = String(1 + below(9))
  for (let i = 1; i < n; i++) r += String(below(10))
  return r
}

const view = new DataView(new ArrayBuffer(8))
function bitsOf(v: number): bigint {
  view.setFloat64(0, v)
  return view.getBigUint64(0)
}
function fromBits(b: bigint): number {
  view.setBigUint64(0, b)
  return view.getFloat64(0)
}
const step = (v: number, ulps: number): number => (v === 0 ? 0 : fromBits(bitsOf(v) + BigInt(ulps)))

const bits = new Set<bigint>()
const add = (...vs: number[]): void => {
  for (const v of vs) bits.add(bitsOf(v))
}
const addNear = (v: number, ulps: number, withNeg = false): void => {
  for (let u = -ulps; u <= ulps; u++) {
    add(step(v, u))
    if (withNeg) add(-step(v, u))
  }
}

// Specials: both zeros, both NaN signs, infinities, the ends of the subnormal and normal ranges.
bits.add(0x7ff8000000000000n).add(0xfff8000000000000n).add(0x7ff0000000000001n)
add(0, -0, Infinity, -Infinity, 1, -1)
for (const b of [1n, 2n, 0x000fffffffffffffn, 0x0010000000000000n, 0x7fefffffffffffffn, 0x3ff0000000000001n, 0x3fefffffffffffffn]) {
  bits.add(b).add(b | (1n << 63n))
}
for (let i = 0; i < 25; i++) bits.add(next64() >> 12n) // subnormals

// Powers of ten and their neighbours; 1e-4/1e-5 and 1e13..1e16 are the %.14g layout boundaries.
for (let k = -30; k <= 30; k++) addNear(Number(`1e${k}`), 2, Math.abs(k) < 8)
for (let k = 31; k <= 308; k += 23) {
  addNear(Number(`1e${k}`), 1)
  addNear(Number(`1e${-k}`), 1)
}

// Rounding carries: 10^k * (1 - 0.5e-p) rounds to 10^k at p significant digits.
// These also get the width forms: the carry changes the digit count that lj_strfmt_num.c:485 reads for its trailing-zero strip.
const withWidth = new Set<bigint>()
for (const p of [14, 6, 15, 1]) {
  for (let k = -12; k <= 22; k += 2) {
    const c = Number(`1e${k}`) * (1 - 0.5 * Number(`1e${-p}`))
    addNear(c, 1)
    for (const u of [-1, 0, 1]) withWidth.add(bitsOf(step(c, u)))
  }
}
for (const h of ['43abc16d674ec7ff']) {
  bits.add(BigInt(`0x${h}`))
  withWidth.add(BigInt(`0x${h}`))
}
for (const v of [0.5, 0.05, 0.005, 0.0005, 9.5, 99.5, 999.5, 9.95, 99.95, 9.995, 99.995, 0.995, 0.095, 0.0095, 999999.5, 9999999999.5, 1.5, 2.5, 3.5]) {
  addNear(v, 1, true)
}

// Exact %.14g ties: K / 2^j with K odd and K * 5^j having exactly 15 digits.
for (let j = 1; j <= 21; j++) {
  const pow5 = 5n ** BigInt(j)
  const lo = 10n ** 14n / pow5 + 1n
  const hi = 10n ** 15n / pow5
  for (let i = 0; i < 6; i++) {
    const K = (lo + (next64() % (hi - lo + 1n))) | 1n
    if (K * pow5 >= 10n ** 15n || K >= 1n << 53n) continue
    add(Number(K) / 2 ** j)
  }
}
// Exact integer ties: 15 digits ending in 5, 16 digits ending in 50 (exact below 2^53).
for (let i = 0; i < 25; i++) {
  add(Number((next64() % (9n * 10n ** 13n) + 10n ** 13n) * 10n + 5n))
  add(Number((next64() % (9n * 10n ** 13n) + 10n ** 13n) * 100n + 50n))
}
// Exact %.Nf ties: odd K / 2^(N+1).
for (const [den, n] of [[2, 40], [4, 30], [8, 80], [16, 30], [64, 15], [1024, 15]] as const) {
  for (let i = 0; i < n; i++) add((below(i < n / 2 ? 2000 : 2 ** 40) * 2 + 1) / den)
}

// lj_strfmt_num.c:271: exact ties with the tie digit at 10^k, both sides of k = 9, with an even and an odd digit before it.
for (const p of [1, 3, 6, 9]) {
  for (const k of [7, 8, 9, 10, 11, 13, 16, 20]) {
    for (let i = 0; i < 8; i++) {
      const head = BigInt(digits(p)) // the printed digits
      for (const last of [0n, 1n]) {
        const d = ((head - (head % 2n) + last) * 10n + 5n) * 10n ** BigInt(k)
        if (BigInt(Number(d)) === d) addNear(Number(d), i === 0 ? 1 : 0)
      }
    }
  }
}

// lj_strfmt_num.c:400: near ties at 14, 6 and 1 digits in every decade (the rescale range and inside it), and its ends.
// (Above 1e40 fewer: each huge value costs six 300-digit %f strings of the size limit.)
for (let x = -300; x <= 40; x += 3) addNear(Number(`${digits(14)}5e${x - 14}`), 1)
for (let x = 41; x <= 300; x += 8) add(Number(`${digits(14)}5e${x - 14}`))
for (let x = -30; x <= 30; x++) add(Number(`${digits(14)}5e${x - 14}`))
for (let x = -200; x <= 200; x += 25) {
  add(Number(`${digits(6)}5e${x - 6}`))
  add(Number(`${digits(1)}5e${x - 1}`))
}
for (const e of [-127, -128, -129, -76, -75, -74, -64, -63, -62, 46, 47, 52, 53, 54, 63, 64, 65, 75, 120, 127, 128, 129, 130]) addNear(2 ** e, 2)
// Values a census found to depend on the preceding call. Only candidates: whether they are marked is measured below like any other.
const CANDIDATES = `36da470f69b38603 22e9770be9307bf5 32ee058423d16d89 72d6e97c3795572b 3b03059101cea8ea 3afafb1625165349 3b084b87de7fdb52 3b08e28eb9095f3f
3b01f31909e83391 3b08c672a3f9ac03 3af44624623ce53a 3b05acabfe9a034d 3b06c827310c8f5b 3b018fa58e909110 3b0217937096d198 3b0a2860807096bc 3af692c7df4cb9d0
3b0a8a853a31175b 3b096a86c42112e8 3af9c89e61954315 3b098903581662b4 3af3ff0bfe4335db 3b0a39baa4b642b5 3afa1b4ef400849c 3b004cf94f749ab0 3b09923b9e03943c
3af3913de1fbccb3 3af8716ea7685cd8 3b04d65cfd0e8344 3b05c3f1be0df8d7 3b056c7201a49ec2 3af91685670f20a4 3af54b5f2ce24691 3b03fcadff3edea5 3b08c10dd2af2aba
3b0406a19a169352 3b0043bf0edb1110 3b0888f619e3d719 3b018d27596fe88b 3b03e5896a6f9256 3b06b82dc9218d82 3b0608aaa4662d64 3afb2ba3d21cdae9 3b09808a933a0533
3b097d9866c7ac82 3b006c2509bef726 3b0aa710e9e83d48 6ee3a6efecbc5cc5 4eec2f2e5eecb3bc 1ad658859f510af7 4ee60a07ba433f8e 5ef116426f512821 77077c1ce68295ea
2efc6043b0b2a8ce 0f02b8a055eacd71 06eca341623d1e3a 6afc09c4c45779d6 06eacb159b083c4d 56d74d8278911c92 16ea8230f90a82bb 0adb44fea9b7a4b9 2af69123f13b1e68
5af21d7d48eda371 36df037aa2b6eeb2 0ae043ea48174746 5afa4ea915db8272 2efd404e15eed86f 6aeb4d7e4eb6c709 67046790a0b55572 6af4242c885be587 32e4b5a8fc6a718e
2ef713f67a340935`
for (const h of CANDIDATES.split(/\s+/)) bits.add(BigInt(`0x${h}`))

// The integer-division range, tiny values.
for (let i = 0; i < 100; i++) add(logUniform(1e14, 18446744073709551616))
for (let i = 0; i < 80; i++) add(logUniform(1e-30, 1e-9))

// Stat-like values.
for (let i = 0; i < 50; i++) add(below(10_000_000))
for (let i = 0; i < 100; i++) add(below(100_000_000) / 100)
for (let i = 0; i < 50; i++) add(below(1_000_000) / 10)
for (let i = 0; i < 50; i++) add(-below(1_000_000) / 1000)
for (let i = 0; i < 120; i++) add(logUniform(0.01, 1e9) * (1 + below(50) / 100))

for (let i = 0; i < 70; i++) bits.add(next64())

const values = [...bits]
const hex = values.map((b) => b.toString(16).padStart(16, '0')).join('')
const FMTS = ['%.14g', '%.0f', '%.1f', '%.2f', '%.3f', '%f', '%g', '%.1g', '%.3g', '%.9g', '%.15g', '%.17g', '%.20f']
const WIDTH_FMTS = ['%+.2f', '%-12.3f', '%012.2f', '% .1f', '%5.0f', '%+g', '%08.3g', '%-12g', '%14.14g', '%9.6g', '%5g']
const luaList = (l: string[]): string => `{ ${l.map((s) => JSON.stringify(s)).join(', ')} }`
// Preceding calls, run in this order with no table write between a call and the answer (a write reaches the same stack): nothing; two that
// fill the stack with digits; a plain one, which then follows the 1e300 fill and an answer; that sequence directly; a short %f.
const HISTORIES = [
  '',
  'fmt("%.99f", 1.5)',
  'fmt("%.99f", 1e300)',
  'fmt("%s %d", "x", 7)',
  'fmt("%.99f", 1e300) fmt("%s %d", "x", 7)',
  'fmt("%.20f", 1 / 3)',
]
const wideFlags = values.map((b, i) => ((i + 1) % 8 === 0 || withWidth.has(b) ? '1' : '0')).join('')
const LUA = `local ffi = require("ffi")
local u = ffi.new("union { double d; uint32_t w[2]; }")
local hex = ${JSON.stringify(hex)}
local fmt = string.format
local fmts = ${luaList(FMTS)}
local wfmts = ${luaList(WIDTH_FMTS)}
local wide = ${JSON.stringify(wideFlags)}
local rows = {}
local function ask(slots, k, v, f)
  local seen = slots[k]
  if not seen then seen = {}; slots[k] = seen end
  if f then
    ${HISTORIES.map((h, i) => `${h} local r${i} = fmt(f, v)`).join('\n    ')}
    ${HISTORIES.map((_, i) => `seen[r${i}] = true`).join(' ')}
  else
    ${HISTORIES.map((h, i) => `${h} local r${i} = tostring(v)`).join('\n    ')}
    ${HISTORIES.map((_, i) => `seen[r${i}] = true`).join(' ')}
  end
end
local function pass()
  for i = 1, ${values.length} do
    local p = (i - 1) * 16
    u.w[1] = tonumber(hex:sub(p + 1, p + 8), 16)
    u.w[0] = tonumber(hex:sub(p + 9, p + 16), 16)
    local v = u.d
    local slots = rows[i]
    if not slots then slots = {}; rows[i] = slots end
    ask(slots, 1, v, nil)
    for k = 1, #fmts do ask(slots, k + 1, v, fmts[k]) end
    if wide:sub(i, i) == "1" then
      for k = 1, #wfmts do ask(slots, #fmts + 1 + k, v, wfmts[k]) end
    end
  end
end
pass()
jit.off()
pass()
local out = {}
for i = 1, #rows do
  local cols = {}
  for k = 1, #rows[i] do
    local l = {}
    for s in pairs(rows[i][k]) do l[#l + 1] = s end
    table.sort(l)
    cols[k] = table.concat(l, "~")
  end
  out[i] = table.concat(cols, "|")
end
return jit.version .. "\\n" .. table.concat(out, "\\n")
`
const docker = (luajitEval<string>(LUA, { roots: resolveRoots() })).split('\n')
const tmp = mkdtempSync(join(tmpdir(), 'pob-format-edges-'))
let desktop: string[]
try {
  writeFileSync(join(tmp, 'edges.lua'), LUA)
  desktop = execFileSync(EXE, [join(tmp, 'edges.lua')], {
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 1024,
    env: { ...process.env, PATH: `${RUNTIME};${process.env.PATH ?? ''}` },
  }).split('\n')
} finally {
  rmSync(tmp, { recursive: true, force: true })
}
if (docker[0] !== desktop[0]) throw new Error(`two LuaJITs: ${docker[0]} / ${desktop[0]} -- STOP`)

// Merge the two answers per slot, count what is not a single string, and where.
const names = ['tostring', ...FMTS, ...WIDTH_FMTS]
const census: Record<string, { inZone: number; outOfZone: number }> = {}
let setsDiffer = 0
const rows = values.map((b, i) => {
  const a = docker[i + 1]!.split('|')
  const c = desktop[i + 1]!.split('|')
  if (a.length !== c.length) throw new Error(`row ${i}: ${a.length} / ${c.length} slots`)
  const abs = Math.abs(fromBits(b))
  const rescale = abs !== 0 && Number.isFinite(abs) && (abs < 2 ** -63 || abs >= 2 ** 129)
  const slots = a.map((s, k) => {
    if (s !== c[k]) setsDiffer++
    const set = [...new Set([...s.split('~'), ...c[k]!.split('~')])].sort()
    if (set.length === 1) return set[0]!
    const e = (census[names[k]!] ??= { inZone: 0, outOfZone: 0 })
    if (rescale) e.inZone++
    else e.outOfZone++
    return set
  })
  return JSON.stringify([b.toString(16).padStart(16, '0'), ...slots])
})
mkdirSync(dirname(OUT), { recursive: true })
const head = { jit: docker[0], histories: HISTORIES, passes: ['jit on', 'jit off'], luajits: ['docker', 'desktop'], fmts: FMTS, widthFmts: WIDTH_FMTS }
writeFileSync(OUT, `${JSON.stringify(head).slice(0, -1)},"rows":[\n${rows.join(',\n')}\n]}\n`)
console.log(`${values.length} values, ${docker[0]}, ${HISTORIES.length} histories x 2 passes x 2 LuaJITs -> ${OUT}`)
console.log(`slots whose answer set differs between docker and desktop: ${setsDiffer}`)
console.log(`slots with several answers (inZone = |x| < 2^-63 or >= 2^129): ${JSON.stringify(census)}`)
