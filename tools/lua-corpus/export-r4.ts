// R4.1: fixtures for the R4 gate.
//   (a)+(b) <build>.r4-pobts.json  -- full pob-ts `calculate(xml)` output (no allowlist), %.17g strings, env state after perform.
//   (g)     <build>.pools.luajit.json -- the Lua half of pob-ts test/calc/estimations-ehp-oracle.test.ts (PROBE verbatim, :88-118) run in
//           desktop LuaJIT (docker) on build.calcsTab.mainEnv.player after loadBuildFromXML; inputs + actor reads stored for a Rust replay.
//   (perf)  `perf` mode -> tools/.scratch/perf-r4.json: LuaJIT calcs.buildOutput(build,"MAIN") x N in docker (pob-ts/wasm: `npm run bench`).
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-r4.ts [perf]
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { calculate } = await import(`${P}/src/api/engine.ts`)
const { reducePoolsByDamage, takenHitFromDamage } = await import(`${P}/src/calc/pools.ts`)
const { formatG } = await import(`${P}/src/lua/index.ts`)
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const roots = resolveRoots()
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)

// --- docker LuaJIT with the timeless-jewel LUT shim (same as export-env.ts) ---
const JEWEL_SHIM = String.raw`
local jewelDataDir = os.getenv("POB_JEWEL_DATA_DIR")
local rawIoOpen = io.open
if jewelDataDir then
  io.open = function(path, mode)
    local fileName = path:match("Data/TimelessJewelData/([^/\\]+%.bin)$")
    if fileName and (not mode or mode:match("r")) then
      return rawIoOpen(jewelDataDir .. "/" .. fileName, mode)
    end
    return rawIoOpen(path, mode)
  end
end
local headlessNewFileSearch = NewFileSearch
function NewFileSearch(spec, findDirectories)
  if spec:match("Data/TimelessJewelData/.+%.bin$") then
    local file = io.open(spec, "rb")
    if file then
      file:close()
      return { GetFileName = function() return spec:match("([^/\\]+)$") end, GetFileModifiedTime = function() return 1 end }
    end
  end
  return headlessNewFileSearch(spec, findDirectories)
end
`
async function prepareLuts(): Promise<string> {
  const dir = resolve(SCRATCH, 'luts')
  mkdirSync(dir, { recursive: true })
  const { findRequiredJewelLuts } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/timelessJewelLuts.mjs')).href)
  const { unpackJewelData } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/nodeJewelData.mjs')).href)
  for (const b of GOLDEN_BUILDS) {
    for (const { fileStem } of findRequiredJewelLuts(readFileSync(b.xmlPath(roots), 'utf8'))) writeFileSync(resolve(dir, `${fileStem}.bin`), unpackJewelData(roots.pobRoot, fileStem).bytes)
  }
  return dir
}
function ljEval(code: string, lutsDir: string): Rec {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r41-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${lutsDir}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = execFileSync('docker', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 }) } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
const loadChunk = (xml: Buffer, name: string): string =>
  `loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(name)})\nif __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n`

