// R7-synth: synthetic LuaJIT corpus for the offence branches the 6 goldens never execute (FullDPS groups, minion as main skill, per-skill functions of
// Data/Skills/*.lua, triggers, mirages). Each scenario = one golden build with <Skills> replaced (+ optional gear / config inputs / custom mod block).
// Truth = desktop-path LuaJIT (docker): loadBuildFromXML -> wipeGlobalCache -> calcs.buildOutput(build,"MAIN") (= the oracle's MAIN pass, R7.1: golden == this).
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-offence-synth.ts coverage   -> tools/.scratch/offence-synth-coverage.json (line hook, jit.off; goldens + scenarios) + table
//   node tools/lua-corpus/export-offence-synth.ts            -> crates/pob-calc/tests/fixtures/offence-synth/<scenario>.{xml,luajit.json}; 2 runs, must be identical
//   node tools/lua-corpus/export-offence-synth.ts parse [f]  -> which mod lines modLib.parseMod accepts (authoring aid)
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
const run = promisify(execFile)
type Rec = Record<string, any> // eslint-disable-line
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures/offence-synth')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const POB = 'C:/pob-work/project_15/PathOfBuilding'
const POBTS = 'C:/pob-work/pob-ts'
const ORACLE = 'C:/pob-work/project_15/pob-engine-wasm'
const IMAGE = 'ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest'
const MAX_BYTES = 1_500_000
const CONCURRENCY = Number(process.env.SYNTH_JOBS ?? 5)
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const BASES: Record<string, string> = { gen: 'Generals Perforate Zerker', mir: 'Mirage Archer Toxic Rain', occ: 'OccVortex', cos: 'Dual Wield Cospris CoC', ds: 'Dual Savior' }
const GOLDENS = ['Dual Savior', 'Dual Wield Cospris CoC', 'Dual Wield Cospris CoC - Lethal Pride', 'Generals Perforate Zerker', 'Mirage Archer Toxic Rain', 'OccVortex']
const goldenPath = (g: string): string => (g === 'Dual Wield Cospris CoC - Lethal Pride' ? `${ORACLE}/test/fixtures/${g}.xml` : `${POB}/spec/TestBuilds/3.13/${g}.xml`)
// source files whose executed lines are recorded; `covers` needles are prefixed with the letter
const FILES: Record<string, string> = { O: 'Modules/CalcOffence.lua', T: 'Modules/CalcTriggers.lua', M: 'Modules/CalcMirages.lua', C: 'Modules/Calcs.lua' }

// --- scenarios (types; the list itself is built after the helpers) ------------------------------------------------------------------
type GemSpec = string | { g: string; lvl?: number; q?: number; part?: number; stage?: number; mine?: number; minion?: string; mskill?: number; mset?: number; off?: boolean; count?: number }
interface Group { gems: GemSpec[]; full?: boolean; slot?: string; label?: string; count?: number; off?: boolean; src?: string; active?: number }
// covers: "O|T|M|C:<needle>" (a line of CalcOffence/CalcTriggers/CalcMirages/Calcs.lua containing the needle that the scenario really executes) or "F:<closure path>"
interface Scenario { name: string; cat: string; base: keyof typeof BASES; groups?: Group[]; main?: number; level?: number; fullAll?: boolean; inputs?: Record<string, number | boolean | string>; mods?: string[]; spectres?: string[]; gear?: Record<string, string | null>; covers: string[]; r13?: R13 }
// R7-synth-2 (MINEFIELD-RUST R13): a scenario whose LuaJIT result depends on `pairs` over string keys (hash seeded per process). `probe` = a Lua
// expression that reveals, in the same process, which order this process took; the docker run is repeated until the order Rust takes (`want`,
// the data / insertion order) is seen -- the fixture's output is that run -- and until another order is seen too (max R13_TRIES runs), whose
// scalar keys / tables that differ are recorded (`r13.otherOrder`): the gate is "Rust = the LuaJIT run of Rust's order", the set is documented.
interface R13 { place: string; probe: string; want: string }
const R13_TRIES = 30
const SCENARIOS: Scenario[] = []
const FN_SKIP = new Set(['FrozenSweep', 'FrozenSweepAltX', 'PoisonousConcoctionAltX', 'ExplosiveTrap']) // never reached with a plain weapon / variant not selectable / ExplosiveTrap: shared statMap with AltX is pairs-order dependent (R13), the 2 runs differ -- R7-synth-2 covers FrozenSweep / AltX (a Frozen Legion group) and ExplosiveTrap / AltX (`r13`) as `s2-*`
const FN_NOGEM = new Set<string>() // closure skills without a gem that are still tried through the skillId-only <Gem>
const FN_ACTIVE: Record<string, number> = {} // which active skill of the group is the main one (Vaal gems: the Vaal skill)
const FN_OPTS: Record<string, Partial<Exclude<GemSpec, string>>> = {}

// --- data ---------------------------------------------------------------------------------------------------------------------
const gems: Rec = JSON.parse(readFileSync(resolve(ROOT, 'data/gems.json'), 'utf8')).gems
const closures: { path: string; src: string; from: number; to: number }[] = JSON.parse(readFileSync(resolve(ROOT, 'data/closures.json'), 'utf8')).closures.filter((c: Rec) => c.src.startsWith('Data/Skills/'))
const gemKeys = Object.keys(gems).sort(byteOrder)
function findGem(spec: string): Rec | undefined { // "@GrantedEffectId" | gem name (case-insensitive; the base variant wins over Alt*)
  if (spec.startsWith('@')) return gemKeys.map((k) => gems[k]).find((g) => g.grantedEffectId === spec.slice(1) && g.variantId === g.grantedEffectId) ?? gemKeys.map((k) => gems[k]).find((g) => g.grantedEffectId === spec.slice(1))
  const hits = gemKeys.map((k) => gems[k]).filter((g) => g.name.toLowerCase() === spec.toLowerCase())
  return hits.find((g) => g.variantId === g.grantedEffectId) ?? hits[0]
}
const xmlEsc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&apos;')
function gemXml(spec: GemSpec): string {
  const o = typeof spec === 'string' ? { g: spec } : spec
  const [name, lvl] = o.g.split(':')
  const gem = findGem(name!)
  const a: Record<string, string | number> = { enableGlobal1: 'true', enableGlobal2: 'false', qualityId: 'Default', quality: o.q ?? 0, enabled: o.off ? 'false' : 'true', count: o.count ?? 1 }
  if (gem) {
    a.level = Math.min(Number(lvl ?? o.lvl ?? 20), gem.naturalMaxLevel ?? 20); a.gemId = gem.gameId; a.variantId = gem.variantId; a.skillId = gem.grantedEffectId; a.nameSpec = gem.name
  } else if (name!.startsWith('@')) { a.level = o.lvl ?? 20; a.skillId = name!.slice(1); a.nameSpec = name!.slice(1) } // skill without a gem (item/spectre skill)
  else throw new Error(`unknown gem ${o.g}`)
  for (const [k, v] of [['skillPart', o.part], ['skillStageCount', o.stage], ['skillMineCount', o.mine], ['skillMinion', o.minion], ['skillMinionSkill', o.mskill], ['skillMinionItemSet', o.mset]] as const) if (v !== undefined) a[k] = v
  return `\t\t\t<Gem ${Object.entries(a).map(([k, v]) => `${k}="${xmlEsc(String(v))}"`).join(' ')}/>`
}
const groupXml = (g: Group): string => `\t\t<Skill mainActiveSkillCalcs="1" includeInFullDPS="${g.full ? 'true' : 'false'}" label="${xmlEsc(g.label ?? '')}" enabled="${g.off ? 'false' : 'true'}"${g.slot ? ` slot="${g.slot}"` : ''}${g.src ? ` source="${xmlEsc(g.src)}"` : ''}${g.count ? ` groupCount="${g.count}"` : ''} mainActiveSkill="${g.active ?? 1}">\n${g.gems.map(gemXml).join('\n')}\n\t\t</Skill>`

