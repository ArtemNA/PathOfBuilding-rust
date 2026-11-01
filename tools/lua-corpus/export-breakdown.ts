// R8.4: LuaJIT truth for the breakdown tables (`env.player.breakdown`, `env.minion.breakdown`; `env.enemy.breakdown` is never set upstream --
// recorded as absent). Mechanics = export-r8.ts (docker LuaJIT, LUT mount, `%.17g` numbers).
// Per scenario: loadBuildFromXML -> (instrument) -> wipeGlobalCache -> calcsTab:BuildOutput() (the R8.1 display path) -> mainEnv.{player,minion}.breakdown.
// Instrumentation (no PoB file is touched):
//   * `calcs.breakdownModule` -> a wrapper of Modules.CalcBreakdown that returns an empty proxy (`__index` = the real table, `__newindex` records the
//     writer `<file>:<line>` of every key and stores into the real table). Upstream never iterates a breakdown table (grep `pairs(breakdown`: 0).
//   * `calcs.triggers` wrapper: serializes `actor.breakdown` on entry (CalcPerform.lua:3797 player / :3807 minion) = the state after perform +
//     calcs.defence + buildDefenceEstimations, before triggers / mirages / offence (R8.5).
// Per actor and key: `snap` (state at the triggers call; null if the key was not there), `final` (after BuildOutput), `writers` (sorted set),
// `class`: "ported"   = every writer in CalcPerform/CalcDefence/CalcSetup and (no snap or snap == final): R8.4 writes it, R8.5 does not touch it;
//          "R8.5"     = some writer in CalcOffence/CalcTriggers/CalcMirages or the value changed after the snapshot (offence appended);
//          "Calcs"    = written by Calcs.lua buildOutput (SkillBuffs/SkillDebuffs, after perform);
//   `gate` = the value the ported modules leave (snap if present, else final for "ported" keys written after triggers, e.g. GemLevel :3948).
// `replaced`: the final table is not the one perform made (CalcMirages.lua:158/:276 hand over the mirage env's table).
// Serialization: string -> itself; number -> {"$n":"%.17g"}; boolean -> itself; table -> object: "[]" = the 1..n run (ipairs), other keys by
// tostring (number keys "#<k>"); a mod -> {"$mod":{name,type,value,source,flags,keywordFlags,tags}}; an item -> {"$item":<name>}; function -> {"$fn":1}.
// Usage (Node 24, repo root):  node tools/lua-corpus/export-breakdown.ts [name-filter]  -> crates/pob-calc/tests/fixtures/breakdown/<slug>.json (2 passes, must be identical)
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { promisify } from 'node:util'
const run = promisify(execFile)
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-calc/tests/fixtures/breakdown')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const JOBS = Number(process.env.R8_JOBS ?? 4)
const roots = resolveRoots()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
// the 20 R7-synth scenarios of the R8.1 display fixtures (export-r8.ts SYNTH_SET): the display underline gate needs their breakdown classes
const R81_SYNTH = ['fd-gen-melee', 'fd-occ-spells', 'fd-occ-dot', 'fd-occ-trap-mine-totem', 'fd-occ-minions', 'fd-mir-mirage', 'fd-mir-bow-ballista', 'fd-cos-coc', 'fd-occ-poison-ignite', 'fd-all-ds',
  't-trap', 't-mine', 't-coc', 't-cwc', 't-cospri', 'mi-sacred-wisps', 'mn-raise-zombie', 'mn-spectre-hydra-mid', 'mn-golem-flame', 'fn-Spark']

async function prepareLuts(): Promise<string> {
  const dir = resolve(SCRATCH, 'luts')
  mkdirSync(dir, { recursive: true })
  const { findRequiredJewelLuts } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/timelessJewelLuts.mjs')).href)
  const { unpackJewelData } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/nodeJewelData.mjs')).href)
  for (const b of GOLDEN_BUILDS) for (const { fileStem } of findRequiredJewelLuts(readFileSync(b.xmlPath(roots), 'utf8'))) writeFileSync(resolve(dir, `${fileStem}.bin`), unpackJewelData(roots.pobRoot, fileStem).bytes)
  return dir
}
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
    if file then file:close() return { GetFileName = function() return spec:match("([^/\\]+)$") end, GetFileModifiedTime = function() return 1 end } end
  end
  return headlessNewFileSearch(spec, findDirectories)