// --- the pools probe: pob-ts test/calc/estimations-ehp-oracle.test.ts:27-118 (inputs and Lua PROBE verbatim) ---
const DMG = ['Physical', 'Lightning', 'Cold', 'Fire', 'Chaos'] as const
const per = (suffixes: readonly string[], types: readonly string[] = DMG): string[] => types.flatMap((t) => suffixes.map((s) => t + s)) // :28
const zeros = (named: Record<string, number>): Record<string, number> => ({ ...named, ...Object.fromEntries(DMG.map((t) => [t, 0])) })
const all = (v: number): Record<string, number> => Object.fromEntries(DMG.map((t) => [t, v]))
const POOL_SETS: Record<string, Rec> = { // :35-43
  'L92/M47': { Life: 92, Mana: 47, EnergyShield: 0, Ward: 0, Aegis: zeros({ shared: 0, sharedElemental: 0 }), Guard: zeros({ shared: 0 }) },
  'L3000/M800/E1611': { Life: 3000, Mana: 800, EnergyShield: 1611, Ward: 0, Aegis: zeros({ shared: 0, sharedElemental: 0 }), Guard: zeros({ shared: 0 }) },
  'L500/M100/E200/W300+aegis+guard': {
    Life: 500, Mana: 100, EnergyShield: 200, Ward: 300,
    Aegis: { shared: 100, sharedElemental: 50, Physical: 10, Lightning: 20, Cold: 30, Fire: 40, Chaos: 5 },
    Guard: { shared: 40, Physical: 20, Lightning: 0, Cold: 0, Fire: 0, Chaos: 0 },
  },
}
const DAMAGE_TABLES: Record<string, Record<string, number>> = { // :44-47
  'Physical 1000': { Physical: 1000 }, 'Chaos 1000': { Chaos: 1000 }, 'all 300': all(300), 'all 10': all(10), 'Physical 1e6': { Physical: 1e6 },
  fractional: { Physical: 33.3, Lightning: 66.6, Cold: 99.9, Fire: 133.2, Chaos: 166.5 }, zero: all(0),
}
const CASES = Object.entries(POOL_SETS).flatMap(([p, pool]) => Object.entries(DAMAGE_TABLES).map(([d, damage]) => ({ name: `${p} x ${d}`, pool, damage }))) // :48
const TAKEN = [100, 5000, 1e6].flatMap((raw) => DMG.map((type) => ({ name: `${type} ${String(raw)}`, raw, type }))) // :49
const FIELDS = ['Ward', 'WardActiveChance', 'EnergyShield', 'Mana', 'Life', 'LifeLossLostOverTime', 'LifeBelowHalfLossLostOverTime', 'OverkillDamage', 'hitPoolRemaining'] as const // :50
const AEGIS_KEYS = ['shared', 'sharedElemental', ...DMG] // :51
const GUARD_KEYS = ['shared', ...DMG] // :52
const POOL_COLUMNS = [...FIELDS, ...DMG.map((t) => `recouped.${t}`), ...DMG.map((t) => `damageTable.${t}`), ...AEGIS_KEYS.map((k) => `Aegis.${k}`), ...GUARD_KEYS.map((k) => `Guard.${k}`)] // :53-54
const TAKEN_COLUMNS = ['sum', ...DMG.map((t) => `damages.${t}`)] // :55
const READS = [ // :58-63
  'Life', 'preventedLifeLoss', 'preventedLifeLossTotal', 'preventedLifeLossBelowHalf', 'sharedMindOverMatter', 'sharedGuardAbsorbRate', 'DamageReductionMax',
  ...per(['MindOverMatter', 'EnergyShieldBypass', 'GuardAbsorbRate', 'takenFlat', 'ResistTakenHitMulti', 'EffectiveAppliedArmour', 'EnemyOverwhelm', 'AfterReductionTakenHitMulti']),
  ...DMG.flatMap((t) => [`Base${t}DamageReductionWhenHit`, `Base${t}DamageReduction`]),
]
const FLAGS = ['Condition:WardNotBreak', 'EnergyShieldProtectsMana', ...DMG.flatMap((t) => [`SelfIgnoreBase${t}DamageReduction`, `SelfIgnore${t}DamageReduction`])] // :64
const SUMS: readonly (readonly ['BASE' | 'MORE', string])[] = [ // :65-68
  ['BASE', 'WardAvoidBreakChance'], ['BASE', 'WardBypassBelowPercent'], ['BASE', 'WardBypass'], ['BASE', 'LifeLossBelowHalfPrevented'], ['MORE', 'VaalArcticArmourMitigation'],
]
function lua(v: unknown): string { // :77-86
  if (typeof v === 'number') { const s = formatG(v, 17); return /[.ein]/.test(s) ? s : `${s}.0` }
  if (typeof v === 'string') return JSON.stringify(v)
  if (Array.isArray(v)) return `{${v.map(lua).join(',')}}`
  if (typeof v === 'object' && v !== null) return `{${Object.entries(v).map(([k, x]) => `[${JSON.stringify(k)}]=${lua(x)}`).join(',')}}`
  throw new Error(`lua(): unsupported ${String(v)}`)
}
// PROBE = estimations-ehp-oracle.test.ts:88-118 verbatim
const PROBE = `
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local function num(v) if v == nil then return "nil" end return RAWF("%.17g", v) end
local function copy(t) local c = {} for k, v in pairs(t) do c[k] = v end return c end
local calcs, actor = build.calcsTab.calcs, build.calcsTab.mainEnv.player
local out = { pools = {}, taken = {}, reads = {}, flags = {}, sums = {}, shift = {} }
for i, c in ipairs(CASES) do
  local damage = copy(c.damage)
  local r = calcs.reducePoolsByDamage({ Life = c.pool.Life, Mana = c.pool.Mana, EnergyShield = c.pool.EnergyShield, Ward = c.pool.Ward,
    Aegis = copy(c.pool.Aegis), Guard = copy(c.pool.Guard), AlliesTakenBeforeYou = {} }, damage, actor)
  local row = {}
  for _, f in ipairs(FIELDS) do row[#row + 1] = num(r[f]) end
  for _, t in ipairs(TYPES) do row[#row + 1] = num(r.damageTakenThatCanBeRecouped[t]) end
  for _, t in ipairs(TYPES) do row[#row + 1] = num(damage[t]) end
  for _, k in ipairs(AEGIS) do row[#row + 1] = num(r.Aegis[k]) end
  for _, k in ipairs(GUARD) do row[#row + 1] = num(r.Guard[k]) end
  out.pools[i] = row
end
for i, c in ipairs(TAKEN) do
  local sum, parts = calcs.takenHitFromDamage(c[1], c[2], actor)
  local row = { num(sum) }
  for _, t in ipairs(TYPES) do row[#row + 1] = num(parts[t]) end
  out.taken[i] = row
end
for i, k in ipairs(READS) do out.reads[i] = num(actor.output[k]) end
for i, k in ipairs(FLAGS) do out.flags[i] = actor.modDB:Flag(nil, k) and "true" or "false" end
for i, s in ipairs(SUMS) do out.sums[i] = num(actor.modDB:Sum(s[1], nil, s[2])) end
for _, from in ipairs(TYPES) do for _, to in ipairs(TYPES) do out.shift[#out.shift + 1] = num(actor.damageShiftTable[from][to]) end end
return out
`
const poolsChunk = (): string =>
  `local CASES = ${lua(CASES.map((c) => ({ pool: c.pool, damage: c.damage })))}\nlocal TAKEN = ${lua(TAKEN.map((c) => [c.raw, c.type]))}\n` +
  `local FIELDS, TYPES, AEGIS, GUARD = ${lua(FIELDS)}, ${lua(DMG)}, ${lua(AEGIS_KEYS)}, ${lua(GUARD_KEYS)}\n` +
  `local READS, FLAGS, SUMS = ${lua(READS)}, ${lua(FLAGS)}, ${lua(SUMS)}\n${PROBE}`