// --- scenario -> XML (LF; the desktop reads builds in text mode, MINEFIELD-RUST R7) ---------------------------------------------------
const baseCache: Record<string, string> = {}
function baseText(base: string): string { return (baseCache[base] ??= readFileSync(goldenPath(BASES[base]!), 'latin1').replace(/\r\n/g, '\n')) }
function scenarioXml(sc: Scenario): Buffer {
  let t = baseText(sc.base)
  if (sc.groups) {
    t = t.replace(/(<Skills [^>]*>)[\s\S]*?(<\/Skills>)/, (_m, a: string, b: string) => `${a}\n${sc.groups!.map(groupXml).join('\n')}\n\t${b}`)
    t = t.replace(/mainSocketGroup="\d+"/, `mainSocketGroup="${sc.main ?? 1}"`)
  }
  if (sc.fullAll) t = t.replace(/(<Skill [^>]*?) includeInFullDPS="[^"]*"/g, '$1').replace(/<Skill /g, '<Skill includeInFullDPS="true" ') // every group of the golden joins FullDPS
  if (sc.spectres) t = t.replace('	</Build>', `${sc.spectres.map((s) => `		<Spectre id="${xmlEsc(s)}"/>
`).join('')}	</Build>`) // build.spectreList for Raise Spectre
  if (sc.level) t = t.replace(/(<Build level=")\d+/, `$1${sc.level}`)
  const cfg = /\t<Config>([\s\S]*?)<\/Config>\n/.exec(t)
  if (!cfg) throw new Error('no <Config>')
  const inputs: Record<string, number | boolean | string> = {}
  for (const m of cfg[1]!.matchAll(/<Input name="([^"]+)" (boolean|number|string)="([^"]*)"\/>/g)) inputs[m[1]!] = m[2] === 'boolean' ? m[3] === 'true' : m[2] === 'number' ? Number(m[3]) : m[3]!.replace(/&apos;/g, "'")
  Object.assign(inputs, sc.inputs ?? {})
  const lines = ['\t<Config>']
  for (const k of Object.keys(inputs).sort(byteOrder)) { const v = inputs[k]!; lines.push(`\t\t<Input name="${k}" ${typeof v}="${xmlEsc(String(v))}"/>`) }
  if (sc.mods?.length) lines.push(`\t\t<CustomModifierBlock title="synth" enabled="true">${xmlEsc(sc.mods.join('\n'))}</CustomModifierBlock>`)
  lines.push('\t</Config>')
  t = t.slice(0, cfg.index) + lines.join('\n') + '\n' + t.slice(cfg.index + cfg[0].length)
  // gear: raw normal-rarity item texts; value null empties the slot. Items are inserted before the first top-level <Slot>/<ItemSet>.
  const slots = Object.keys(sc.gear ?? {})
  if (slots.length) {
    const anchor = /^\t\t<(Slot|ItemSet) /m.exec(t)
    if (!anchor) throw new Error('no <Slot>/<ItemSet> anchor')
    let items = ''
    slots.forEach((slot, i) => {
      const txt = sc.gear![slot]
      let id = 0
      if (txt !== null && txt !== undefined) { id = 1000 + i; items += `\t\t<Item id="${id}">\n\t\t\t${xmlEsc(txt).split('\n').join('\n\t\t\t')}\n\t\t</Item>\n` }
      const re = new RegExp(`(<Slot name="${slot}" itemId=")\\d+(")`, 'g')
      if (!re.test(t)) throw new Error(`no slot ${slot} in ${sc.base}`)
      t = t.replace(new RegExp(`(<Slot name="${slot}" itemId=")\\d+(")`, 'g'), `$1${id}$2`)
    })
    const at = /^\t\t<(Slot|ItemSet) /m.exec(t)!.index
    t = t.slice(0, at) + items + t.slice(at)
  }
  return Buffer.from(t, 'latin1')
}

// --- docker LuaJIT (twin of export-defence-synth ljEval) --------------------------------------------------------------------------
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
async function ljEval(code: string): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r7s-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const luts = resolve(SCRATCH, 'luts')
    mkdirSync(luts, { recursive: true })
    const args = ['run', '--rm', '-v', `${POB}:/workdir:ro`, '-v', `${POBTS}/tools/oracle/lua:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${luts}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = (await run('docker', args, { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024 })).stdout } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
const loadChunk = (xml: Buffer, name: string): string => `loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(name)})\nif __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n`

// --- Lua probes ---------------------------------------------------------------------------------------------------------------------
// DUMP: encoding of export-offence.ts (numbers `%.17g` strings, number-looking strings prefixed `$str:`, tables = objects keyed by tostring(key)); the top-level scalar
// split (nums/strs/bools) follows export-defence-synth so the Rust test can compare keys directly. pairs-order sets: none dumped (R13 -- the 2 runs prove it).
const DUMP = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local N = function(v) return RAWF("%.17g", v) end
local fmtMod = modLib.formatSourceMod
local function className(t) local mt = getmetatable(t) if type(mt) == "table" then return rawget(mt, "_className") end end
local function isMod(t) return rawget(t, "name") ~= nil and rawget(t, "type") ~= nil and rawget(t, "flags") ~= nil and rawget(t, "keywordFlags") ~= nil end
local D
D = function(v, depth, seen)
  local tv = type(v)
  if tv == "number" then return N(v) end
  if tv == "boolean" then return v end
  if tv == "string" then
    if v == "nan" or v == "inf" or v == "-inf" or v:find("^%-?[%d%.]+[eE]?[%+%-]?%d*$") then return "$str:" .. v end
    return v
  end
  if tv ~= "table" then return "<" .. tv .. ">" end
  if isMod(v) then return fmtMod(v) end
  local cn = className(v)
  if cn == "ModList" then local t = {} for i, m in ipairs(v) do t[i] = fmtMod(m) end table.sort(t) local o = obj() o["$ModList"] = t return o end
  if cn then return "<class " .. cn .. ">" end
  if depth > 8 or seen[v] then return "<deep>" end
  seen[v] = true
  local o, nk = obj(), 0
  for k, x in pairs(v) do if type(k) == "table" then nk = nk + 1 else o[tostring(k)] = D(x, depth + 1, seen) end end
  if nk > 0 then o["$tableKeys"] = N(nk) end
  seen[v] = nil
  return o
end
local function split(out)
  local r = { nums = obj(), strs = obj(), bools = obj(), tables = obj() }
  for k, v in pairs(out) do
    local t = type(v)
    if t == "number" then r.nums[k] = N(v) elseif t == "string" then r.strs[k] = v elseif t == "boolean" then r.bools[k] = v elseif t == "table" then r.tables[tostring(k)] = D(v, 0, {}) end
  end
  return r
end
local calcs = build.calcsTab.calcs
local oFull, fullRes = calcs.calcFullDPS, nil
calcs.calcFullDPS = function(...) local r = oFull(...) if not fullRes then fullRes = r end return r end
wipeGlobalCache()
local env = calcs.buildOutput(build, "MAIN")
calcs.calcFullDPS = oFull
local ms, res = env.player.mainSkill, obj()
res.output = split(env.player.output)
res.fullDPS = D(fullRes or {}, 0, {})
res.minion = env.minion and split(env.minion.output) or nil
res.minionName = env.minion and env.minion.minionData and env.minion.minionData.name or nil
local info = obj()
info.name, info.uuid = ms.activeEffect.grantedEffect.name, cacheSkillUUID(ms, env)
info.infoTrigger, info.infoMessage, info.infoMessage2, info.skillPartName = ms.infoTrigger, ms.infoMessage, ms.infoMessage2, ms.skillPartName
info.disable, info.hasMinion, info.hasMirage = ms.skillFlags.disable or false, ms.minion ~= nil, ms.mirage ~= nil
local list = {}
for i, sk in ipairs(env.player.activeSkillList) do
  list[i] = { name = sk.activeEffect.grantedEffect.name, uuid = cacheSkillUUID(sk, env), main = sk == ms, fullDPS = sk.socketGroup and sk.socketGroup.includeInFullDPS or false, disable = sk.skillFlags.disable or false, minion = sk.minion ~= nil, mirage = sk.mirage ~= nil, trigger = sk.infoTrigger }
end
info.activeSkills = list
res.mainSkill = info
return res
`
// coverage: line hook with jit.off() around ONE wipeGlobalCache + calcs.buildOutput(build,"MAIN"); executable lines from the bytecode of every nested prototype
const covPre = (clos: string): string => String.raw`
jit.off()
jit.flush()
local util = require("jit.util")
local FILES = { "Modules/CalcOffence.lua", "Modules/CalcTriggers.lua", "Modules/CalcMirages.lua", "Modules/Calcs.lua" }
local CLOS = ${clos}
local executable = {}
local function walk(p, set)
  local fi = util.funcinfo(p)
  for pc = 1, fi.bytecodes do local l = util.funcinfo(p, pc).currentline; if l and l > 0 then set[l] = true end end
  for n = -1, -(fi.gcconsts or 0), -1 do local k = util.funck(p, n); if type(k) == "proto" then walk(k, set) end end
end
for _, f in ipairs(FILES) do executable[f] = {}; walk(assert(loadfile(f)), executable[f]) end
`
const covPost = String.raw`
local hits, isT = {}, {}
local function hook(_, line)
  local f = debug.getinfo(2, "f").func
  local k = isT[f]
  if k == nil then
    k = false
    local s = debug.getinfo(2, "S").source
    for _, fn in ipairs(FILES) do if s:find(fn, 1, true) then k = fn end end
    if not k then local m = s:match("(Data/Skills/[%w_]+%.lua)$") if m then k = m end end
    isT[f] = k
  end
  if k then local h = hits[k] if not h then h = {} hits[k] = h end h[line] = true end
end
wipeGlobalCache()
debug.sethook(hook, "l")
build.calcsTab.calcs.buildOutput(build, "MAIN")
debug.sethook()
local out = { exec = {}, hits = {}, fn = {} }
for _, f in ipairs(FILES) do
  local e, h = {}, {}
  for l in pairs(executable[f]) do e[#e + 1] = l end
  for l in pairs(hits[f] or {}) do h[#h + 1] = l end
  table.sort(e) table.sort(h)
  out.exec[f], out.hits[f] = e, h
end
for i, c in ipairs(CLOS) do
  for l = c[2], c[3] do if hits[c[1]] and hits[c[1]][l] then out.fn[#out.fn + 1] = i - 1 break end end
end
return out
`
const closArg = '{' + closures.map((c) => `{${JSON.stringify(c.src)},${c.from},${c.to}}`).join(',') + '}'

// --- deterministic JSON -------------------------------------------------------------------------------------------------------------------
function ser(v: unknown): string {
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
const norm = (s: string): string => s.replace(/(function|table): 0x[0-9a-f]+/g, '$1: <addr>')
async function pool<T, R>(items: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(items.length)
  let next = 0
  await Promise.all(Array.from({ length: CONCURRENCY }, async () => {
    for (;;) { const i = next++; if (i >= items.length) return; res[i] = await f(items[i]!); console.error(`  done ${i + 1}/${items.length}`) }
  }))
  return res
}

// --- coverage cache + covers -------------------------------------------------------------------------------------------------------------
const COVER_CACHE = resolve(SCRATCH, process.env.SYNTH_CACHE ?? 'offence-synth-coverage.json')
const srcLines: Record<string, string[]> = Object.fromEntries(Object.entries(FILES).map(([l, f]) => [l, readFileSync(`${POB}/src/${f}`, 'latin1').split('\n')]))
const loadCache = (): Rec => (existsSync(COVER_CACHE) ? JSON.parse(readFileSync(COVER_CACHE, 'utf8')) : { entries: {}, exec: {} })
const ranges = (xs: number[]): string => { const r: string[] = []; for (let i = 0; i < xs.length;) { let j = i; while (j + 1 < xs.length && xs[j + 1]! === xs[j]! + 1) j++; r.push(j > i ? `${xs[i]}-${xs[j]}` : `${xs[i]}`); i = j + 1 } return r.join(',') }
function coverInfo(sc: Scenario, xml: Buffer, cache: Rec): { covers: string[]; fns: string[]; newLines: Rec; newLineCount: number } {
  const e = cache.entries[sc.name]
  if (!e || e.sha !== sha(xml)) throw new Error(`${sc.name}: coverage cache is stale: re-run \`coverage\``)
  const covers: string[] = []
  for (const c of sc.covers) {
    if (c.startsWith('F:')) {
      const idx = closures.findIndex((x) => x.path === c.slice(2))
      if (idx < 0) throw new Error(`${sc.name}: unknown closure ${c}`)
      if (!e.fn.includes(idx)) throw new Error(`${sc.name}: closure ${c.slice(2)} (${closures[idx]!.src}:${closures[idx]!.from}) was NOT called`)
      continue
    }
    const letter = c[0]!, needle = c.slice(2)
    const f = FILES[letter]
    if (!f || c[1] !== ':') throw new Error(`${sc.name}: bad cover ${c}`)
    const exec = new Set<number>(cache.exec[f]), hit = new Set<number>(e.hits[f])
    // R7-synth-2: "@<line>" names one line exactly (one-line config bodies of CalcTriggers.lua repeat word for word)
    const all = needle.startsWith('@') ? [Number(needle.slice(1))].filter((n) => exec.has(n)) : srcLines[letter]!.flatMap((l, i) => (l.includes(needle) && exec.has(i + 1) ? [i + 1] : []))
    if (!all.length) throw new Error(`${sc.name}: needle matches no executable ${f} line: ${needle}`)
    const lines = all.filter((n) => hit.has(n))
    if (!lines.length) throw new Error(`${sc.name}: needle ${needle} matches ${f} lines ${all.join(',')} but the scenario executes none of them`)
    covers.push(`${f}:${lines.join(',')} ${needle}`)
  }
  for (const i of e.fn as number[]) covers.push(`${closures[i]!.src}:${closures[i]!.from} ${closures[i]!.path}`)
  const gold = (letter: string): Set<number> => new Set<number>(GOLDENS.flatMap((g) => cache.entries[g]?.hits[FILES[letter]!] ?? (() => { throw new Error(`no cached golden ${g}`) })()))
  const newLines: Rec = {}
  let count = 0
  for (const letter of Object.keys(FILES)) {
    const g = gold(letter), ex = new Set<number>(cache.exec[FILES[letter]!])
    const fresh = (e.hits[FILES[letter]!] as number[]).filter((n) => ex.has(n) && !g.has(n))
    count += fresh.length
    if (fresh.length) newLines[FILES[letter]!] = ranges(fresh)
  }
  return { covers, fns: (e.fn as number[]).map((i) => closures[i]!.path), newLines, newLineCount: count }
}

async function coverage(): Promise<void> {
  mkdirSync(SCRATCH, { recursive: true })
  type Job = { label: string; xml: Buffer; name: string }
  const only = process.env.SYNTH_ONLY ? new RegExp(process.env.SYNTH_ONLY) : undefined // authoring aid: run a subset (use SYNTH_CACHE to keep a separate cache, merge with `merge`)
  const jobs: Job[] = [...GOLDENS.map((g) => ({ label: g, xml: Buffer.from(readFileSync(goldenPath(g), 'latin1').replace(/\r\n/g, '\n'), 'latin1'), name: g })), ...SCENARIOS.filter((s) => !only || only.test(s.name)).map((s) => ({ label: s.name, xml: scenarioXml(s), name: BASES[s.base]! }))]
  const names = jobs.map((j) => j.label)
  if (new Set(names).size !== names.length) throw new Error('duplicate scenario names')
  const cache = loadCache()
  const todo = jobs.filter((j) => cache.entries[j.label]?.sha !== sha(j.xml))
  console.error(`coverage: ${jobs.length} jobs, ${todo.length} to run (the rest cached by xml sha256)`)
  const fresh = await pool(todo, (j) => ljEval(covPre(closArg) + loadChunk(j.xml, j.name) + covPost))
  todo.forEach((j, i) => { cache.entries[j.label] = { sha: sha(j.xml), hits: fresh[i]!.hits, fn: fresh[i]!.fn }; Object.assign(cache.exec, fresh[i]!.exec) })
  writeFileSync(COVER_CACHE, JSON.stringify(cache) + '\n')
  const ran = SCENARIOS.filter((s) => cache.entries[s.name])
  const count = (set: Set<number>, f: string): number => [...set].filter((l) => (cache.exec[f] as number[]).includes(l)).length
  console.log('file\texecutable\tgoldens\tgoldens+synth')
  for (const f of Object.values(FILES)) {
    const g = new Set<number>(GOLDENS.flatMap((x) => cache.entries[x].hits[f])), u = new Set<number>([...g, ...ran.flatMap((s) => cache.entries[s.name].hits[f] as number[])])
    console.log(`${f}\t${cache.exec[f].length}\t${count(g, f)}\t${count(u, f)}`)
  }
  const called = new Set<number>(ran.flatMap((s) => cache.entries[s.name].fn as number[]))
  const goldCalled = new Set<number>(GOLDENS.flatMap((g) => cache.entries[g].fn as number[]))
  console.log(`skill functions called: goldens ${goldCalled.size}/${closures.length}; synth ${called.size}/${closures.length}; union ${new Set([...called, ...goldCalled]).size}/${closures.length}`)
  let bad = 0
  for (const [i, s] of ran.entries()) {
    const e = cache.entries[s.name]
    const miss = s.covers.flatMap((c) => { try { coverInfo({ ...s, covers: [c] }, scenarioXml(s), cache); return [] } catch (err) { return [(err as Error).message.replace(`${s.name}: `, '')] } })
    bad += miss.length ? 1 : 0
    console.log(`${i + 1}\t${s.name}\tfns ${e.fn.length}\t${miss.length ? 'MISS ' + miss.join(' ; ') : 'ok'}`)
  }
  console.log(`scenarios ${ran.length}/${SCENARIOS.length} run, with unmet covers ${bad}`)
}

async function r13Eval(sc: Scenario, r: R13): Promise<Rec> {
  const code = loadChunk(scenarioXml(sc), BASES[sc.base]!) + DUMP.replace(/return res\n$/, `res.r13Key = tostring(${r.probe})\nreturn res\n`)
  if (code === loadChunk(scenarioXml(sc), BASES[sc.base]!) + DUMP) throw new Error('r13Eval: DUMP tail changed')
  const byKey = new Map<string, Rec>()
  for (let i = 0; i < R13_TRIES && !(byKey.has(r.want) && byKey.size > 1); i++) {
    const o = await ljEval(code)
    const k = String(o.r13Key)
    delete o.r13Key
    if (!byKey.has(k)) byKey.set(k, o)
  }
  const got = byKey.get(r.want)
  if (!got) throw new Error(`${sc.name}: R13 order ${r.want} not seen in ${R13_TRIES} runs (seen ${[...byKey.keys()].join(',')}) -- STOP`)
  const otherOrder: Rec = {}
  for (const [k, o] of byKey) {
    if (k === r.want) continue
    const d: Rec = {}
    for (const part of ['nums', 'strs', 'bools', 'tables']) {
      const a = (got.output[part] ?? {}) as Rec, b = (o.output[part] ?? {}) as Rec
      d[part] = [...new Set([...Object.keys(a), ...Object.keys(b)])].filter((x) => norm(ser(a[x] ?? null)) !== norm(ser(b[x] ?? null))).sort(byteOrder)
    }
    d.minion = got.minion || o.minion ? [...new Set([...Object.keys(got.minion?.nums ?? {}), ...Object.keys(o.minion?.nums ?? {})])].filter((x) => got.minion?.nums?.[x] !== o.minion?.nums?.[x]).sort(byteOrder) : []
    d.fullDPS = norm(ser(got.fullDPS)) !== norm(ser(o.fullDPS))
    otherOrder[k] = d
  }
  return { ...got, r13: { class: 'lj-strhash', place: r.place, order: r.want, ordersSeen: [...byKey.keys()].sort(byteOrder), otherOrder } }
}

async function fixtures(): Promise<void> {
  mkdirSync(FIX, { recursive: true })
  const only = process.env.SYNTH_ONLY ? new RegExp(process.env.SYNTH_ONLY) : undefined // authoring aid: regenerate a subset (the 2-run check then covers the subset)
  const list = SCENARIOS.filter((s) => !only || only.test(s.name))
  const cache = loadCache()
  for (const s of SCENARIOS) coverInfo(s, scenarioXml(s), cache) // validate every cover BEFORE spending docker runs
  const image = (await run('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', IMAGE]).catch(() => ({ stdout: IMAGE }))).stdout.trim()
  const pobSha = (await run('git', ['-C', POB, 'rev-parse', '--short', 'HEAD'])).stdout.trim()
  const runs: string[][] = []
  for (let r = 0; r < 2; r++) {
    console.error(`run ${r + 1}/2`)
    const outs = await pool(list, (sc) => (sc.r13 ? r13Eval(sc, sc.r13) : ljEval(loadChunk(scenarioXml(sc), BASES[sc.base]!) + DUMP)))
    runs.push(outs.map((o, i) => {
      const sc = list[i]!, xml = scenarioXml(sc), ci = coverInfo(sc, xml, cache)
      const text = norm(ser({ build: BASES[sc.base], base: sc.base, scenario: sc.name, category: sc.cat, covers: ci.covers, fnCalled: ci.fns, newLines: ci.newLines, newLineCount: ci.newLineCount, groups: sc.groups ?? null, gear: sc.gear ?? {}, inputs: sc.inputs ?? {}, mods: sc.mods ?? [], xmlSha256: sha(xml),
        ...o, producer: { image, luajit: 'desktop-path LuaJIT docker (Alpine/musl libm, MINEFIELD-RUST R2), loadBuildFromXML -> wipeGlobalCache -> calcs.buildOutput(build,"MAIN")', pobSha, generator: 'tools/lua-corpus/export-offence-synth.ts' } })) + '\n'
      if (Buffer.byteLength(text) > MAX_BYTES) throw new Error(`${sc.name}: ${Buffer.byteLength(text)} bytes > 1.5 MB`)
      if (r === 0) { writeFileSync(resolve(FIX, `${sc.name}.xml`), xml); writeFileSync(resolve(FIX, `${sc.name}.luajit.json`), text) }
      return sha(text) + ' ' + sha(xml)
    }))
  }
  const same = runs[0]!.every((h, i) => h === runs[1]![i])
  console.log(`scenarios ${list.length}/${SCENARIOS.length}; run1 == run2 byte-identical: ${same}`)
  list.forEach((s, i) => { const bad = runs[0]![i] !== runs[1]![i]; console.log(`${s.name}\t${runs[0]![i]!.slice(0, 16)}\t${runs[1]![i]!.slice(0, 16)}${bad ? '\tDIFFERS' : ''}\t${coverInfo(s, scenarioXml(s), cache).covers.length} covers`) })
  if (!same) process.exitCode = 1
}

async function parse(file?: string): Promise<void> {
  const lines = (file ? readFileSync(file, 'utf8') : [...new Set(SCENARIOS.flatMap((s) => s.mods ?? []))].join('\n')).split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'))
  const lua = `local LINES = {${lines.map((l) => JSON.stringify(l)).join(',')}}
local out = {}
for i, line in ipairs(LINES) do
  local mods, extra = modLib.parseMod(line)
  out[i] = (mods and not extra) and "OK" or ("FAIL extra=" .. tostring(extra))
end
return out`
  const r = (await ljEval(lua)) as unknown as string[]
  lines.forEach((l, i) => console.log(`${r[i]!.startsWith('OK') ? 'OK  ' : 'FAIL'} | ${l}`))
}

// --- scenario list -----------------------------------------------------------------------------------------------------------------
// (c) one scenario per skill that owns a Data/Skills function: the skill is the only socket group (= the main skill); the weapon follows the skill's weaponTypes
const WEAPON: Record<string, string> = { bow: 'Crude Bow', hatchet: 'Rusted Hatchet', club: 'Driftwood Club', blade: 'Corroded Blade', fist: 'Nailed Fist' }
const item = (b: string): string => `Rarity: NORMAL
${WEAPON[b]}`
const fnIds = [...new Set(closures.map((c) => c.path.split('.')[1]!))]
for (const id of fnIds) {
  const gem = findGem('@' + id)
  if ((!gem && !FN_NOGEM.has(id)) || FN_SKIP.has(id)) continue
  const base: Scenario['base'] = /^(Barrage|BlastRain|ExplosiveArrow|ScourgeArrow|ShrapnelBallista|StormRain|TornadoShot|ToxicRain|VaalLightningArrow)/.test(id) ? 'mir' : /^(KineticFusillade)/.test(id) ? 'occ'
    : /^(BladeFlurry|ChargedDash|Cyclone|VaalCyclone|InfernalBlow|LancingSteel|MoltenStrike|Perforate|RageVortex|StaticStrike|PoisonousConcoction|Earthquake|FrozenSweep)/.test(id) ? 'cos' : 'occ'
  const gear: Record<string, string | null> | undefined = base !== 'cos' ? undefined : id === 'PoisonousConcoction' ? { 'Weapon 1': null, 'Weapon 2': null } : /^(Earthquake|FrozenSweep)/.test(id) ? { 'Weapon 1': item('club'), 'Weapon 2': null }
    : id === 'MoltenStrikeAltX' ? { 'Weapon 1': item('blade'), 'Weapon 2': null } : undefined
  const fn = closures.find((c) => c.path.startsWith(`skills.${id}.`) && /Func$/.test(c.path))!
  SCENARIOS.push({ name: `fn-${id}`, cat: 'skillfn', base, gear, groups: [{ gems: [gem ? { g: '@' + id, ...(FN_OPTS[id] ?? {}) } : '@' + id], active: FN_ACTIVE[id] }], covers: [`F:${fn.path}`] })
}

// (a) FullDPS, (b) minion as the main skill, (d) triggers, (e) mirages -- hand-written; every scenario is one golden build with <Skills> replaced
const g = (gems: GemSpec[], o: Partial<Group> = {}): Group => ({ gems, ...o })
const add = (name: string, cat: string, base: Scenario['base'], groups: Group[] | undefined, covers: string[], o: Partial<Scenario> = {}): void => { SCENARIOS.push({ name, cat, base, groups, covers, ...o }) }
const full = { full: true }
const FDP = 'C:t_insert(fullDPS.skills, { name = activeSkill.activeEffect.grantedEffect.name, dps = usedEnv.player.output.TotalDPS'
add('fd-occ-spells', 'fulldps', 'occ', [g(['Vortex']), g(['Arc'], full), g(['Fireball'], full), g(['Ice Nova', 'Added Cold Damage'], full)], [FDP])
add('fd-occ-dot', 'fulldps', 'occ', [g(['Vortex']), g(['Contagion'], full), g(['Essence Drain'], full), g(['Blight'], full), g(['Scorching Ray'], full)], [FDP, 'C:fullDPS.dotDPS = fullDPS.dotDPS + usedEnv.player.output.TotalDot', 'C:"Full DoT DPS"'])
add('fd-occ-poison-ignite', 'fulldps', 'occ', [g(['Vortex']), g(['Essence Drain', 'Chance to Poison'], full), g(['Fireball', 'Added Fire Damage'], full), g(['Righteous Fire'], full)], [FDP, 'C:fullDPS.igniteDPS = usedEnv.player.output.IgniteDPS'])
add('fd-occ-trap-mine-totem', 'fulldps', 'occ', [g(['Vortex']), g(['Fire Trap', 'Trap'], full), g(['Pyroclast Mine', 'Minefield'], full), g(['Fireball', 'Spell Totem'], full)], [FDP])
add('fd-occ-minions', 'fulldps', 'occ', [g(['Vortex']), g(['Raise Zombie', 'Minion Damage'], full), g(['Summon Skeletons'], full), g(['Fireball'], full)], [FDP, 'C:usedEnv.minion.output.TotalDPS * activeSkillCount'])
add('fd-occ-group-count', 'fulldps', 'occ', [g(['Vortex']), g(['Arc'], { full: true, count: 3 }), g(['Fireball'], { full: true, off: true })], [FDP])
add('fd-mir-bow-ballista', 'fulldps', 'mir', [g(['Toxic Rain']), g(['Toxic Rain'], full), g(['Caustic Arrow'], full), g(['Shrapnel Ballista', 'Ballista Totem'], full), g(['Tornado Shot'], full)], [FDP])
add('fd-mir-mirage', 'fulldps', 'mir', [g(['Toxic Rain']), g(['Rain of Arrows', 'Mirage Archer'], full), g(['Toxic Rain', 'Mirage Archer'], full)], [FDP, 'C:activeSkill.mirage.output.TotalDPS * mirageCount'])
add('fd-gen-melee', 'fulldps', 'gen', [g(['Perforate']), g(['Perforate', 'Impale'], full), g(['Cyclone'], full), g(['Lancing Steel', 'Culling Strike'], full), g(['Blade Flurry'], full)], [FDP, 'C:"Full Impale DPS"'])
add('fd-cos-coc', 'fulldps', 'cos', [g(['Frostbolt']), g(['Cyclone', 'Cast On Critical Strike', 'Freezing Pulse'], { full: true, slot: 'Body Armour' }), g(['Ice Spear'], full)], [FDP])
add('fd-all-ds', 'fulldps', 'ds', undefined, [FDP], { fullAll: true })
add('fd-all-cos', 'fulldps', 'cos', undefined, [FDP], { fullAll: true })
add('fd-all-gen', 'fulldps', 'gen', undefined, [FDP], { fullAll: true })
add('fd-all-mir', 'fulldps', 'mir', undefined, [FDP], { fullAll: true })
add('fd-all-occ', 'fulldps', 'occ', undefined, [FDP], { fullAll: true })
const mn = (name: string, gems: GemSpec[], o: Partial<Scenario> = {}, active?: number): void => add(name, 'minion', o.base ?? 'occ', [g(gems, { active })], ['O:output.SummonedMinionsPerCast = m_floor'], o)
mn('mn-raise-zombie', ['Raise Zombie', 'Minion Damage'])
mn('mn-raise-zombie-slamming', ['@RaiseZombieAltX'])
mn('mn-summon-skeletons', ['Summon Skeletons', 'Minion Damage'])
mn('mn-summon-skeletons-archers', ['@SummonSkeletonsAltX'])
mn('mn-summon-skeletons-mages', ['@SummonSkeletonsAltY'])
for (const [n, id] of [['chaos', 'SummonChaosGolem'], ['flame', 'SummonFlameGolem'], ['ice', 'SummonIceGolem'], ['lightning', 'SummonLightningGolem'], ['stone', 'SummonStoneGolem'], ['carrion', 'SummonCarrionGolem']] as const) mn(`mn-golem-${n}`, ['@' + id])
mn('mn-animate-weapon', ['Animate Weapon'])
mn('mn-animate-guardian', ['Animate Guardian'])
mn('mn-raging-spirit', ['Summon Raging Spirit'])
mn('mn-holy-relic', ['Summon Holy Relic'])
mn('mn-reaper', ['Summon Reaper'])
mn('mn-absolution', ['Absolution'])
mn('mn-dominating-blow', ['Dominating Blow'], { base: 'gen' })
for (const [n, minion, mskill] of [['axis-caster', 'Metadata/Monsters/Axis/AxisCaster'], ['bandit-bow-explosive', 'Metadata/Monsters/Bandits/BanditBowExplosiveArrow'], ['demon-blade-vortex', 'Metadata/Monsters/DemonModular/DemonModularBladeVortex'], ['hydra-mid', 'Metadata/Monsters/LeagueAzmeri/SpecialCorpses/Hydra/HydraMid', 2]] as const) mn(`mn-spectre-${n}`, [{ g: 'Raise Spectre', minion, mskill }], { spectres: [minion] })
mn('mn-zombie-ground-impact', [{ g: 'Raise Zombie', mskill: 3 }])
// skills granted by item lines (ExtraSkill) become an appended socket group
for (const [n, line] of [['death-wish', 'Grants Level 20 Death Wish Skill'], ['blood-sacrament', 'Grants Level 1 Blood Sacrament Skill'], ['herald-of-the-hive', 'Grants Level 30 Herald of the Hive Skill']] as const) add(`mn-granted-${n}`, 'skillfn', 'occ', [], [], { mods: [line] })
// (d) triggers: gem-based (same slot) and item-based (ExtraSupport lines of a rare weapon, socketed group slot = Weapon 1)
const TC: Record<string, string[]> = { 't-coc': ['T:triggerChance = triggerChance * (sourceHitChance or 0) / 100'], 't-coc-awakened': ['T:triggerChance = triggerChance * (sourceHitChance or 0) / 100'],
  't-cwc': ['T:if triggeredCD or cooldownOverride then'], 't-cwc-awakened': ['T:if triggeredCD or cooldownOverride then'], 't-cwdt': ['T:env.player.breakdown.CWDTThreshold = {'], 't-cwdt-multi': ['T:env.player.breakdown.CWDTThreshold = {'],
  't-comk': ['T:infoMessage2 = "DPS reported assuming Self-Cast"'], 't-hextouch': ['T:infoMessage2 = "DPS reported assuming Self-Cast"'], 't-cast-stunned': ['T:(chance to trigger)'], 't-cast-on-death': ['T:Triggered on Death'],
  't-spellslinger': ['T:output.SkillTriggerRate = 0'], 't-arcanist-brand': ['T:base activation cooldown of %s'], 't-cospri': ['T:due to dual wielding'], 't-mjolner': ['T:triggerName'],
  't-trap': ['O:output.TrapThrowingTime = 1 /'], 't-trap-cluster': ['O:output.TrapThrowingTime = 1 /'], 't-mine': ['O:if skillFlags.mine then'], 't-mine-charged': ['O:if skillFlags.mine then'],
  'mi-tawhoa-chosen': ['M:local skillTypeMatch = (skill.skillTypes[SkillType.Slam]'] }
const trig = (name: string, base: Scenario['base'], groups: Group[], covers: string[] = [], o: Partial<Scenario> = {}): void => add(name, 'trigger', base, groups, covers.length ? covers : (TC[name] ?? []), o)
trig('t-coc', 'cos', [g(['Cyclone', 'Cast On Critical Strike', 'Freezing Pulse'], { slot: 'Body Armour', active: 2 })])
trig('t-coc-awakened', 'cos', [g(['Cyclone', 'Awakened Cast On Critical Strike', 'Frostbolt'], { slot: 'Body Armour', active: 2 })])
trig('t-cwc', 'gen', [g(['Cyclone', 'Cast while Channelling', 'Frostbolt'], { slot: 'Boots', active: 2 })])
trig('t-cwc-awakened', 'gen', [g(['Cyclone', 'Awakened Cast While Channelling', 'Arc'], { slot: 'Boots', active: 2 })])
trig('t-cwdt', 'occ', [g(['Cast when Damage Taken', 'Ice Nova'], { slot: 'Helmet' })])
trig('t-cwdt-multi', 'occ', [g(['Cast when Damage Taken', 'Ice Nova', 'Arc', 'Fireball'], { slot: 'Helmet', active: 2 })])
trig('t-comk', 'cos', [g(['Cast on Melee Kill', 'Fireball'], { slot: 'Helmet' })])
trig('t-cast-stunned', 'occ', [g(['Cast when Stunned', 'Arc'], { slot: 'Helmet' })])
trig('t-cast-on-death', 'occ', [g(['Cast on Death', 'Arc'], { slot: 'Helmet' })])
trig('t-spellslinger', 'occ', [g(['Spellslinger', 'Fireball'], { slot: 'Weapon 1', active: 2 }), g(['Power Siphon'], { slot: 'Weapon 1' })])
trig('t-arcanist-brand', 'occ', [g(['Arcanist Brand', 'Fireball', 'Ice Nova'], { slot: 'Helmet', active: 2 })])
trig('t-hextouch', 'occ', [g(['Hextouch', 'Vulnerability'], { slot: 'Gloves' })])
trig('t-trap', 'occ', [g(['Fire Trap', 'Trap'])])
trig('t-trap-cluster', 'occ', [g(['Fire Trap', 'Trap', 'Cluster Traps'])])
trig('t-mine', 'occ', [g(['Pyroclast Mine', 'Minefield'])])
trig('t-mine-charged', 'occ', [g(['Stormblast Mine', 'Minefield', 'Charged Mines'])])
const ITEM = (base: string, mod: string): string => `Rarity: RARE\nSynth Item\n${base}\nImplicits: 0\n${mod}`
trig('t-mjolner', 'occ', [g(['Spark'], { slot: 'Weapon 1' }), g(['Cyclone'], { slot: 'Body Armour' })], [], { gear: { 'Weapon 1': ITEM('Driftwood Club', 'Trigger a Socketed Lightning Spell on Hit, with a 0.25 second Cooldown'), 'Weapon 2': null } })
trig('t-cospri', 'cos', [g(['Frostbolt'], { slot: 'Weapon 1' }), g(['Cyclone'], { slot: 'Body Armour' })], [], { gear: { 'Weapon 1': ITEM('Jewelled Foil', 'Trigger a Socketed Cold Spell on Melee Critical Strike, with a 0.25 second Cooldown') } })
trig('t-spell-on-attack', 'cos', [g(['Frostbolt'], { slot: 'Weapon 1' }), g(['Cyclone'], { slot: 'Body Armour' })], [], { gear: { 'Weapon 1': ITEM('Jewelled Foil', 'Trigger a Socketed Spell when you Attack with this Weapon, with a 0.25 second Cooldown') } })
trig('t-spell-on-bow-attack', 'mir', [g(['Arc'], { slot: 'Weapon 1' }), g(['Tornado Shot'], { slot: 'Body Armour' })], [], { gear: { 'Weapon 1': ITEM('Crude Bow', 'Trigger a Socketed Spell when you Attack with a Bow, with a 0.25 second Cooldown') } })
trig('t-spell-on-block', 'occ', [g(['Arc'], { slot: 'Weapon 2' })], [], { gear: { 'Weapon 2': ITEM('Rawhide Tower Shield', 'Trigger a Socketed Elemental Spell on Block, with a 0.25 second Cooldown') } })
// (e) mirages
add('mi-archer-rain-of-arrows', 'mirage', 'mir', [g(['Rain of Arrows', 'Mirage Archer'])], [])
add('mi-archer-tornado-shot', 'mirage', 'mir', [g(['Tornado Shot', 'Mirage Archer'])], [])
add('mi-saviour-more-damage', 'mirage', 'ds', undefined, ['M:SaviourMirageWarriorMaxCount'], { mods: ['50% increased Physical Damage', '+20% to Critical Strike Multiplier'] })
add('mi-generals-cry', 'mirage', 'gen', [g(["General's Cry", 'Lancing Steel'])], ['O:breakdown.WarcryEffectMod = {'])
add('mi-generals-cry-double-strike', 'mirage', 'gen', [g(["General's Cry", 'Double Strike'], { active: 1 })], ['O:breakdown.WarcryEffectMod = {'])
add('mi-sacred-wisps', 'mirage', 'occ', [g(['Power Siphon', 'Sacred Wisps'])], [])
add('mi-tawhoa-chosen', 'mirage', 'gen', [g(['@SummonMirageChieftain']), g(['Cyclone'])], TC['mi-tawhoa-chosen']!)
// (f) R7-synth-2 (tasks/R7-synth-2.md): the branches R7 / R8.5b left without a LuaJIT check -- skill functions outside FN_SKIP's reach, the
// `configTable` entries of CalcTriggers.lua no scenario executed (T@<line> = that exact line), a minion skill under a mirage
const s2 = (name: string, base: Scenario['base'], groups: Group[] | undefined, covers: string[], o: Partial<Scenario> = {}): void => add(name, 'synth2', base, groups, covers, o)
const CLUB = { 'Weapon 1': item('club'), 'Weapon 2': null }
const ET_R13: R13 = { place: 'Data.lua:1049-1068 pairs(data.skills): statMap._grantedEffect of the statMap ExplosiveTrapAltX shares (act_dex.lua:7326)', probe: 'data.skills.ExplosiveTrap.statMap._grantedEffect.id', want: 'ExplosiveTrapAltX' }
const EXPLODE = (chance: number, amount: string, type: string): string => `Enemies you Kill have a ${chance}% chance to Explode, dealing ${amount} of their maximum Life as ${type} Damage`
s2('s2-avenging-flame', 'occ', [g(['Searing Bond', 'Flamewood'], { active: 2 })], ['F:skills.AvengingFlame.preDamageFunc', 'T:skill.skillFlags.totem and slotMatch(env, skill)'])
for (const [n, id, part] of [['', 'ExplosiveTrap', 1], ['-p2', 'ExplosiveTrap', 2], ['-p3', 'ExplosiveTrap', 3], ['-altx', 'ExplosiveTrapAltX', 1], ['-altx-p2', 'ExplosiveTrapAltX', 2]] as const) s2(`s2-explosive-trap${n}`, 'occ', [g([{ g: '@' + id, part }])], ['F:skills.ExplosiveTrap.preDamageFunc'], { r13: ET_R13 })
for (const [n, gem, part] of [['', 'Frozen Legion', 1], ['-p2', 'Frozen Legion', 2], ['-altx', 'Frozen Legion of Rallying', 1]] as const) s2(`s2-frozen-sweep${n}`, 'cos', [g([{ g: gem, part }], { active: 2 })], [`F:skills.FrozenSweep${n === '-altx' ? 'AltX' : ''}.preDamageFunc`], { gear: CLUB })
s2('s2-minion-instability', 'occ', [g([{ g: 'Raise Zombie', mskill: 4 }])], ['F:skills.MinionInstability.preDamageFunc'], { mods: ['Minions Explode when reduced to Low Life, dealing 33% of their Life as Fire Damage to surrounding Enemies'] })
s2('s2-enemy-explode-item', 'occ', [g(['Arc'])], ['F:skills.EnemyExplode.preDamageFunc'], { main: 2, gear: { Amulet: ITEM('Jade Amulet', EXPLODE(20, 'a tenth', 'Physical')) } })
s2('s2-enemy-explode-avg', 'occ', [g([{ g: '@EnemyExplode', lvl: 1, part: 3 }])], ['F:skills.EnemyExplode.preDamageFunc'], { mods: [EXPLODE(20, 'a tenth', 'Physical'), EXPLODE(10, 'a quarter', 'Physical'), EXPLODE(5, 'a tenth', 'Physical')] })
s2('s2-enemy-explode-avg-2types', 'occ', [g([{ g: '@EnemyExplode', lvl: 1, part: 3 }])], ['F:skills.EnemyExplode.preDamageFunc'], { mods: [EXPLODE(20, 'a tenth', 'Physical'), EXPLODE(30, 'a quarter', 'Fire')],
  r13: { place: 'other.lua:6230 pairs(typeAmountChances) (string keys: ExplodeMod types)', probe: '(function() local t = {} t.Physical = {} t.Fire = {} for k in pairs(t) do return k end end)()', want: 'Physical' } })
// configTable entries (CalcTriggers.lua:902-1600)
s2('s2-doom-blast', 'occ', [g(['Despair', 'Impending Doom'], { active: 2 })], ['T:@1394'])
s2('s2-doom-blast-replacement', 'occ', [g(['Despair', 'Impending Doom'], { active: 2 })], ['T:env.player.modDB:NewMod("UsesCurseOverlaps"'], { inputs: { doomBlastSource: 'replacement' } })
s2('s2-snipe-standalone', 'mir', [g(['Snipe'])], ['T:env.player.mainSkill.skillData.hitTimeMultiplier = m_max(1, snipeStages) - 0.5'], { inputs: { configSnipeStages: 3 } })
s2('s2-snipe-source', 'mir', [g(['Snipe', 'Ice Shot'])], ['T:"DealNoLightning"'], { inputs: { configSnipeStages: 3 } })
s2('s2-snipe-triggered', 'mir', [g(['Snipe', 'Ice Shot'], { active: 2 })], ['T:skill.skillData.hitTimeMultiplier = snipeStages - 0.5'], { inputs: { configSnipeStages: 3 } })
s2('s2-snipe-not-enough', 'mir', [g(['Snipe', 'Ice Shot'], { active: 2 })], ['T:"Not enough Snipe stages to trigger this skill"'])
s2('s2-manaforged', 'mir', [g(['Tornado Shot', 'Manaforged Arrows']), g(['Rain of Arrows'])], ['T:@1386'])
s2('s2-battlemage', 'gen', [g(["Battlemage's Cry", 'Arc'], { active: 2 }), g(['Double Strike'])], ["T:grantedEffect.name ~= \"Battlemage's Cry\""])
s2('s2-kitava', 'occ', [g(['Arc'], { slot: 'Helmet' }), g(['Fireball'])], ['T:cachedManaCost or 0) >= requiredManaCost'], { gear: { Helmet: ITEM('Zealot Helmet', '50% chance to Trigger Socketed Spells when you Spend at least 0 Mana on an Upfront Cost to Use or Trigger a Skill, with a 0.1 second Cooldown') } })
s2('s2-kitava-100', 'occ', [g(['Arc'], { slot: 'Helmet' }), g(['Fireball'])], ['T:"KitavaRequiredManaCost"'], { gear: { Helmet: ITEM('Zealot Helmet', '50% chance to Trigger Socketed Spells when you Spend at least 100 Mana on an Upfront Cost to Use or Trigger a Skill, with a 0.1 second Cooldown') } })
s2('s2-focus', 'occ', [g(['Arc'], { slot: 'Helmet' })], ['T:local icdrFocus = calcLib.mod'], { gear: { Helmet: ITEM('Zealot Helmet', 'Trigger Socketed Spells when you Focus, with a 0.25 second Cooldown') } })
s2('s2-shattershard', 'occ', [g(['Arc'])], ['T:"Shattershard uses duration as pseudo cooldown"'], { main: 2, gear: { 'Weapon 2': 'Rarity: UNIQUE\nShattershard\nCrimson Round Shield\nImplicits: 0\nTrigger Level 20 Shield Shatter when you Block' } })
s2('s2-trigger-craft', 'occ', [g(['Arc'], { slot: 'Helmet' }), g(['Fireball'])], ['T:t_insert(triggeredSkills, packageSkillDataForSimulation(skill, env))'], { gear: { Helmet: ITEM('Zealot Helmet', 'Trigger a Socketed Spell when you Use a Skill, with a 8 second Cooldown') } })
s2('s2-automation', 'occ', [g(['Automation', 'Arc'], { active: 2 })], ['T:grantedEffect.name == "Automation"'])
s2('s2-autoexertion', 'gen', [g(['Autoexertion', 'Intimidating Cry'], { active: 2 })], ['T:grantedEffect.name == "Autoexertion"'])
s2('s2-combust', 'gen', [g(['Infernal Cry'], { active: 2 }), g(['Double Strike'])], ['T:output.InfernalUpTimeRatio'])
s2('s2-prismatic-burst', 'gen', [g(['Double Strike', 'Prismatic Burst'], { active: 2 })], ['T:@1361'])
s2('s2-voidstorm', 'mir', [g(['Rain of Arrows', 'Voidstorm'], { active: 2 })], ['T:@1364'])
s2('s2-shockwave', 'cos', [g(['Double Strike', 'Shockwave'], { active: 2 })], ['T:@1367'], { gear: CLUB })
s2('s2-void-shockwave', 'cos', [g(['Double Strike', 'Void Shockwave'], { active: 2 })], ['T:@1370'], { gear: CLUB })
s2('s2-fissure', 'cos', [g(['Ground Slam', 'Fissure'], { active: 2 })], ['T:@1278'], { gear: CLUB })
s2('s2-call-the-pyre', 'gen', [g(['Double Strike', 'Pyre'], { active: 2 })], ['T:@1377'], { inputs: { conditionEnemyIgnited: true } })
s2('s2-mark-on-hit', 'mir', [g(['Tornado Shot', 'Mark On Hit', "Sniper's Mark"], { active: 2 })], ['T:@1281'])
s2('s2-cast-on-ward-break', 'occ', [g(['Cast on Ward Break', 'Arc'])], ['T:@1201'])
s2('s2-intuitive-link', 'occ', [g(['Intuitive Link', 'Arc'], { active: 2 })], ['T:grantedEffect.name == "Intuitive Link" then'])
s2('s2-tempest-shield', 'occ', [g(['Tempest Shield'])], ['T:@1296'])
const DLINK: Partial<Scenario> = { mods: ['Link Skills can target Damageable Minions'], inputs: { linkedToMinion: true } }
// a minion skill under a mirage: General's Cry copies Dominating Blow (copyActiveSkill of a skill with a minion, CalcActiveSkill.lua:185-190); + Destructive Link
s2('s2-generals-cry-dominating-blow', 'gen', [g(["General's Cry", 'Dominating Blow'])], ['O:breakdown.WarcryEffectMod = {'])
s2('s2-generals-cry-dominating-blow-main', 'gen', [g(["General's Cry", 'Dominating Blow'], { active: 2 })], ['M:local mainSkillOutputCache = GlobalCache.cachedData[env.mode][uuid].Env.player.output'])
s2('s2-tawhoa-dominating-blow', 'gen', [g(['@SummonMirageChieftain']), g(['Dominating Blow'])], ['M:local skillTypeMatch = (skill.skillTypes[SkillType.Slam]', 'M:local newSkill, newEnv = calcs.copyActiveSkill(env, "CALCULATOR", mirageSkill)'])
s2('s2-saviour-dominating-blow', 'ds', [g(['Dominating Blow'])], ['M:SaviourMirageWarriorMaxCount', 'M:local newSkill, newEnv = calcs.copyActiveSkill(env, "CALCULATOR", mirageSkill)'])
// ... + Destructive Link: the minion reads `actor.parent.output.MainHand` (CalcOffence.lua:3079) after the mirage handed its output over
// (CalcMirages.lua:157 The Saviour) / after General's Cry's nested run
s2('s2-saviour-dominating-blow-dlink', 'ds', [g(['Dominating Blow']), g(['Destructive Link'])], ['M:env.player.output = newEnv.player.output', 'O:actor.parent.output.MainHand and actor.parent.output.MainHand.CritChance'], DLINK)
s2('s2-generals-cry-dominating-blow-main-dlink', 'gen', [g(["General's Cry", 'Dominating Blow'], { active: 2 }), g(['Destructive Link'])], ['O:actor.parent.output.MainHand and actor.parent.output.MainHand.CritChance'], DLINK)
s2('s2-dominating-blow-destructive-link', 'gen', [g(['Dominating Blow']), g(['Destructive Link'])], ['O:actor.parent.output.MainHand and actor.parent.output.MainHand.CritChance'], DLINK)
s2('s2-generals-cry-dominating-blow-dlink', 'gen', [g(["General's Cry", 'Dominating Blow']), g(['Destructive Link'])], ['O:actor.parent.output.MainHand and actor.parent.output.MainHand.CritChance'], DLINK)
// R8.5b breakdown branches without variants: KineticFusillade's sequential projectiles (damagePerProjectile with ProjectileCount > 1, act_int.lua:11021)
s2('s2-kinetic-fusillade-projectiles', 'occ', [g(['@KineticFusillade'])], ['F:skills.KineticFusillade.preDamageFunc'], { mods: ['Skills fire 2 additional Projectiles'] })


if (process.argv[2] === 'coverage') await coverage()
else if (process.argv[2] === 'parse') await parse(process.argv[3])
else await fixtures()