end
`
let LUTS = ''
async function ljEval(code: string): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r84-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${LUTS}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    try { const { stdout } = await run('docker', args, { encoding: 'utf8', maxBuffer: 1024 * 1024 * 1024 }); return JSON.parse(stdout.trim().split('\n').pop()!).result } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
const loadChunk = (xml: Buffer, name: string): string =>
  `loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(name)})\nif __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n`

const PROBE = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local function N(v) if v ~= v then return "nan" end return RAWF("%.17g", v) end
local MODTYPES = { BASE = true, INC = true, MORE = true, FLAG = true, LIST = true, OVERRIDE = true, MAX = true, MIN = true }
local S
local function SMod(m, depth)
  local o = obj()
  o.name, o.type, o.source = m.name, m.type, m.source and tostring(m.source) or nil
  o.value = S(m.value, depth + 1)
  o.flags, o.keywordFlags = m.flags and N(m.flags) or nil, m.keywordFlags and N(m.keywordFlags) or nil
  local tags = {}
  for i, t in ipairs(m) do tags[i] = S(t, depth + 1) end
  if #tags > 0 then o.tags = tags end
  local w = obj() w["$mod"] = o return w
end
S = function(v, depth)
  local tv = type(v)
  if tv == "string" then return v end
  if tv == "boolean" then return v end
  if tv == "number" then local o = obj() o["$n"] = N(v) return o end
  if tv == "function" then local o = obj() o["$fn"] = 1 return o end
  if tv ~= "table" then local o = obj() o["$type"] = tv return o end
  if depth > 10 then local o = obj() o["$deep"] = 1 return o end
  if type(v.name) == "string" and MODTYPES[v.type] and v.source ~= nil and v.value ~= nil then return SMod(v, depth) end
  if v.baseName ~= nil and v.rarity ~= nil then local o = obj() o["$item"] = tostring(v.name) return o end
  local o = obj()
  local n = 0
  while v[n + 1] ~= nil do n = n + 1 end
  if n > 0 then local l = {} for i = 1, n do l[i] = S(v[i], depth + 1) end o["[]"] = l end
  for k, x in pairs(v) do
    if not (type(k) == "number" and k >= 1 and k <= n and k == math.floor(k)) then
      local key = type(k) == "number" and ("#" .. N(k)) or tostring(k)
      o[key] = S(x, depth + 1)
    end
  end
  return o
end
local calcs = require("Modules.CalcBase")
local origModName = calcs.breakdownModule
local origCtor = require(origModName)
local writes = setmetatable({}, { __mode = "k" })
local realOf = setmetatable({}, { __mode = "k" })
package.loaded["ProbeBreakdown"] = function(modDB, output, actor)
  local real = origCtor(modDB, output, actor)
  local w = {}
  local proxy = setmetatable({}, { __index = real, __newindex = function(_, k, v)
    local info = debug.getinfo(2, "Sl")
    local src = (info.short_src:match("([^/]+)$") or info.short_src) .. ":" .. tostring(info.currentline)
    w[k] = w[k] or {}
    w[k][src] = true
    rawset(real, k, v)
  end })
  writes[proxy], realOf[proxy] = w, real
  return proxy
end
calcs.breakdownModule = "ProbeBreakdown"
-- the real table's own keys (the constructor's functions) are not breakdown entries
local function entries(proxy)
  local real, e = realOf[proxy], {}
  for k, v in pairs(real) do if type(v) ~= "function" then e[tostring(k)] = v end end
  return e
end
local snaps = setmetatable({}, { __mode = "k" })
local origTriggers = calcs.triggers
calcs.triggers = function(env, actor, ...)
  local b = actor.breakdown
  if b and realOf[b] then
    local s = {}
    for k, v in pairs(entries(b)) do s[k] = S(v, 0) end
    snaps[actor] = { proxy = b, ser = s }
  end
  return origTriggers(env, actor, ...)
end
wipeGlobalCache()
build.calcsTab:BuildOutput()
calcs.triggers, calcs.breakdownModule = origTriggers, origModName
local mainEnv = build.calcsTab.mainEnv
local res = obj()
local function dumpActor(actor)
  if not actor then return nil end
  local b = actor.breakdown
  local a = obj()
  if not b then a.absent = true return a end
  if not realOf[b] then a.uninstrumented = true return a end
  local snap = snaps[actor]
  a.hasSnap = snap ~= nil
  a.replaced = snap ~= nil and snap.proxy ~= b
  local keys = obj()
  local seen = {}
  local fin = entries(b)
  for k, v in pairs(fin) do seen[k] = true end
  if snap then for k in pairs(snap.ser) do seen[k] = true end end
  for k in pairs(seen) do
    local e = obj()
    local wl = {}
    for src in pairs(writes[b][k] or {}) do wl[#wl + 1] = src end
    table.sort(wl)
    e.writers = wl
    if fin[k] ~= nil then e.final = S(fin[k], 0) end
    if snap and snap.ser[k] ~= nil then e.snap = snap.ser[k] end
    keys[k] = e
  end
  a.keys = keys
  return a
end
res.player = dumpActor(mainEnv.player)
res.minion = dumpActor(mainEnv.minion)
res.enemy = dumpActor(mainEnv.enemy)
res.hasMinion = mainEnv.minion ~= nil
-- R13 (MINEFIELD-RUST): gem variants sharing a grantedEffectId resolve per process (SkillsTab.lua:472-477) -> name sets
local amb = obj()
for _, vars in pairs(data.gemsByGameId) do
  local byEff = {}
  for _, v in pairs(vars) do local k = tostring(v.grantedEffectId) byEff[k] = byEff[k] or {} byEff[k][#byEff[k] + 1] = tostring(v.nameSpec or v.name) end
  for _, names in pairs(byEff) do
    if #names > 1 then table.sort(names) local set = table.concat(names, "|") for _, n in ipairs(names) do amb[n] = set end end
  end
end
res.r13GemNames = amb
return res
`