const fmt = (v: unknown): string => (v === undefined ? 'nil' : typeof v === 'number' ? formatG(v, 17) : String(v))
const canon = (s: string): string => (s === '-nan' ? 'nan' : s)
const parseNum = (s: string): number | undefined => (s === 'nil' ? undefined : s === 'inf' ? Infinity : s === '-inf' ? -Infinity : s === 'nan' || s === '-nan' ? NaN : Number(s))
function poolRow(actor: Rec, c: (typeof CASES)[number]): string[] {
  const damage = { ...c.damage }
  const r = reducePoolsByDamage({ Life: c.pool.Life, Mana: c.pool.Mana, EnergyShield: c.pool.EnergyShield, Ward: c.pool.Ward, Aegis: { ...c.pool.Aegis }, Guard: { ...c.pool.Guard }, AlliesTakenBeforeYou: {} }, damage, actor)
  return [...FIELDS.map((f) => fmt(r[f])), ...DMG.map((t) => fmt(r.damageTakenThatCanBeRecouped[t])), ...DMG.map((t) => fmt(damage[t])), ...AEGIS_KEYS.map((k) => fmt(r.Aegis[k])), ...GUARD_KEYS.map((k) => fmt(r.Guard[k]))]
}
function takenRow(actor: Rec, c: (typeof TAKEN)[number]): string[] {
  const [sum, damages] = takenHitFromDamage(c.raw, c.type, actor)
  return [fmt(sum), ...DMG.map((t) => fmt(damages[t]))]
}

