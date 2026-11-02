// R8.6: LuaJIT truth for the Calcs tab: the CALCS run (`calcsTab.calcsEnv` -- Calcs.lua buildOutput(build, "CALCS")) and the text of every
// CalcSectionControl cell. Mechanics = export-breakdown.ts (docker LuaJIT, LUT mount, `%.17g` numbers, the same breakdown proxy).
// Per scenario: loadBuildFromXML -> (breakdown proxy) -> wipeGlobalCache -> calcsTab:BuildOutput() (MAIN, CALCS, misc calculator) ->
//   output    = calcsOutput, every key (numbers {"$n":"%.17g"}, strings, booleans; table values serialized; `Req<Attr>Item.sourceGem` -> {"$gem":nameSpec});
//   breakdown = calcsEnv.{player,minion}.breakdown, final values (the breakdown-module proxy of export-breakdown.ts: a nested hand module serializes as {});
//   sections  = for the player (input.showMinion = false) and, with calcsEnv.minion, for the minion (showMinion = true): every section of
//               calcsTab.sectionList in order -> section:UpdateSize() then section:DrawContent(...) with the globals DrawString captured
//               (only the calls made from Classes/CalcSectionControl.lua, as `[line, text]`; DrawControls of the section is a no-op).
//               `enabled` / `height` after UpdateSize. No coordinates.
//   data      = (once, `_sections.json`) LoadModule("Modules/CalcSections") serialized -- the gate for the Rust copy of the data.
// R13: the same normalisation as export-breakdown.ts on the breakdown; 2 passes must be byte-identical.
// Usage (Node 24, repo root):  node tools/lua-corpus/export-calcs.ts [name-filter]  -> crates/pob-engine/tests/fixtures/calcs/<slug>.json
//                              node tools/lua-corpus/export-calcs.ts perf           -> tools/.scratch/perf-calcs.json
import '../upstream.ts'
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
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
const OUT = resolve(ROOT, 'crates/pob-engine/tests/fixtures/calcs')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const JOBS = Number(process.env.R8_JOBS ?? 4)
const roots = resolveRoots()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const median = (xs: number[]): number => { const s = [...xs].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]! }
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
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r86-'))
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

// --- Lua: serializer (export-breakdown.ts `S`) + the breakdown proxy -------------------------------------------------------------------------------
const HELPERS = String.raw`
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
-- an output value: Req<Attr>Item.sourceGem is the gem instance (a deep table) -> its nameSpec
local function SOut(k, v)
  if type(v) == "table" and type(k) == "string" and k:match("^Req%a+Item$") then
    local o = obj()
    for kk, x in pairs(v) do
      if kk == "sourceGem" then local g = obj() g["$gem"] = tostring(x.nameSpec) o.sourceGem = g else o[tostring(kk)] = S(x, 1) end
    end
    return o
  end
  return S(v, 0)
end
local calcs = require("Modules.CalcBase")
local origModName = calcs.breakdownModule
local origCtor = require(origModName)
local realOf = setmetatable({}, { __mode = "k" })
package.loaded["ProbeBreakdown"] = function(modDB, output, actor)
  local real = origCtor(modDB, output, actor)
  local proxy = setmetatable({}, { __index = real, __newindex = function(_, k, v) rawset(real, k, v) end })
  realOf[proxy] = real
  return proxy
end
local function entries(b)
  local real, e = realOf[b] or b, {}
  for k, v in pairs(real) do if type(v) ~= "function" then e[tostring(k)] = v end end
  return e
end
local function dumpBreakdown(actor)
  if not actor then return nil end
  local b = actor.breakdown
  if not b then local a = obj() a.absent = true return a end
  local keys = obj()
  for k, v in pairs(entries(b)) do keys[k] = S(v, 0) end
  local a = obj() a.keys = keys a.instrumented = realOf[b] ~= nil
  return a
end
`