// --- deterministic JSON ---
function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
// dkjson encodes an empty Lua table as [] -- normalise every serialized table back to an object (a table is always an object here)
function norm(v: unknown): unknown {
  if (Array.isArray(v)) return v.length === 0 ? {} : v.map(norm)
  if (v && typeof v === 'object') { const o: Rec = {}; for (const [k, x] of Object.entries(v as Rec)) o[k] = k === '[]' || k === 'tags' || k === 'writers' ? (x as unknown[]).map(norm) : norm(x); return o }
  return v
}
// R13 normalisation (MINEFIELD-RUST R13: pairs over string keys differs per process):
//  * rowList order of <T>MaximumHitTaken (CalcDefence.lua:3326 pairs(takenDamages)), TotalBuildDegen / <T>BuildDegen (:3389 pairs(damageOverTimeShiftTable[T])),
//    TotalDegen / <T>EnemyDegen (:3556, :3618 pairs(ailmentList), :3625) -> rows sorted by their serialization;
//  * Req<Attr> Gem rows whose gem resolves to one of several variants (SkillsTab.lua:472-477) -> name replaced by `<R13:a|b>` (the set).
const ROW_SORTED = /(MaximumHitTaken|BuildDegen|EnemyDegen|^TotalDegen)$/
const GEM_ROW = /^(\^x[0-9A-Fa-f]{6}|\^\d)?(.*) (\^7-?\d+\/-?\d+)$/
function normValue(key: string, v: unknown, amb: Rec, used: Set<string>): unknown {
  if (!v || typeof v !== 'object') return v
  const o = v as Rec
  const rl = o.rowList as Rec | undefined
  if (ROW_SORTED.test(key) && rl && Array.isArray(rl['[]'])) o.rowList = { ...rl, '[]': [...rl['[]']].sort((a, b) => byteOrder(ser(a), ser(b))) }
  if (/^Req/.test(key) && rl && Array.isArray(rl['[]'])) {
    for (const r of rl['[]'] as Rec[]) {
      if (r.source !== 'Gem' || typeof r.sourceName !== 'string') continue
      const m = GEM_ROW.exec(r.sourceName)
      if (m && amb[m[2]!]) { used.add(amb[m[2]!]); r.sourceName = `${m[1] ?? ''}<R13:${amb[m[2]!]}> ${m[3]}` }
    }
  }
  return o
}
const PORTED_FILES = new Set(['CalcPerform.lua', 'CalcDefence.lua', 'CalcSetup.lua'])
const R85_FILES = new Set(['CalcOffence.lua', 'CalcTriggers.lua', 'CalcMirages.lua'])
function classify(a: Rec | null, amb: Rec, used: Set<string>): Rec | null {
  if (!a || a.absent || a.uninstrumented) return a
  const keys: Rec = {}
  for (const [k, e0] of Object.entries(a.keys as Rec)) {
    const e = norm(e0) as Rec
    e.snap = normValue(k, e.snap, amb, used)
    e.final = normValue(k, e.final, amb, used)
    const files = (e.writers as string[]).map((w) => w.replace(/:\d+$/, ''))
    let cls: string
    if (files.includes('Calcs.lua')) cls = 'Calcs'
    else if (a.replaced) cls = 'R8.5'
    else if (files.some((f) => R85_FILES.has(f))) cls = 'R8.5'
    else if (files.every((f) => PORTED_FILES.has(f)) && (e.snap === undefined || e.final === undefined || ser(e.snap) === ser(e.final))) cls = 'ported'
    else if (files.every((f) => PORTED_FILES.has(f))) cls = 'R8.5' // changed after the snapshot (offence appended into a ported table)
    else throw new Error(`unknown writer file for ${k}: ${files.join(',')} -- STOP`)
    // the value the ported modules leave: the snapshot, else (written after triggers by a ported writer, e.g. GemLevel) the final value
    const gate = e.snap !== undefined ? e.snap : files.every((f) => PORTED_FILES.has(f)) ? e.final : undefined
    keys[k] = { class: cls, writers: e.writers, snap: e.snap, final: e.final, gate }
  }
  return { hasSnap: a.hasSnap, replaced: a.replaced, keys }
}

