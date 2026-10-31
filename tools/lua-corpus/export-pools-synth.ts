// R4.4c: synthetic-actor corpus for the CalcDefence.lua pool helpers, in desktop LuaJIT (docker).
// The 6 golden builds leave every Mind-over-Matter / prevented-loss / ally / ward-bypass / output-fallback branch dead (R4.4a);
// here ONE Dual Savior instance is loaded once and each scenario runs the R4.1 pools PROBE on a synthetic actor
// {output = copy of mainEnv.player.output + overrides, modDB = fresh ModDB with "Synth" mods, damageShiftTable = copy (+ row overrides)}.
// Output: crates/pob-calc/tests/fixtures/pools-synth.luajit.json  { scenarios: [ { name, covers, inputs, results, actorReads } ] }
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-pools-synth.ts
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { reducePoolsByDamage, takenHitFromDamage } = await import(`${P}/src/calc/pools.ts`)
const { formatG } = await import(`${P}/src/lua/index.ts`)
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-calc/tests/fixtures/pools-synth.luajit.json')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const roots = resolveRoots()
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const BUILD = 'Dual Savior'

// --- docker LuaJIT with the timeless-jewel LUT shim (same as export-r4.ts) ---
const JEWEL_SHIM = String.raw`
local jewelDataDir = os.getenv("POB_JEWEL_DATA_DIR")
local rawIoOpen = io.open
if jewelDataDir then
  io.open = function(path, mode)
    local fileName = path:match("Data/TimelessJewelData/([^/\\]+%.bin)$")
    if fileName and (not mode or mode:match("r")) then return rawIoOpen(jewelDataDir .. "/" .. fileName, mode) end
    return rawIoOpen(path, mode)
  end
end
local headlessNewFileSearch = NewFileSearch
function NewFileSearch(spec, findDirectories)
  if spec:match("Data/TimelessJewelData/.+%.bin$") then
    local file = io.open(spec, "rb")
    if file then file:close(); return { GetFileName = function() return spec:match("([^/\\]+)$") end, GetFileModifiedTime = function() return 1 end } end
  end
  return headlessNewFileSearch(spec, findDirectories)
end
`
async function prepareLuts(): Promise<string> {
  const dir = resolve(SCRATCH, 'luts')
  mkdirSync(dir, { recursive: true })
  const { findRequiredJewelLuts } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/timelessJewelLuts.mjs')).href)
  const { unpackJewelData } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/nodeJewelData.mjs')).href)
  const b = GOLDEN_BUILDS.find((x: Rec) => x.name === BUILD)
  for (const { fileStem } of findRequiredJewelLuts(readFileSync(b.xmlPath(roots), 'utf8'))) writeFileSync(resolve(dir, `${fileStem}.bin`), unpackJewelData(roots.pobRoot, fileStem).bytes)
  return dir
}
function ljEval(code: string, lutsDir: string): Rec {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r44c-'))
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
function lua(v: unknown): string {
  if (typeof v === 'number') { const s = formatG(v, 17); return /[.ein]/.test(s) ? s : `${s}.0` }
  if (typeof v === 'boolean') return String(v)
  if (typeof v === 'string') return JSON.stringify(v)
  if (Array.isArray(v)) return `{${v.map(lua).join(',')}}`
  if (typeof v === 'object' && v !== null) return `{${Object.entries(v).map(([k, x]) => `[${JSON.stringify(k)}]=${lua(x)}`).join(',')}}`
  throw new Error(`lua(): unsupported ${String(v)}`)
}

// --- shared vocabulary (same columns as R4.1, + Allies.<key> remaining) ---
const DMG = ['Physical', 'Lightning', 'Cold', 'Fire', 'Chaos'] as const
const per = (suffixes: readonly string[]): string[] => DMG.flatMap((t) => suffixes.map((s) => t + s))
const ALLY_KEYS = ['minion', 'radianceSentinel', 'spectres', 'totems', 'vaalRejuvenationTotems', 'voidSpawn', 'stoneGolem', 'soulLink', 'frostShield']
const ALLY_OUT = ['TotalMinionLife', 'MinionAllyDamageMitigation', 'TotalRadianceSentinelLife', 'RadianceSentinelAllyDamageMitigation', 'TotalSpectreLife', 'SpectreAllyDamageMitigation',
  'TotalTotemLife', 'TotemAllyDamageMitigation', 'TotalVaalRejuvenationTotemLife', 'VaalRejuvenationTotemAllyDamageMitigation', 'TotalVoidSpawnLife', 'VoidSpawnAllyDamageMitigation',
  'TotalStoneGolemLife', 'StoneGolemAllyDamageMitigation', 'AlliedEnergyShield', 'SoulLinkMitigation', 'FrostShieldLife', 'FrostShieldDamageMitigation']
const FIELDS = ['Ward', 'WardActiveChance', 'EnergyShield', 'Mana', 'Life', 'LifeLossLostOverTime', 'LifeBelowHalfLossLostOverTime', 'OverkillDamage', 'hitPoolRemaining']
const AEGIS_KEYS = ['shared', 'sharedElemental', ...DMG]
const GUARD_KEYS = ['shared', ...DMG]
const POOL_COLUMNS = [...FIELDS, ...DMG.map((t) => `recouped.${t}`), ...DMG.map((t) => `damageTable.${t}`), ...AEGIS_KEYS.map((k) => `Aegis.${k}`), ...GUARD_KEYS.map((k) => `Guard.${k}`),
  ...ALLY_KEYS.map((k) => `Allies.${k}`)]
const TAKEN_COLUMNS = ['sum', ...DMG.map((t) => `damages.${t}`)]
const READS = [...new Set([ // R4.1 READS + every fallback / ally key the helpers can read
  'Life', 'preventedLifeLoss', 'preventedLifeLossTotal', 'preventedLifeLossBelowHalf', 'sharedMindOverMatter', 'sharedGuardAbsorbRate', 'DamageReductionMax',
  ...per(['MindOverMatter', 'EnergyShieldBypass', 'GuardAbsorbRate', 'takenFlat', 'ResistTakenHitMulti', 'EffectiveAppliedArmour', 'EnemyOverwhelm', 'AfterReductionTakenHitMulti']),
  ...DMG.flatMap((t) => [`Base${t}DamageReductionWhenHit`, `Base${t}DamageReduction`]),
  'sharedAegis', 'sharedElementalAegis', 'sharedGuardAbsorb', ...DMG.flatMap((t) => [`${t}Aegis`, `${t}GuardAbsorb`]),
  'Ward', 'EnergyShieldRecoveryCap', 'ManaUnreserved', 'LifeRecoverable', ...ALLY_OUT])]
const FLAGS = ['Condition:WardNotBreak', 'EnergyShieldProtectsMana', ...DMG.flatMap((t) => [`SelfIgnoreBase${t}DamageReduction`, `SelfIgnore${t}DamageReduction`])]
const SUMS: readonly (readonly ['BASE' | 'MORE', string])[] = [
  ['BASE', 'WardAvoidBreakChance'], ['BASE', 'WardBypassBelowPercent'], ['BASE', 'WardBypass'], ['BASE', 'LifeLossBelowHalfPrevented'], ['MORE', 'VaalArcticArmourMitigation'],
]
const TAKEN = [100, 5000, 1e6].flatMap((raw) => DMG.map((type) => ({ name: `${type} ${String(raw)}`, raw, type })))

// --- pools and damage tables ---
const allN = (v: number): Rec => Object.fromEntries(DMG.map((t) => [t, v]))
const aegisZ = { shared: 0, sharedElemental: 0, ...allN(0) }
const guardZ = { shared: 0, ...allN(0) }
const pool = (o: Rec): Rec => ({ Aegis: aegisZ, Guard: guardZ, Allies: {}, ...o }) // complete table unless overridden
const POOL: Record<string, Rec | false> = {
  A: pool({ Life: 3000, Mana: 800, EnergyShield: 1611, Ward: 0 }),
  B: pool({ Life: 500, Mana: 100, EnergyShield: 200, Ward: 300, Aegis: { shared: 100, sharedElemental: 50, Physical: 10, Lightning: 20, Cold: 30, Fire: 40, Chaos: 5 }, Guard: { shared: 40, Physical: 20, Lightning: 0, Cold: 0, Fire: 0, Chaos: 0 } }),
  L: pool({ Life: 1000, Mana: 5000, EnergyShield: 3000, Ward: 500 }),
  W: pool({ Life: 3000, Mana: 800, EnergyShield: 1000, Ward: 300 }),
  N: false, // poolTable = nil: every pool from output
  PART: { Life: 2000 }, // allies/Mana/ES/Ward/Aegis/Guard all from output (Aegis/Guard built for the types in the hit)
  NEG: pool({ Life: -50, Mana: -1, EnergyShield: -10, Ward: -5 }),
  ZERO: pool({ Life: 0, Mana: 0, EnergyShield: 0, Ward: 0 }),
  HUGE: pool({ Life: 1e18, Mana: 1e18, EnergyShield: 1e18, Ward: 1e18 }),
}
const DAMAGE: Record<string, Rec> = {
  'Physical 1000': { Physical: 1000 }, 'Chaos 1000': { Chaos: 1000 }, 'all 300': allN(300), 'Physical 1e6': { Physical: 1e6 },
  fractional: { Physical: 33.3, Lightning: 66.6, Cold: 99.9, Fire: 133.2, Chaos: 166.5 }, 'mixed big': { Physical: 4000, Fire: 2500, Chaos: 700 }, zero: allN(0),
}
const DN = Object.keys(DAMAGE)
const withAllies = (base: string, allies: Rec): Rec => ({ ...(POOL[base] as Rec), Allies: allies })

interface Scenario { name: string; covers: string[]; output?: Rec; unset?: string[]; mods?: [string, string, number | boolean][]; shiftRows?: Rec; pools: Record<string, Rec | false>; dmg?: string[] }
const S: Scenario[] = [
  { name: 'mom-shared-50', covers: ['CalcDefence.lua:245-264', 'CalcDefence.lua:319-321', 'CalcDefence.lua:331-350'], output: { Life: 3000, sharedMindOverMatter: 50, preventedLifeLoss: 0 }, pools: { A: POOL.A!, L: POOL.L!, N: POOL.N! } },
  { name: 'mom-typed-mix-cap100', covers: ['CalcDefence.lua:251 (shared+typed, cap 100)', 'CalcDefence.lua:331-350'], output: { Life: 3000, sharedMindOverMatter: 40, PhysicalMindOverMatter: 30, FireMindOverMatter: 80, ChaosMindOverMatter: 10 }, pools: { A: POOL.A!, L: POOL.L!, B: POOL.B! } },
  { name: 'mom-100-infinite', covers: ['CalcDefence.lua:251-253 (MoMEffect==1 -> x/0)', 'CalcDefence.lua:331-350'], output: { Life: 3000, sharedMindOverMatter: 100 }, pools: { A: POOL.A!, L: POOL.L!, ZERO: POOL.ZERO! } },
  { name: 'es-protects-mana', covers: ['CalcDefence.lua:257-259', 'CalcDefence.lua:333-342', 'CalcDefence.lua:331-350'], mods: [['EnergyShieldProtectsMana', 'FLAG', true]], output: { Life: 3000, sharedMindOverMatter: 60, FireEnergyShieldBypass: 25, ChaosEnergyShieldBypass: 100 }, pools: { A: POOL.A!, L: POOL.L!, B: POOL.B! } },
  { name: 'es-bypass-no-mom', covers: ['CalcDefence.lua:255-256 (bypass>0)', 'CalcDefence.lua:322-330'], output: { Life: 3000, PhysicalEnergyShieldBypass: 0, FireEnergyShieldBypass: 50, ChaosEnergyShieldBypass: 100, ColdEnergyShieldBypass: 20 }, pools: { A: POOL.A!, L: POOL.L!, B: POOL.B! } },
  { name: 'prevented-no-below-half', covers: ['CalcDefence.lua:351-361', 'CalcDefence.lua:381-385 (BelowHalf==0)'], output: { Life: 3000, preventedLifeLossTotal: 30, preventedLifeLoss: 30, preventedLifeLossBelowHalf: 0 }, pools: { A: POOL.A!, L: POOL.L!, PART: POOL.PART! } },
  { name: 'prevented-below-half', covers: ['CalcDefence.lua:362-380', 'CalcDefence.lua:179-183 (loss prevention pool)'], mods: [['LifeLossBelowHalfPrevented', 'BASE', 25]], output: { Life: 3000, preventedLifeLossTotal: 40, preventedLifeLoss: 40, preventedLifeLossBelowHalf: 20 }, pools: { A: POOL.A!, L: POOL.L!, N: POOL.N! } },
  { name: 'prevented-with-mom-and-es', covers: ['CalcDefence.lua:351-387 x 331-350 x 257-259'], mods: [['EnergyShieldProtectsMana', 'FLAG', true], ['LifeLossBelowHalfPrevented', 'BASE', 30]], output: { Life: 3000, sharedMindOverMatter: 40, preventedLifeLossTotal: 25, preventedLifeLoss: 25, preventedLifeLossBelowHalf: 15 }, pools: { A: POOL.A!, L: POOL.L!, B: POOL.B! } },
  { name: 'allies-explicit', covers: ['CalcDefence.lua:269-279 (ally order, percent, floor)', 'CalcDefence.lua:280-281 (recoup)'], output: { Life: 3000 },
    pools: { A_ALLY: withAllies('A', { minion: { remaining: 500, percent: 0.5 }, totems: { remaining: 200, percent: 1 }, frostShield: { remaining: 50.5, percent: 0.3 }, soulLink: { remaining: 0, percent: 0.5 }, stoneGolem: { remaining: 1000, percent: 0.25 } }),
      B_ALLY: withAllies('B', { spectres: { remaining: 100000, percent: 0.9 }, voidSpawn: { remaining: 10, percent: 0.2 } }) } },
  { name: 'allies-from-output', covers: ['CalcDefence.lua:45-55 buildAllyLifePools', 'CalcDefence.lua:202-205', 'CalcDefence.lua:269-279'],
    output: { Life: 3000, TotalMinionLife: 800, MinionAllyDamageMitigation: 40, TotalSpectreLife: 0, SpectreAllyDamageMitigation: 50, TotalTotemLife: 300, TotemAllyDamageMitigation: 250, FrostShieldLife: 120.5, FrostShieldDamageMitigation: 75, AlliedEnergyShield: 90, TotalVoidSpawnLife: -5, VoidSpawnAllyDamageMitigation: 10 },
    unset: ['SoulLinkMitigation'], pools: { PART: POOL.PART!, N: POOL.N! }, dmg: DN },
  { name: 'ward-bypass', covers: ['CalcDefence.lua:228-231 WardBypassBelowPercent', 'CalcDefence.lua:312-318 WardBypass'], mods: [['WardBypass', 'BASE', 40], ['WardBypassBelowPercent', 'BASE', 20]], output: { Life: 3000 }, pools: { W: POOL.W!, B: POOL.B!, A: pool({ Life: 3000, Mana: 800, EnergyShield: 1611, Ward: 5000 }) } },
  { name: 'ward-not-break', covers: ['CalcDefence.lua:228 Condition:WardNotBreak', 'CalcDefence.lua:408-413 (ward report)'], mods: [['Condition:WardNotBreak', 'FLAG', true]], output: { Life: 3000 }, pools: { W: POOL.W!, B: POOL.B!, WA: { ...(POOL.W as Rec), WardActiveChance: 0.5 } } },
  { name: 'ward-avoid-break-50', covers: ['CalcDefence.lua:228 WardAvoidBreakChance', 'CalcDefence.lua:412-413'], mods: [['WardAvoidBreakChance', 'BASE', 50], ['WardAvoidBreakChance', 'BASE', 80]], output: { Life: 3000 }, pools: { W: POOL.W!, B: POOL.B!, WA: { ...(POOL.W as Rec), WardActiveChance: 0.25 } } },
  { name: 'output-fallbacks', covers: ['CalcDefence.lua:208-224 (Aegis/Guard from output)', 'CalcDefence.lua:226-232 (Ward/ES/Mana/Life from output)', 'CalcDefence.lua:301-311 (guard rate)'],
    output: { Life: 3000, sharedAegis: 100, sharedElementalAegis: 70, PhysicalAegis: 20, FireAegis: 15, sharedGuardAbsorb: 200, PhysicalGuardAbsorb: 50, ChaosGuardAbsorb: 30, PhysicalGuardAbsorbRate: 50, ChaosGuardAbsorbRate: 80, sharedGuardAbsorbRate: 60, Ward: 150, EnergyShieldRecoveryCap: 900, ManaUnreserved: 400, LifeRecoverable: 1200 },
    pools: { N: POOL.N!, PART: POOL.PART! } },
  { name: 'takenflat-and-shift', covers: ['CalcDefence.lua:168 (takenFlat ~= 0)', 'CalcDefence.lua:166-175 (shift 0 + flat, negative flat)', 'CalcDefence.lua:170 (Vaal Arctic)'],
    mods: [['VaalArcticArmourMitigation', 'MORE', -25]], output: { Life: 3000, PhysicaltakenFlat: 50, LightningtakenFlat: -30, ColdtakenFlat: 20, FiretakenFlat: 7.5 },
    shiftRows: { Physical: { Physical: 100, Cold: 0 }, Lightning: { Lightning: 100, Fire: 0 } }, pools: { A: POOL.A!, N: POOL.N! } },
  { name: 'shift-split-hash-order', covers: ['CalcDefence.lua:166 (pairs over a split shift row, float sum order)'],
    output: { Life: 3000 }, shiftRows: { Physical: { Physical: 33.3, Fire: 33.3, Chaos: 33.4 }, Fire: { Fire: 10, Cold: 20, Lightning: 30, Chaos: 40 } }, pools: { A: POOL.A! } },
  { name: 'extreme-pools', covers: ['CalcDefence.lua:239-243 (negative clamp)', 'CalcDefence.lua:226-229 (0 / huge pools)', 'CalcDefence.lua:394-404 (overkill, NaN pools)'],
    output: { Life: 3000, sharedMindOverMatter: 50 }, pools: { NEG: POOL.NEG!, ZERO: POOL.ZERO!, HUGE: POOL.HUGE! } },
  { name: 'nil-es-error', covers: ['CalcDefence.lua:231/241 (EnergyShieldRecoveryCap nil -> runtime error)'], output: { Life: 3000 }, unset: ['EnergyShieldRecoveryCap'], pools: { N: POOL.N!, A: POOL.A! }, dmg: ['Physical 1000'] },
]

// --- the PROBE, per scenario, on a synthetic actor ---
const PROBE = `
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local function num(v) if v == nil then return "nil" end return RAWF("%.17g", v) end
local function copy(t) local c = {} for k, v in pairs(t) do c[k] = v end return c end
local function deep2(t) local c = {} for k, v in pairs(t) do c[k] = copy(v) end return c end
local calcs, base = build.calcsTab.calcs, build.calcsTab.mainEnv.player
local res = {}
for si, sc in ipairs(SCEN) do
  local output = copy(base.output)
  for k, v in pairs(sc.output) do output[k] = v end
  for _, k in ipairs(sc.unset) do output[k] = nil end
  local modDB = new("ModDB"):ModDB()
  for _, m in ipairs(sc.mods) do modDB:NewMod(m[1], m[2], m[3], "Synth") end
  local shift = deep2(base.damageShiftTable)
  for from, row in pairs(sc.shiftRows) do shift[from] = copy(row) end
  local actor = { output = output, modDB = modDB, damageShiftTable = shift }
  local r = { pools = {}, taken = {}, reads = {}, flags = {}, sums = {}, shift = {} }
  for i, c in ipairs(sc.cases) do
    local damage = copy(c.damage)
    local pt = nil
    if c.pool then
      pt = { Life = c.pool.Life, Mana = c.pool.Mana, EnergyShield = c.pool.EnergyShield, Ward = c.pool.Ward, WardActiveChance = c.pool.WardActiveChance,
        Aegis = c.pool.Aegis and copy(c.pool.Aegis), Guard = c.pool.Guard and copy(c.pool.Guard), AlliesTakenBeforeYou = c.pool.Allies and deep2(c.pool.Allies) }
    end
    local ok, rr = pcall(calcs.reducePoolsByDamage, pt, damage, actor)
    if not ok then r.pools[i] = { "ERROR", tostring(rr) } else
      local row = {}
      for _, f in ipairs(FIELDS) do row[#row + 1] = num(rr[f]) end
      for _, t in ipairs(TYPES) do row[#row + 1] = num(rr.damageTakenThatCanBeRecouped[t]) end
      for _, t in ipairs(TYPES) do row[#row + 1] = num(damage[t]) end
      for _, k in ipairs(AEGIS) do row[#row + 1] = num(rr.Aegis[k]) end
      for _, k in ipairs(GUARD) do row[#row + 1] = num(rr.Guard[k]) end
      for _, k in ipairs(ALLYK) do local a = rr.AlliesTakenBeforeYou[k]; row[#row + 1] = num(a and a.remaining) end
      r.pools[i] = row
    end
  end
  for i, c in ipairs(TAKEN) do
    local ok, sum, parts = pcall(calcs.takenHitFromDamage, c[1], c[2], actor)
    if not ok then r.taken[i] = { "ERROR", tostring(sum) } else
      local row = { num(sum) }
      for _, t in ipairs(TYPES) do row[#row + 1] = num(parts[t]) end
      r.taken[i] = row
    end
  end
  for i, k in ipairs(READS) do r.reads[i] = num(output[k]) end
  for i, k in ipairs(FLAGS) do r.flags[i] = modDB:Flag(nil, k) and "true" or "false" end
  for i, s in ipairs(SUMS) do r.sums[i] = num(modDB:Sum(s[1], nil, s[2])) end
  for _, from in ipairs(TYPES) do for _, to in ipairs(TYPES) do r.shift[#r.shift + 1] = num(shift[from] and shift[from][to]) end end
  res[si] = r
end
return res
`
const casesOf = (s: Scenario): { name: string; pool: Rec | false; damage: Rec }[] =>
  Object.entries(s.pools).flatMap(([p, pl]) => (s.dmg ?? DN).map((d) => ({ name: `${p} x ${d}`, pool: pl, damage: DAMAGE[d]! })))
const poolsChunk = (): string =>
  `local SCEN = ${lua(S.map((s) => ({ output: s.output ?? {}, unset: s.unset ?? [], mods: s.mods ?? [], shiftRows: s.shiftRows ?? {}, cases: casesOf(s) })))}\n` +
  `local TAKEN = ${lua(TAKEN.map((c) => [c.raw, c.type]))}\nlocal FIELDS, TYPES, AEGIS, GUARD, ALLYK = ${lua(FIELDS)}, ${lua(DMG)}, ${lua(AEGIS_KEYS)}, ${lua(GUARD_KEYS)}, ${lua(ALLY_KEYS)}\n` +
  `local READS, FLAGS, SUMS = ${lua(READS)}, ${lua(FLAGS)}, ${lua(SUMS)}\n${PROBE}`

// --- pob-ts replay on the same reads (informational self-check) ---
const fmt = (v: unknown): string => (v === undefined ? 'nil' : typeof v === 'number' ? formatG(v, 17) : String(v))
const canon = (s: string): string => (s === '-nan' ? 'nan' : s)
const parseNum = (s: string): number | undefined => (s === 'nil' ? undefined : s === 'inf' ? Infinity : s === '-inf' ? -Infinity : s === 'nan' || s === '-nan' ? NaN : Number(s))
function tsActor(raw: Rec): Rec {
  const output: Rec = {}
  READS.forEach((k, i) => { const v = parseNum(raw.reads[i]); if (v !== undefined) output[k] = v })
  const flags = new Map(FLAGS.map((k, i) => [k, raw.flags[i] === 'true']))
  const sums = new Map(SUMS.map(([kind, n], i) => [`${kind}:${n}`, parseNum(raw.sums[i])!]))
  const damageShiftTable: Rec = {}
  DMG.forEach((from, i) => DMG.forEach((to, j) => { const v = parseNum(raw.shift[i * 5 + j]); if (v !== undefined) (damageShiftTable[from] ??= {})[to] = v }))
  return { output, damageShiftTable, modDB: { Flag: (_c: unknown, n: string) => flags.get(n) ?? false, Sum: (kind: string, _c: unknown, n: string) => sums.get(`${kind}:${n}`) ?? 0 } }
}
function tsPoolRow(actor: Rec, c: Rec): string[] {
  const damage = { ...c.damage }
  const p = c.pool as Rec | false
  const pt = p === false ? undefined : { Life: p.Life, Mana: p.Mana, EnergyShield: p.EnergyShield, Ward: p.Ward, WardActiveChance: p.WardActiveChance, Aegis: p.Aegis && { ...p.Aegis }, Guard: p.Guard && { ...p.Guard },
    AlliesTakenBeforeYou: p.Allies && Object.fromEntries(Object.entries(p.Allies).map(([k, v]) => [k, { ...(v as Rec) }])) }
  const r = reducePoolsByDamage(pt, damage, actor)
  return [...FIELDS.map((f) => fmt(r[f])), ...DMG.map((t) => fmt(r.damageTakenThatCanBeRecouped[t])), ...DMG.map((t) => fmt(damage[t])), ...AEGIS_KEYS.map((k) => fmt(r.Aegis[k])), ...GUARD_KEYS.map((k) => fmt(r.Guard[k])),
    ...ALLY_KEYS.map((k) => fmt(r.AlliesTakenBeforeYou[k]?.remaining))]
}
function tsTakenRow(actor: Rec, c: Rec): string[] {
  const [sum, damages] = takenHitFromDamage(c.raw, c.type, actor)
  return [fmt(sum), ...DMG.map((t) => fmt(damages[t]))]
}

function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}