// --- deterministic JSON ---
function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
function writeFix(slug: string, kind: string, obj: Rec): { file: string; bytes: number } {
  mkdirSync(FIX, { recursive: true })
  const text = `{\n${Object.keys(obj).filter((k) => obj[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(obj[k])}`).join(',\n')}\n}\n`
  if (Buffer.byteLength(text) > MAX_BYTES) throw new Error(`${slug}.${kind}: ${Buffer.byteLength(text)} bytes > 1.5 MB`)
  const file = resolve(FIX, `${slug}.${kind}.json`)
  writeFileSync(file, text, 'utf8')
  return { file, bytes: statSync(file).size }
}
const XML_OF = (name: string): Buffer => readFileSync(GOLDEN_BUILDS.find((b: Rec) => b.name === name).xmlPath(roots))
const sortedRecord = (r: Rec, f: (v: any) => unknown): Rec => Object.fromEntries(Object.keys(r).sort(byteOrder).map((k) => [k, f(r[k])])) // eslint-disable-line

async function fixtures(): Promise<void> {
  const luts = await prepareLuts()
  let digest = ''
  try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
  const generator = `pob-ts ${git('C:/pob-work/pob-ts')}`
  const report: Rec = {}
  for (const b of GOLDEN_BUILDS) {
    const xmlBuf = XML_OF(b.name)
    const slug = b.name.replace(/ /g, '-')
    const head = { build: b.name, format: 1, xmlSha256: createHash('sha256').update(xmlBuf).digest('hex') }
    // (a)+(b) pob-ts
    const res = calculate(xmlBuf.toString('utf8'))
    const env = res.env
    const stats: Rec = {}
    const negZero: string[] = []
    for (const k of Object.keys(res.stats).sort(byteOrder)) {
      const v = (res.stats as Rec)[k]
      if (typeof v === 'number') { if (Object.is(v, -0)) negZero.push(k); stats[k] = formatG(v, 17) } else stats[k] = v
    }
    const conds = (a: Rec): Rec => sortedRecord(a.modDB.conditions, (x) => x)
    const mults = (a: Rec): Rec => sortedRecord(a.modDB.multipliers, (x) => (typeof x === 'number' ? formatG(x, 17) : x))
    const f1 = writeFix(slug, 'r4-pobts', { ...head, producer: { generator }, stats, negZero, meta: res.meta, unimplemented: res.unimplemented,
      playerConditions: conds(env.player), playerMultipliers: mults(env.player), enemyConditions: conds(env.enemy), enemyMultipliers: mults(env.enemy) })
    // (g) LuaJIT pools
    const raw = ljEval(loadChunk(xmlBuf, b.name) + poolsChunk(), luts)
    const actor: Rec = { ...env.player, output: { ...env.player.output } }
    const reads: Rec = {}
    READS.forEach((k, i) => {
      reads[k] = raw.reads[i]
      const v = parseNum(raw.reads[i])
      if (v === undefined) delete actor.output[k]; else actor.output[k] = v
    })
    let exact = 0
    let total = 0
    const bad: string[] = []
    const chk = (label: string, cols: readonly string[], theirs: readonly string[], ours: readonly string[]): void =>
      cols.forEach((c, k) => { total++; if (canon(ours[k]!) === canon(theirs[k]!)) exact++; else bad.push(`${label}.${c}: pob-ts ${ours[k]} != luajit ${theirs[k]}`) })
    CASES.forEach((c, i) => chk(`reducePools[${c.name}]`, POOL_COLUMNS, raw.pools[i], poolRow(actor, c)))
    TAKEN.forEach((c, i) => chk(`takenHit[${c.name}]`, TAKEN_COLUMNS, raw.taken[i], takenRow(actor, c)))
    const readDiffs = READS.filter((k, i) => fmt(env.player.output[k]) !== canon(raw.reads[i]))
    const f2 = writeFix(slug, 'pools.luajit', { ...head,
      producer: { generator, image: digest || LUAJIT_IMAGE, luajit: 'desktop LuaJIT docker', pobSha: git('C:/pob-work/project_15/PathOfBuilding'),
        probe: 'pob-ts test/calc/estimations-ehp-oracle.test.ts:27-118 (Lua PROBE verbatim; actor = build.calcsTab.mainEnv.player)' },
      inputs: { cases: CASES.map((c) => ({ name: c.name, pool: c.pool, damage: c.damage })), taken: TAKEN.map((c) => ({ name: c.name, raw: String(c.raw), type: c.type })),
        poolColumns: POOL_COLUMNS, takenColumns: TAKEN_COLUMNS, reads: READS, flags: FLAGS, sums: SUMS.map(([kind, n]) => `${kind}:${n}`), types: DMG },
      results: { pools: raw.pools, taken: raw.taken }, actorReads: { reads, flags: raw.flags, sums: raw.sums, shift: raw.shift } })
    report[b.name] = { statsKeys: Object.keys(stats).length, negZero: negZero.length, unimplemented: res.unimplemented.length,
      playerConds: Object.keys(conds(env.player)).length, playerMults: Object.keys(mults(env.player)).length,
      enemyConds: Object.keys(conds(env.enemy)).length, enemyMults: Object.keys(mults(env.enemy)).length,
      pools: { calls: CASES.length + TAKEN.length, fields: total, pobtsBitExactWithLuajitReads: exact, mismatches: bad.slice(0, 5) }, readsDifferingPobtsVsLuajit: readDiffs.length,
      files: [f1, f2].map((f) => `${f.file.replace(`${ROOT}\\`, '')} ${f.bytes}`) }
    console.error(`${b.name}: done`)
  }
  console.log(JSON.stringify(report, null, 1))
}

async function perf(): Promise<void> {
  const luts = await prepareLuts()
  const RUNS = 7
  const out: Rec = { runs: RUNS, note: 'LuaJIT: calcs.buildOutput(build,"MAIN"), os.clock CPU ms, jit.on(), 1 warmup; full buildOutput incl. skills', builds: {} }
  for (const b of GOLDEN_BUILDS) {
    const code = `jit.on()\nlocal t0 = os.clock()\n` + loadChunk(XML_OF(b.name), b.name) + `local loadS = os.clock() - t0\ncollectgarbage()\nlocal calcs = build.calcsTab.calcs\n` +
      `calcs.buildOutput(build, "MAIN")\nlocal r, w = {}, {}\nfor i = 1, ${RUNS} do local t = os.clock(); calcs.buildOutput(build, "MAIN"); r[i] = (os.clock() - t) * 1000 end\n` +
      `for i = 1, ${RUNS} do local t = os.clock(); wipeGlobalCache(); calcs.buildOutput(build, "MAIN"); w[i] = (os.clock() - t) * 1000 end\n` +
      'return { jit = jit.version, status = jit.status() and "on" or "off", load_ms = loadS * 1000, ms = r, wms = w }\n'
    const r = ljEval(code, luts)
    const s = [...(r.ms as number[])].sort((x, y) => x - y)
    const sw = [...(r.wms as number[])].sort((x, y) => x - y)
    out.builds[b.name] = { luajit: r.jit, jit: r.status, loadBuildFromXML_ms: r.load_ms, buildOutput_ms: r.ms, median_ms: s[Math.floor(s.length / 2)], min_ms: s[0], max_ms: s[s.length - 1],
      wipeCache_buildOutput_ms: r.wms, wipeCache_median_ms: sw[Math.floor(sw.length / 2)], wipeCache_min_ms: sw[0], wipeCache_max_ms: sw[sw.length - 1] }
    console.error(`perf ${b.name}: ${out.builds[b.name].median_ms.toFixed(1)} ms`)
  }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-r4.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(out, null, 2))
}

if (process.argv[2] === 'perf') await perf()
else await fixtures()