const PROBE = String.raw`
calcs.breakdownModule = "ProbeBreakdown"
local res = obj()
res.calcsInputOverride = OVERRIDE
wipeGlobalCache()
build.calcsTab:BuildOutput()
calcs.breakdownModule = origModName
local tab = build.calcsTab
local env = tab.calcsEnv
local out = obj()
for k, v in pairs(tab.calcsOutput) do out[tostring(k)] = SOut(k, v) end
res.output = out
res.breakdown = obj()
res.breakdown.player = dumpBreakdown(env.player)
res.breakdown.minion = dumpBreakdown(env.minion)
res.hasMinion = env.minion ~= nil
res.calcsInput = obj()
for k, v in pairs(tab.input) do res.calcsInput[tostring(k)] = S(v, 0) end
res.mainSkill = env.player.mainSkill and env.player.mainSkill.activeEffect.grantedEffect.name or nil
-- the cell texts as DrawContent draws them
local SRC = "CalcSectionControl"
local rawDrawString = DrawString
local cur
DrawString = function(x, y, align, size, font, text)
  local info = debug.getinfo(2, "Sl")
  if cur and info.short_src:find(SRC, 1, true) then cur[#cur + 1] = { info.currentline, text } end
  return rawDrawString(x, y, align, size, font, text)
end
local viewPort = { x = 0, y = 0, width = 1920, height = 1080 }
local function renderAll(actor)
  local list = {}
  for i, section in ipairs(tab.sectionList) do
    section.DrawControls = function() end
    section.x, section.y = 10000, 10000
    local s = obj()
    s.id = section.id
    local draws = {}
    local ok, err = pcall(function()
      section:UpdateSize()
      s.enabled = section.enabled and true or false
      s.height = N(section.height)
      section:UpdatePos()
      cur = draws
      section:DrawContent(10000, 10000, section.width, actor, viewPort, false, true)
      cur = nil
    end)
    cur = nil
    if not ok then s.error = tostring(err):gsub("^[^:]*/", "") end
    s.draws = draws
    list[i] = s
  end
  return list
end
local savedShow = tab.input.showMinion
tab.input.showMinion = false
res.sectionsPlayer = renderAll(env.player)
if env.minion then
  tab.input.showMinion = true
  res.sectionsMinion = renderAll(env.minion)
end
tab.input.showMinion = savedShow
DrawString = rawDrawString
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

const DATA_PROBE = String.raw`
local data = LoadModule("Modules/CalcSections")
local l = {}
for i, s in ipairs(data) do l[i] = S(s, 0) end
return l
`

// --- deterministic JSON ---
function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
// dkjson encodes an empty Lua table as [] -- every serialized table here is an object except the `[]` / `tags` / `draws` arrays
function norm(v: unknown): unknown {
  if (Array.isArray(v)) return v.length === 0 ? {} : v.map(norm)
  if (v && typeof v === 'object') { const o: Rec = {}; for (const [k, x] of Object.entries(v as Rec)) o[k] = k === '[]' || k === 'tags' ? (x as unknown[]).map(norm) : norm(x); return o }
  return v
}
// R13 normalisation of the breakdown (export-breakdown.ts normValue)
const ROW_SORTED = /(MaximumHitTaken|BuildDegen|EnemyDegen|^TotalDegen)$/
const GEM_ROW = /^(\^x[0-9A-Fa-f]{6}|\^\d)?(.*) (\^7-?\d+\/-?\d+)$/
function normValue(key: string, v: unknown, amb: Rec, used: Set<string>): unknown {
  if (!v || typeof v !== 'object') return v
  const o = v as Rec
  // SkillBuffs / SkillDebuffs (Calcs.lua:762-806): the mod order inside one buff / curse list differs per process (2 passes on OccVortex) -> rows sorted
  if (/^Skill(De)?[Bb]uffs$/.test(key) && o.modList && Array.isArray((o.modList as Rec)['[]'])) o.modList = { '[]': [...(o.modList as Rec)['[]']].sort((a: unknown, b: unknown) => byteOrder(ser(a), ser(b))) }
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
function normBreakdown(a: Rec | null, amb: Rec, used: Set<string>): Rec | null {
  if (!a || a.absent) return a
  const keys: Rec = {}
  for (const [k, v] of Object.entries(norm(a.keys) as Rec)) keys[k] = normValue(k, v, amb, used)
  return { instrumented: a.instrumented, keys }
}
const sections = (l: Rec[] | undefined): Rec[] | null => (l ? l.map((s) => ({ ...s, draws: Array.isArray(s.draws) ? s.draws : [] })) : null)

// Variants (the XML's own Calcs inputs leave most R7-synth CALCS runs on a template's `skill_number`): `ov.skill = 'main'` sets
// `calcsTab.input.skill_number = build.mainSocketGroup` (the CALCS run then views the scenario's main skill), `ov.buffMode` sets
// `misc_buffMode`. The values written are recorded in the fixture (`calcsInputOverride`); Rust writes the same into the model's Calcs inputs.
type Ov = { skill?: 'main'; buffMode?: string }
type Scenario = { slug: string; name: string; xml: Buffer; kind: 'golden' | 'r81-synth'; ov?: Ov }
const BUFF_MODE_GOLDENS = ['OccVortex', 'Dual Wield Cospris CoC']
function scenarios(filter?: string): Scenario[] {
  const s: Scenario[] = GOLDEN_BUILDS.map((b: Rec) => ({ slug: b.name.replace(/ /g, '-'), name: b.name, xml: readFileSync(b.xmlPath(roots)), kind: 'golden' as const }))
  for (const n of R81_SYNTH) s.push({ slug: `r81--${n}`, name: n, xml: readFileSync(resolve(FIX, 'offence-synth', `${n}.xml`)), kind: 'r81-synth' })
  for (const n of R81_SYNTH) s.push({ slug: `main--${n}`, name: n, xml: readFileSync(resolve(FIX, 'offence-synth', `${n}.xml`)), kind: 'r81-synth', ov: { skill: 'main' } })
  for (const b of GOLDEN_BUILDS as Rec[]) {
    if (!BUFF_MODE_GOLDENS.includes(b.name)) continue
    for (const m of ['UNBUFFED', 'BUFFED', 'COMBAT']) s.push({ slug: `buff-${m}--${b.name.replace(/ /g, '-')}`, name: b.name, xml: readFileSync(b.xmlPath(roots)), kind: 'golden', ov: { buffMode: m } })
  }
  return filter ? s.filter((x) => x.slug.includes(filter)) : s
}
function ovChunk(ov?: Ov): string {
  if (!ov) return 'local OVERRIDE = nil\n'
  let t = 'local OVERRIDE = obj()\n'
  if (ov.skill === 'main') t += 'build.calcsTab.input.skill_number = build.mainSocketGroup\nOVERRIDE.skill_number = S(build.mainSocketGroup, 0)\n'
  if (ov.buffMode) t += `build.calcsTab.input.misc_buffMode = ${JSON.stringify(ov.buffMode)}\nOVERRIDE.misc_buffMode = ${JSON.stringify(ov.buffMode)}\n`
  return t
}
async function pool<T, R>(xs: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(xs.length)
  let next = 0
  await Promise.all(Array.from({ length: JOBS }, async () => { for (;;) { const i = next++; if (i >= xs.length) return; res[i] = await f(xs[i]!); if ((i + 1) % 10 === 0) console.error(`  done ${i + 1}/${xs.length}`) } }))
  return res
}
const producer = { generator: 'tools/lua-corpus/export-calcs.ts', image: LUAJIT_IMAGE, note: 'LuaJIT docker; wipeGlobalCache + calcsTab:BuildOutput() after load; calcsEnv output/breakdown; CalcSectionControl DrawString texts per section (player, minion)' }
async function pass(list: Scenario[]): Promise<{ files: Map<string, string>; rows: Rec[] }> {
  const probes = await pool(list, (s) => ljEval(loadChunk(s.xml, s.name) + `local function probe()\n${HELPERS}\n${ovChunk(s.ov)}${PROBE}\nend\nreturn probe()\n`))
  const files = new Map<string, string>()
  const rows: Rec[] = []
  list.forEach((s, i) => {
    const r = probes[i]!
    const used = new Set<string>()
    const amb = (r.r13GemNames ?? {}) as Rec
    const fx = { format: 1, producer, build: s.name, kind: s.kind, xmlSha256: sha(s.xml), calcsInputOverride: r.calcsInputOverride ? norm(r.calcsInputOverride) : null, hasMinion: r.hasMinion, mainSkill: r.mainSkill ?? null, calcsInput: norm(r.calcsInput),
      output: norm(r.output), breakdown: { player: normBreakdown(r.breakdown?.player ?? null, amb, used), minion: normBreakdown(r.breakdown?.minion ?? null, amb, used) },
      sectionsPlayer: sections(r.sectionsPlayer), sectionsMinion: sections(r.sectionsMinion), r13: { rowsSorted: ROW_SORTED.source, modListSorted: 'SkillBuffs|SkillDebuffs', gemSets: [...used].sort(byteOrder) } }
    const t = ser(fx) + '\n'
    if (Buffer.byteLength(t) > MAX_BYTES) throw new Error(`${s.slug}: ${Buffer.byteLength(t)} bytes > ${MAX_BYTES} -- STOP`)
    files.set(`${s.slug}.json`, t)
    const cells = (l: Rec[] | null): number => (l ? l.reduce((a, x) => a + x.draws.length, 0) : 0)
    const errs = (l: Rec[] | null): number => (l ? l.filter((x) => x.error).length : 0)
    rows.push({ slug: s.slug, bytes: Buffer.byteLength(t), outputKeys: Object.keys(fx.output as Rec).length, draws: cells(fx.sectionsPlayer) + cells(fx.sectionsMinion), sectionErrors: errs(fx.sectionsPlayer) + errs(fx.sectionsMinion) })
  })
  // the section data, once (any build loads it the same)
  const d = await ljEval(loadChunk(list[0]!.xml, list[0]!.name) + `local function probe()\n${HELPERS}\n${DATA_PROBE}\nend\nreturn probe()\n`)
  files.set('_sections.json', ser({ format: 1, producer: { ...producer, note: 'LoadModule("Modules/CalcSections") serialized (S)' }, sections: norm(d) }) + '\n')
  return { files, rows }
}

async function fixtures(filter?: string): Promise<void> {
  LUTS = await prepareLuts()
  const list = scenarios(filter)
  console.error(`pass 1 (${list.length} scenarios)`)
  const a = await pass(list)
  console.error('pass 2')
  const b = await pass(list)
  const diff = [...a.files.keys()].filter((k) => a.files.get(k) !== b.files.get(k))
  if (diff.length) {
    mkdirSync(SCRATCH, { recursive: true })
    for (const k of diff) { writeFileSync(resolve(SCRATCH, `calcs-a-${k}`), a.files.get(k)!); writeFileSync(resolve(SCRATCH, `calcs-b-${k}`), b.files.get(k)!) }
    console.log(JSON.stringify({ STOP: 'passes differ', diff }, null, 1)); process.exitCode = 1; return
  }
  mkdirSync(OUT, { recursive: true })
  const shas: Rec = {}
  for (const [n, t] of a.files) { writeFileSync(resolve(OUT, n), t, 'utf8'); shas[n] = sha(t) }
  const tot = a.rows.reduce((acc, r) => { acc.outputKeys += r.outputKeys; acc.draws += r.draws; acc.sectionErrors += r.sectionErrors; acc.maxBytes = Math.max(acc.maxBytes, r.bytes); return acc }, { outputKeys: 0, draws: 0, sectionErrors: 0, maxBytes: 0 })
  console.log(JSON.stringify({ scenarios: list.length, files: a.files.size, passesIdentical: true, combinedSha256: sha(Object.keys(shas).sort(byteOrder).map((k) => `${k}:${shas[k]}`).join('\n')), totals: tot, rows: a.rows }, null, 1))
}

// --- perf: LuaJIT CALCS buildOutput (after MAIN, fresh cache each run) and the section render ---------------------------------------------------
async function perf(): Promise<void> {
  LUTS = await prepareLuts()
  const RUNS = 7
  const out: Rec = { runs: RUNS, note: 'LuaJIT docker jit.on(), os.clock CPU ms, run 0 = warm-up (dropped). calcsBuildOutput = calcs.buildOutput(build, "CALCS") after wipeGlobalCache + buildOutput(MAIN) (untimed); render = UpdateSize + DrawContent of every section, player (+ minion)', builds: {} }
  for (const s of scenarios()) {
    const lua = String.raw`