type Scenario = { slug: string; name: string; xml: Buffer; kind: 'golden' | 'defence-synth' | 'defence-synth2' | 'r81-synth' }
function scenarios(filter?: string): Scenario[] {
  const s: Scenario[] = GOLDEN_BUILDS.map((b: Rec) => ({ slug: b.name.replace(/ /g, '-'), name: b.name, xml: readFileSync(b.xmlPath(roots)), kind: 'golden' as const }))
  for (const [dir, kind] of [['defence-synth', 'defence-synth'], ['defence-synth2', 'defence-synth2']] as const) {
    for (const f of readdirSync(resolve(FIX, dir)).filter((x) => x.endsWith('.xml')).sort(byteOrder)) {
      const n = f.replace(/\.xml$/, '')
      s.push({ slug: `${kind}--${n}`, name: n, xml: readFileSync(resolve(FIX, dir, f)), kind })
    }
  }
  for (const n of R81_SYNTH) s.push({ slug: `r81--${n}`, name: n, xml: readFileSync(resolve(FIX, 'offence-synth', `${n}.xml`)), kind: 'r81-synth' })
  return filter ? s.filter((x) => x.slug.includes(filter)) : s
}
async function pool<T, R>(xs: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(xs.length)
  let next = 0
  await Promise.all(Array.from({ length: JOBS }, async () => { for (;;) { const i = next++; if (i >= xs.length) return; res[i] = await f(xs[i]!); if ((i + 1) % 10 === 0) console.error(`  done ${i + 1}/${xs.length}`) } }))
  return res
}
async function pass(list: Scenario[]): Promise<{ files: Map<string, string>; rows: Rec[] }> {
  const probes = await pool(list, (s) => ljEval(loadChunk(s.xml, s.name) + `local function probe()\n${PROBE}\nend\nreturn probe()\n`))
  const files = new Map<string, string>()
  const rows: Rec[] = []
  const producer = { generator: 'tools/lua-corpus/export-breakdown.ts', image: LUAJIT_IMAGE, note: 'LuaJIT docker; wipeGlobalCache + calcsTab:BuildOutput() after load; breakdown writers via a proxy over Modules.CalcBreakdown, snapshot at calcs.triggers' }
  list.forEach((s, i) => {
    const r = probes[i]!
    const used = new Set<string>()
    const amb = (r.r13GemNames ?? {}) as Rec
    const player = classify(r.player, amb, used), minion = classify(r.minion ?? null, amb, used)
    const fx = { format: 1, producer, build: s.name, kind: s.kind, xmlSha256: sha(s.xml), hasMinion: r.hasMinion, enemy: r.enemy ? norm(r.enemy) : null, player, minion,
      r13: { rowsSorted: ROW_SORTED.source, gemSets: [...used].sort(byteOrder) } }
    const t = ser(fx) + '\n'
    if (Buffer.byteLength(t) > MAX_BYTES) throw new Error(`${s.slug}: ${Buffer.byteLength(t)} bytes > ${MAX_BYTES} -- STOP`)
    files.set(`${s.slug}.json`, t)
    const count = (a: Rec | null, c: string): number => (a && a.keys ? Object.values(a.keys as Rec).filter((e: Rec) => e.class === c).length : 0)
    rows.push({ slug: s.slug, bytes: Buffer.byteLength(t), player: { ported: count(player, 'ported'), r85: count(player, 'R8.5'), calcs: count(player, 'Calcs'), replaced: player?.replaced },
      minion: minion ? { ported: count(minion, 'ported'), r85: count(minion, 'R8.5'), calcs: count(minion, 'Calcs') } : null })
  })
  return { files, rows }
}