async function main(): Promise<void> {
  const luts = await prepareLuts()
  const b = GOLDEN_BUILDS.find((x: Rec) => x.name === BUILD)
  const xmlBuf = readFileSync(b.xmlPath(roots))
  const loadChunk = `loadBuildFromXML(${luaBytes(xmlBuf)}, ${JSON.stringify(BUILD)})\nif __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n`
  let digest = ''
  try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
  const raw = ljEval(loadChunk + poolsChunk(), luts) as Rec[]
  const scenarios: Rec[] = []
  const report: Rec[] = []
  S.forEach((s, si) => {
    const cases = casesOf(s)
    const r = raw[si]!
    const reads: Rec = {}
    READS.forEach((k, i) => { reads[k] = r.reads[i] })
    const actor = tsActor(r)
    let total = 0
    let exact = 0
    let errs = 0
    const bad: string[] = []
    const chk = (label: string, cols: readonly string[], theirs: string[], ours: (() => string[]) | undefined): void => {
      if (theirs[0] === 'ERROR') {
        errs++
        let msg = 'no throw'
        try { ours!() } catch (e) { msg = (e as Error).message }
        total++; if (msg !== 'no throw') exact++; else bad.push(`${s.name} ${label}: luajit ERROR ${theirs[1]} / pob-ts no throw`)
        return
      }
      let o: string[]
      try { o = ours!() } catch (e) { total += cols.length; bad.push(`${s.name} ${label}: pob-ts threw ${(e as Error).message}`); return }
      cols.forEach((c, k) => { total++; if (canon(o[k]!) === canon(theirs[k]!)) exact++; else bad.push(`${s.name} ${label}.${c}: pob-ts ${o[k]} != luajit ${theirs[k]}`) })
    }
    cases.forEach((c, i) => chk(c.name, POOL_COLUMNS, r.pools[i], () => tsPoolRow(actor, c)))
    TAKEN.forEach((c, i) => chk(c.name, TAKEN_COLUMNS, r.taken[i], () => tsTakenRow(actor, c)))
    scenarios.push({ name: s.name, covers: s.covers, inputs: { synth: { output: s.output ?? {}, unset: s.unset ?? [], mods: (s.mods ?? []).map(([n, k, v]) => `${k}:${n}=${String(v)}`), shiftRows: s.shiftRows ?? {} },
      cases, taken: TAKEN.map((c) => ({ name: c.name, raw: String(c.raw), type: c.type })) }, results: { pools: r.pools, taken: r.taken }, actorReads: { reads, flags: r.flags, sums: r.sums, shift: r.shift } })
    report.push({ scenario: s.name, covers: s.covers, calls: cases.length + TAKEN.length, luajitErrors: errs, fields: total, pobtsEqualLuajit: exact, mismatches: bad.slice(0, 4), mismatchCount: bad.length,
      flagsTrue: FLAGS.filter((_, i) => r.flags[i] === 'true').length, sumsNonZero: SUMS.filter((_, i) => parseNum(r.sums[i]) !== 0).map(([k, n]) => `${k}:${n}`) })
    console.error(`${s.name}: ${exact}/${total}`)
  })
  const obj: Rec = { build: BUILD, format: 1, xmlSha256: createHash('sha256').update(xmlBuf).digest('hex'),
    producer: { generator: `pob-ts ${git('C:/pob-work/pob-ts')}`, image: digest || LUAJIT_IMAGE, luajit: 'desktop LuaJIT docker', pobSha: git('C:/pob-work/project_15/PathOfBuilding'),
      probe: 'R4.1 pools PROBE (export-r4.ts) on a synthetic actor { output = copy(mainEnv.player.output)+overrides, modDB = new("ModDB")+Synth mods, damageShiftTable = copy+rows }; one docker run, build loaded once' },
    columns: { pool: POOL_COLUMNS, taken: TAKEN_COLUMNS, reads: READS, flags: FLAGS, sums: SUMS.map(([kind, n]) => `${kind}:${n}`), types: DMG, allyKeys: ALLY_KEYS }, scenarios }
  const text = `{\n${Object.keys(obj).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(obj[k])}`).join(',\n')}\n}\n`
  if (Buffer.byteLength(text) > MAX_BYTES) throw new Error(`${Buffer.byteLength(text)} bytes > 1.5 MB`)
  writeFileSync(OUT, text, 'utf8')
  console.log(JSON.stringify({ file: OUT, bytes: statSync(OUT).size, sha256: createHash('sha256').update(text).digest('hex'), report }, null, 1))
}
await main()