local function now() return os.clock() * 1000 end
jit.on()
local tab = build.calcsTab
local R = { calcsBuildOutput = {}, render = {} }
local viewPort = { x = 0, y = 0, width = 1920, height = 1080 }
local rawDrawString = DrawString
for i = 0, ${RUNS} do
  wipeGlobalCache()
  tab.mainEnv = tab.calcs.buildOutput(build, "MAIN")
  local t0 = now()
  tab.calcsEnv = tab.calcs.buildOutput(build, "CALCS")
  local t1 = now()
  tab.calcsOutput = tab.calcsEnv.player.output
  local function renderAll(actor)
    for _, section in ipairs(tab.sectionList) do
      section.DrawControls = function() end
      section.x, section.y = 10000, 10000
      section:UpdateSize()
      section:UpdatePos()
      section:DrawContent(10000, 10000, section.width, actor, viewPort, false, true)
    end
  end
  local t2 = now()
  tab.input.showMinion = false
  renderAll(tab.calcsEnv.player)
  if tab.calcsEnv.minion then tab.input.showMinion = true renderAll(tab.calcsEnv.minion) tab.input.showMinion = false end
  local t3 = now()
  if i > 0 then R.calcsBuildOutput[i], R.render[i] = t1 - t0, t3 - t2 end
end
R.jit = jit.version
return R
`
    const r = await ljEval(loadChunk(s.xml, s.name) + `local function probe()\n${HELPERS}\n${ovChunk(s.ov)}${lua}\nend\nreturn probe()\n`)
    const st = (xs: number[]) => ({ median: median(xs), min: Math.min(...xs), n: xs.length })
    out.builds[s.slug] = { luajit: r.jit, calcsBuildOutput: st(r.calcsBuildOutput), render: st(r.render) }
    console.error(`perf ${s.slug}: CALCS buildOutput ${median(r.calcsBuildOutput).toFixed(2)} ms, render ${median(r.render).toFixed(2)} ms`)
  }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-calcs.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(Object.fromEntries(Object.entries(out.builds).map(([k, v]: [string, any]) => [k, { calcsBuildOutput: +v.calcsBuildOutput.median.toFixed(3), render: +v.render.median.toFixed(3) }])), null, 1)) // eslint-disable-line
}

if (process.argv[2] === 'perf') await perf()
else await fixtures(process.argv[2])
