// R6.1: LuaJIT truth for the skills (R6 gate (a)) and the perform checkpoints (R6 gate (b)) on the 6 goldens.
// One docker run per golden (PoB boot + loadBuildFromXML, timeless LUTs mounted as in export-env.ts -- MINEFIELD-RUST R5), then
//   * a FRESH calcs.initEnv(build, "MAIN"): socketGroupList (CalcSetup.lua:1448-1563 merge included), env.player.activeSkillList, mainSkill,
//     requirementsTable{,Gems}, env.modDB -- dumped BEFORE perform touches them;
//   * calcs.perform(env) on that same env with CP1..CP4 (see CPS) snapshotted by wrapping the globals/fields perform calls; perform is stopped at CP4;
//   * a second fresh initEnv + a full calcs.perform only to count how often each wrapped function runs at the top level (callCountsFull).
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-skills.ts        -> crates/pob-calc/tests/fixtures/<build>.skills.json (+ <build>.skills-<n>.json parts if > 1.45 MB)
//   node tools/lua-corpus/export-skills.ts perf   -> tools/.scratch/perf-skills.json
// Encoding: Lua numbers are `%.17g` strings (the raw lj_strfmt, as export-env.ts), Lua strings are strings, booleans are booleans; tables are
// objects keyed by tostring(key) (empty = {}), mods are `modLib.formatSourceMod` strings, gem instances/skills are referenced as "<group>:<gem>" /
// 1-based indices. Only normalisation: `function|table: 0x<addr>` -> `<addr>`. Docker libm = Alpine/musl (MINEFIELD-RUST R2).
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line

if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_450_000
const roots = resolveRoots()
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)

// --- the docker twin of export-env.ts ljEval (LUT mount + io.open/NewFileSearch shim, luajit-dump17.lua:35-59) --------------------------
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
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r61-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${lutsDir}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = execFileSync('docker', args, { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024 }) } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
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

// --- Lua: the checkpoint wrapper (shared by the fixture and the perf chunk) --------------------------------------------------------------
// CPS (CalcPerform.lua @ ed354c2f8): CP1 = after the first top-level doActorLifeMana(env.player) (the one at the end of doActorAttribsConditions, :546, called
// from :1944; :1945 is the 2nd, :3407 the conditional 3rd); CP2 = after the first doActorLifeManaReservation(env.player) (:2092; :3408 is the conditional 2nd);
// CP3 = ENTRY of calcs.defenceForConditionals(env, env.player) (:3484); CP4 = ENTRY of calcs.defence(env, env.player) (:3792). "Top level" = same env
// (minion calls have another actor, CalcMirages.lua:52 / Calcs.lua buildActiveSkill run perform on another env).
const WRAP = String.raw`
local calcs = build.calcsTab.calcs
local STOP = {}
local function runPerform(env, onCp, stopAtCp4)
  local cnt, log = { lm = 0, lmr = 0, dfc = 0, def = 0 }, {}
  local oLM, oLMR, oDFC, oDEF = doActorLifeMana, doActorLifeManaReservation, calcs.defenceForConditionals, calcs.defence
  doActorLifeMana = function(actor, ...)
    local r = oLM(actor, ...)
    if actor == env.player then
      cnt.lm = cnt.lm + 1; log[#log + 1] = "doActorLifeMana#" .. cnt.lm
      if cnt.lm == 1 and onCp then onCp("CP1", cnt, log) end
    end
    return r
  end
  doActorLifeManaReservation = function(actor, ...)
    local r = oLMR(actor, ...)
    if actor == env.player then
      cnt.lmr = cnt.lmr + 1; log[#log + 1] = "doActorLifeManaReservation#" .. cnt.lmr
      if cnt.lmr == 1 and onCp then onCp("CP2", cnt, log) end
    end
    return r
  end
  calcs.defenceForConditionals = function(e, actor, ...)
    if e == env and actor == env.player then
      cnt.dfc = cnt.dfc + 1; log[#log + 1] = "defenceForConditionals#" .. cnt.dfc
      if cnt.dfc == 1 and onCp then onCp("CP3", cnt, log) end
    end
    return oDFC(e, actor, ...)
  end
  calcs.defence = function(e, actor, ...)
    if e == env and actor == env.player then
      cnt.def = cnt.def + 1; log[#log + 1] = "defence#" .. cnt.def
      if cnt.def == 1 then
        if onCp then onCp("CP4", cnt, log) end
        if stopAtCp4 then error(STOP, 0) end
      end
    end
    return oDEF(e, actor, ...)
  end
  local ok, err = pcall(calcs.perform, env)
  doActorLifeMana, doActorLifeManaReservation, calcs.defenceForConditionals, calcs.defence = oLM, oLMR, oDFC, oDEF
  if not ok and err ~= STOP then error(err, 0) end
  return cnt, log
end
`