async function main(filter?: string): Promise<void> {
  LUTS = await prepareLuts()
  const list = scenarios(filter)
  console.error(`pass 1 (${list.length} scenarios)`)
  const a = await pass(list)
  console.error('pass 2')
  const b = await pass(list)
  const diff = [...a.files.keys()].filter((k) => a.files.get(k) !== b.files.get(k))
  if (diff.length) {
    mkdirSync(SCRATCH, { recursive: true })
    for (const k of diff) { writeFileSync(resolve(SCRATCH, `bd-a-${k}`), a.files.get(k)!); writeFileSync(resolve(SCRATCH, `bd-b-${k}`), b.files.get(k)!) }
    console.log(JSON.stringify({ STOP: 'passes differ', diff }, null, 1)); process.exitCode = 1; return
  }
  mkdirSync(OUT, { recursive: true })
  const shas: Rec = {}
  for (const [n, t] of a.files) { writeFileSync(resolve(OUT, n), t, 'utf8'); shas[n] = sha(t) }
  const tot = a.rows.reduce((acc, r) => { acc.ported += r.player.ported + (r.minion?.ported ?? 0); acc.r85 += r.player.r85 + (r.minion?.r85 ?? 0); acc.calcs += r.player.calcs + (r.minion?.calcs ?? 0); acc.maxBytes = Math.max(acc.maxBytes, r.bytes); return acc }, { ported: 0, r85: 0, calcs: 0, maxBytes: 0 })
  console.log(JSON.stringify({ scenarios: list.length, files: a.files.size, passesIdentical: true, combinedSha256: sha(Object.keys(shas).sort(byteOrder).map((k) => `${k}:${shas[k]}`).join('\n')), totals: tot, rows: a.rows }, null, 1))
}
await main(process.argv[2])