// --- Lua: the dump (fixture mode) -------------------------------------------------------------------------------------------------------
const DUMP = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local function N(v) return RAWF("%.17g", v) end
local fmtMod = modLib.formatSourceMod
local function className(t) local mt = getmetatable(t) if type(mt) == "table" then return rawget(mt, "_className") end end
local function isMod(t) return rawget(t, "name") ~= nil and rawget(t, "type") ~= nil and rawget(t, "flags") ~= nil and rawget(t, "keywordFlags") ~= nil end
-- ModList order comes from upstream pairs() over string-keyed tables, whose order varies per LuaJIT process (string-hash seed, D13): so mod lists are
-- stored as sorted multisets (keepOrder only for the per-name ModDB buckets, checked for determinism by the double run).
local function modsOf(list, keepOrder) local t = {} for i, m in ipairs(list) do t[i] = fmtMod(m) end if not keepOrder then table.sort(t) end return t end
local function dbSet(db) local t = {} for _, bucket in pairs(db.mods) do for _, m in ipairs(bucket) do t[#t + 1] = fmtMod(m) end end return t end
local SKIP = { actor = true, minion = true }
local D, REF
D = function(v, depth, seen)
  local tv = type(v)
  if tv == "number" then return N(v) end
  if tv == "string" or tv == "boolean" then return v end
  if tv ~= "table" then return "<" .. tv .. ">" end
  if isMod(v) then return fmtMod(v) end
  local cn = className(v)
  if cn == "ModList" then local o = obj() o["$ModList"] = modsOf(v) return o end
  if cn == "ModDB" then local o = obj() o["$ModDB"] = dbSet(v) return o end
  if cn then return "<class " .. cn .. ">" end
  if depth > 5 or seen[v] then return "<deep>" end
  seen[v] = true
  local o, nk = obj(), 0
  for k, x in pairs(v) do
    if type(k) == "table" then nk = nk + 1 elseif not SKIP[k] then
      local r = type(x) == "table" and REF[k] -- references into gems/items/skills (e.g. output.ReqStrItem = a requirementsTable entry)
      o[tostring(k)] = r and r(x) or D(x, depth + 1, seen)
    end
  end
  if nk > 0 then o["$tableKeys"] = N(nk) end
  seen[v] = nil
  return o
end
local function dbFull(d)
  local o, m, n = obj(), obj(), 0
  for name, bucket in pairs(d.mods) do m[name] = modsOf(bucket, true) n = n + #bucket end
  o.mods, o.count, o.conditions, o.multipliers = m, N(n), D(d.conditions, 1, {}), D(d.multipliers, 1, {})
  return o
end
local function snapActor(a)
  local o, n = obj(), 0
  for _, bucket in pairs(a.modDB.mods) do n = n + #bucket end
  o.modDB, o.modCount, o.conditions, o.multipliers, o.output = dbSet(a.modDB), N(n), D(a.modDB.conditions, 1, {}), D(a.modDB.multipliers, 1, {}), D(a.output, 1, {})
  return o
end
local function copyCnt(c) local o = obj() for k, v in pairs(c) do o[k] = N(v) end return o end

local groups = build.skillsTab.socketGroupList
local groupsBefore = #groups
wipeGlobalCache()
local env = calcs.initEnv(build, "MAIN")
local gemIdx, grpIdx, skillIdx = {}, {}, {}
for gi, g in ipairs(groups) do grpIdx[g] = gi for i, gem in ipairs(g.gemList) do gemIdx[gem] = gi .. ":" .. i end end
for i, sk in ipairs(env.player.activeSkillList) do skillIdx[sk] = i end
local function key(m, k) return function(v) return m[v] and (type(m[v]) == "number" and N(m[v]) or m[v]) or "<none>" end end
local function listOf(m) return function(v) local t = {} for i, x in ipairs(v) do t[i] = m[x] and (type(m[x]) == "number" and N(m[x]) or m[x]) or "<none>" end return t end end
local fields
local function effBrief(e) local o = obj() o.grantedEffect, o.level, o.quality, o.srcInstance = e.grantedEffect and e.grantedEffect.id, e.level and N(e.level), e.quality and N(e.quality), gemIdx[e.srcInstance] return o end
REF = {
  gemData = function(v) return v.id end, grantedEffect = function(v) return v.id end, skillGem = function(v) return v.id end,
  skillGrantedEffect = function(v) return v.id end, secondaryGrantedEffect = function(v) return v.id end,
  displayEffect = effBrief, supportEffect = effBrief, sourceItem = function(v) return v.name end, sourceNode = function(v) return D(v.id, 0, {}) end,
  srcInstance = key(gemIdx), socketGroup = key(grpIdx), summonSkill = key(skillIdx), sourceGem = key(gemIdx),
  displaySkillList = listOf(skillIdx), displaySkillListCalcs = function(v) return N(#v) end, triggeredBy = effBrief, actor = function() return "<actor>" end, minion = function() return "<minion>" end, displayGemList = listOf(gemIdx),
  isSupporting = function(v) local t = {} for g in pairs(v) do t[#t + 1] = gemIdx[g] or "<none>" end table.sort(t) return t end,
  propertyModList = function(v) local t = {} for i, e in ipairs(v) do t[i] = fmtMod(e.mod) end return t end,
  skillCfg = function(v) return fields(v, { skillTypes = true }) end, gemCfg = function(v) return fields(v) end,
}
REF.weapon1Cfg, REF.weapon2Cfg = REF.skillCfg, REF.skillCfg
-- explode sources are tree nodes / items: scalar fields only (they link into the whole tree)
local function srcRef(v) local o = obj() for k, x in pairs(v) do if type(x) ~= "table" and type(k) ~= "table" then o[tostring(k)] = D(x, 1, {}) end end return o end
REF.explodeSource = srcRef
-- buffList entries ({type, modList, ...}) and their mod lists are built from pairs() over hash tables: order differs per process (see modsOf) -> multisets
REF.modList = function(v) return modsOf(v) end
REF.extraSkillModList = REF.modList
REF.buffList = function(v) local t = {} for i, e in ipairs(v) do t[i] = fields(e) end return t end
REF.explodeSources = function(v) local t = {} for i, x in ipairs(v) do t[i] = srcRef(x) end return t end
fields = function(t, skip)
  local o = obj()
  for k, v in pairs(t) do
    if type(k) ~= "table" and not (skip and skip[k]) then
      local r = type(v) == "table" and REF[k]
      if r then o[tostring(k)] = r(v) else o[tostring(k)] = D(v, 1, {}) end
    end
  end
  return o
end
local function mlDump(ml) local o = obj() o.mods = modsOf(ml) return o end
local function skillDump(sk, i)
  local o = fields(sk, { activeEffect = true, supportList = true, effectList = true, skillModList = true, baseSkillModList = true })
  o.index = N(i)
  o.activeEffect = fields(sk.activeEffect)
  o.supportList, o.effectList = {}, {}
  for j, e in ipairs(sk.supportList) do o.supportList[j] = fields(e) end
  for j, e in ipairs(sk.effectList) do o.effectList[j] = fields(e) end
  o.skillModList = mlDump(sk.skillModList)
  o.baseSame = rawequal(sk.skillModList, sk.baseSkillModList)
  if not o.baseSame then o.baseSkillModList = mlDump(sk.baseSkillModList) end
  o.skillTypesIsCfgTypes = rawequal(sk.skillTypes, sk.skillCfg and sk.skillCfg.skillTypes)
  return o
end
local function reqDump(list) local t = {} for i, r in ipairs(list) do t[i] = fields(r) end return t end

local out = obj()
out.jit, out.groupsBefore, out.groupsAfter = jit.version, N(groupsBefore), N(#groups)
out.mainSocketGroup, out.buildMainSocketGroup = N(env.mainSocketGroup), N(build.mainSocketGroup)
out.socketGroups = {}
for gi, g in ipairs(groups) do
  local o = fields(g, { gemList = true })
  o.gemList = {}
  for i, gem in ipairs(g.gemList) do
    o.gemList[i] = fields(gem)
    -- D15: SkillsTab.lua LoadSkill picks the variant via pairs(gemsByGameId[gameId]) (first with the same grantedEffectId) -> per-process order
    local gd = gem.gemData
    local variants = gd and gd.gameId and build.data.gemsByGameId[gd.gameId]
    if variants then
      local c = {}
      for _, v in pairs(variants) do if v.grantedEffectId == gem.skillId then c[#c + 1] = v.id end end
      table.sort(c)
      if #c > 1 then o.gemList[i].ambiguousVariants = c end
    end
  end
  out.socketGroups[gi] = o
end
out.activeSkills = {}
for i, sk in ipairs(env.player.activeSkillList) do out.activeSkills[i] = skillDump(sk, i) end
out.mainSkill = N(skillIdx[env.player.mainSkill] or 0)
out.requirementsGems, out.requirementsItems, out.requirementsTable = reqDump(env.requirementsTableGems), reqDump(env.requirementsTableItems), reqDump(env.requirementsTable)
out.modDB = dbFull(env.modDB)
out.skillModCount = N((function() local n = 0 for _, sk in ipairs(env.player.activeSkillList) do n = n + #sk.skillModList end return n end)())
-- perform with checkpoints on the SAME env (initEnv state is already dumped above)
out.checkpoints = obj()
local cnt, log = runPerform(env, function(name, c, lg)
  local s = obj()
  s.player, s.enemy, s.calls, s.logLen = snapActor(env.player), snapActor(env.enemy), copyCnt(c), N(#lg)
  s.mainSkillModListCount = N(#env.player.mainSkill.skillModList)
  s.skillModCounts = {} -- per activeSkillList entry: [own mods of skillModList, own mods of baseSkillModList] (perform :1296 swaps skillModList for an empty child of baseSkillModList)
  for i, sk in ipairs(env.player.activeSkillList) do s.skillModCounts[i] = { N(#sk.skillModList), N(#sk.baseSkillModList) } end
  s.activeSkillCount = N(#env.player.activeSkillList)
  s.mainSkill = N(skillIdx[env.player.mainSkill] or 0)
  out.checkpoints[name] = s
end, true)
out.callLogToCp4 = log
out.callCountsToCp4 = copyCnt(cnt)
-- a second fresh env, full perform, counts only
wipeGlobalCache()
local env2 = calcs.initEnv(build, "MAIN")
local cnt2, log2 = runPerform(env2, nil, false)
out.callCountsFull, out.callLogFull = copyCnt(cnt2), log2
return out
`

const PERF = (runs: number): string => String.raw`
jit.on()
${WRAP}
local groups = build.skillsTab.socketGroupList
local RUNS = ${runs}
local a, b = {}, {}
for i = 0, RUNS do
  wipeGlobalCache()
  local t0 = os.clock()
  local env = calcs.initEnv(build, "MAIN")
  local t1 = os.clock()
  runPerform(env, nil, true)
  local t2 = os.clock()
  if i > 0 then a[i], b[i] = (t1 - t0) * 1000, (t2 - t1) * 1000 end -- run 0 = warm-up
end
return { jit = jit.version, status = jit.status() and "on" or "off", initEnv_ms = a, performToCp4_ms = b, nGroups = #groups }
`

// --- deterministic JSON -----------------------------------------------------------------------------------------------------------------
const normAddr = (s: string): string => s.replace(/(function|table): 0x[0-9a-f]+/g, '$1: <addr>')
function norm(v: any): any { // eslint-disable-line
  if (typeof v === 'string') return normAddr(v)
  if (Array.isArray(v)) return v.map(norm)
  if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [normAddr(k), norm(x)]))
  return v
}
function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
// dkjson turns an empty Lua array into [] and our empty objects keep __jsontype=object, so the shapes are already right; sort the mod multisets here
function sorted(a: string[]): string[] { return [...a].sort(byteOrder) }
const median = (xs: number[]): number => { const s = [...xs].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]! }

async function fixtures(): Promise<void> {
  const luts = await prepareLuts()
  let digest = ''
  try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
  const lutSha = sha(Buffer.concat(readdirSync(luts).sort().map((f) => Buffer.concat([Buffer.from(f), readFileSync(resolve(luts, f))]))))
  const producer = { generator: 'tools/lua-corpus/export-skills.ts', image: digest || LUAJIT_IMAGE, pobSha: git(roots.pobRoot), dataManifestSha256: sha(readFileSync(resolve(ROOT, 'data/manifest.json'))),
    lutSha, libm: 'docker = Alpine/musl (MINEFIELD-RUST R2)', cps: 'CP1/CP2 = after the first top-level call, CP3/CP4 = entry (CalcPerform.lua :546/:1944, :2092, :3484, :3792)' }
  mkdirSync(FIX, { recursive: true })
  const report: Rec = {}
  for (const b of GOLDEN_BUILDS) {
    const xml = readFileSync(b.xmlPath(roots))
    const r = norm(ljEval(loadChunk(xml, b.name) + `local function probe()\n${WRAP}\n${DUMP}\nend\nreturn probe()\n`, luts))
    if (process.env.RAW) writeFileSync(resolve(SCRATCH, 'skills-raw-' + b.name.replace(/ /g, '-') + '.json'), JSON.stringify(r))
    for (const cp of Object.values(r.checkpoints as Rec)) for (const who of ['player', 'enemy']) cp[who].modDB = sorted(cp[who].modDB)
    for (const sk of r.activeSkills as Rec[]) if (Array.isArray(sk.buffList)) sk.buffList = sk.buffList.map((e: Rec) => [ser(e), e] as const).sort((x: readonly [string, Rec], y: readonly [string, Rec]) => byteOrder(x[0], y[0])).map((x: readonly [string, Rec]) => x[1])
    // D15: gem instances with several candidate variants resolve differently per LuaJIT process -> the candidate ids / the name are replaced by a sentinel
    const amb = new Set<string>()
    for (const g of r.socketGroups as Rec[]) for (const gem of g.gemList as Rec[]) if (gem.ambiguousVariants) { gem.ambiguousVariants.forEach((x: string) => amb.add(x)) }
    const D15 = '<D15:ambiguous>'
    const mask = (v: any): any => { // eslint-disable-line
      if (typeof v === 'string') return amb.has(v) ? D15 : v
      if (Array.isArray(v)) return v.map(mask)
      if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, k === 'ambiguousVariants' ? x : mask(x)]))
      return v
    }
    if (amb.size > 0) {
      Object.assign(r, mask({ socketGroups: r.socketGroups, activeSkills: r.activeSkills, requirementsGems: r.requirementsGems, requirementsItems: r.requirementsItems, requirementsTable: r.requirementsTable, checkpoints: r.checkpoints, modDB: r.modDB }))
      for (const g of r.socketGroups as Rec[]) for (const gem of g.gemList as Rec[]) if (gem.ambiguousVariants) gem.nameSpec = D15
    }
    const slug = b.name.replace(/ /g, '-')
    const { jit, checkpoints, ...rest } = r
    const head = { format: 1, producer: { ...producer, luajit: jit }, build: b.name, xmlSha256: sha(xml) }
    const text = (o: Rec): string => ser(o) + '\n'
    if (process.env.SIZES) console.error(b.name, Object.fromEntries(Object.entries({ ...rest, checkpoints }).map(([k, v]) => [k, ser(v).length])), Object.fromEntries(Object.entries(checkpoints as Rec).map(([k, v]) => [k, ser(v).length])))
    const whole = text({ ...head, ...rest, checkpoints })
    const files: { file: string; bytes: number }[] = []
    const put = (name: string, o: Rec): void => {
      const t = text(o)
      if (Buffer.byteLength(t) > MAX_BYTES) throw new Error(`${name}: ${Buffer.byteLength(t)} bytes > ${MAX_BYTES} -- STOP (split further, card allows parts <= 1.5 MB)`)
      const file = resolve(FIX, name)
      writeFileSync(file, t, 'utf8')
      files.push({ file: file.replace(`${ROOT}\\`, ''), bytes: statSync(file).size })
    }
    if (Buffer.byteLength(whole) <= MAX_BYTES) put(`${slug}.skills.json`, { ...head, ...rest, checkpoints })
    else {
      put(`${slug}.skills.json`, { ...head, ...rest, parts: Object.keys(checkpoints).sort(byteOrder).map((k, i) => `${slug}.skills-${i + 1}.json (${k})`) })
      Object.keys(checkpoints).sort(byteOrder).forEach((k, i) => put(`${slug}.skills-${i + 1}.json`, { ...head, part: i + 1, checkpoints: { [k]: checkpoints[k] } }))
    }
    const modsOfCp = (k: string): number[] => ['player', 'enemy'].map((w) => Number(checkpoints[k][w].modCount))
    report[b.name] = {
      groups: (r.socketGroups as Rec[]).length, groupsBeforeInitEnv: r.groupsBefore, activeSkills: (r.activeSkills as Rec[]).length, mainSkill: r.mainSkill,
      skillModListModsTotal: r.skillModCount, modDBAfterInitEnv: r.modDB.count,
      modDBmods_playerEnemy: Object.fromEntries(Object.keys(checkpoints).sort().map((k) => [k, modsOfCp(k)])),
      callCountsToCp4: r.callCountsToCp4, callLogToCp4: r.callLogToCp4, callCountsFull: r.callCountsFull,
      requirementsGems: (r.requirementsGems as Rec[]).length, requirementsTable: (r.requirementsTable as Rec[]).length, files }
    console.error(`${b.name}: done`)
  }
  console.log(JSON.stringify(report, null, 1))
}

async function perf(): Promise<void> {
  const luts = await prepareLuts()
  const RUNS = 7
  const out: Rec = { runs: RUNS, note: 'LuaJIT docker: wipeGlobalCache + calcs.initEnv(build,"MAIN") (incl. skills) and calcs.perform stopped at CP4 (entry of calcs.defence); os.clock CPU ms, jit.on(), run 0 = warm-up (dropped)', builds: {} }
  for (const b of GOLDEN_BUILDS) {
    const r = ljEval(loadChunk(readFileSync(b.xmlPath(roots)), b.name) + `local function probe()\n${WRAP}\n${PERF(RUNS)}\nend\nreturn probe()\n`, luts)
    const a = r.initEnv_ms as number[]
    const p = r.performToCp4_ms as number[]
    out.builds[b.name] = { luajit: r.jit, jit: r.status, socketGroups: r.nGroups, initEnv_ms: a, performToCp4_ms: p, initEnv_median_ms: median(a), initEnv_min_ms: Math.min(...a), performToCp4_median_ms: median(p), performToCp4_min_ms: Math.min(...p) }
    console.error(`perf ${b.name}: initEnv ${median(a).toFixed(1)} ms, perform->CP4 ${median(p).toFixed(1)} ms`)
  }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-skills.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(out, null, 2))
}

if (process.argv[2] === 'perf') await perf()
else await fixtures()
