// R6.2: LuaJIT truth for the gem/skill data and the gem model (docker, the ljEval twin of export-env.ts with the LUT mount; 4 in parallel).
//   (1) data -- every `data.gems[id]` (id, grantedEffect(Id), secondaryGrantedEffect(Id), naturalMaxLevel, name, gameId, variantId, ...),
//       `gemForSkill` (re-keyed by grantedEffect.id, D4) and its collisions (grantedEffects shared by >= 2 gems, who wins), `gemForBaseName`,
//       `gemsByGameId`; every granted effect: `levels` (canonical text, sha256 here), stats/constantStats/qualityStats, baseMods/qualityMods/
//       levelMods (`modLib.formatSourceMod`, as export-env.ts; "ERR:<LuaJIT message>" when it raises -- L24), the RESOLVED `statMap[stat]` of
//       every stat in stats/constantStats/qualityStats (after the lazy skillStatMapMeta copy + processMod, Data.lua:1033-1045), scalar flags
//       and skill-type sets; what LuaJIT does with the 4 malformed mods (format / lazy copy / ModList query);
//   (2) CalcTools helpers: doesTypeExpressionMatch / canGrantedEffectSupportActiveSkill (pairs active x support from the goldens + seeded
//       random ones), getGemStatRequirement (every gem x level requirement), buildSkillInstanceStats (every effect x 3 cases);
//       SkillsTab:ProcessGemLevel (every gem x defaultGemLevel x character level) and SkillsTab:FindSkillGem (golden nameSpecs + all gem names
//       + abbreviations, with the order-free set of first-level matches);
//   (3) the 6 goldens: `build.skillsTab` right after SkillsTab:Load (hooked -- before CalcSetup merges granted groups): every skill set,
//       every socket group and gem (all fields + the key set), imbuedSupportBySlot, activeSkillSetId.
// MINEFIELD-RUST R13: string-keyed `pairs` differ per LuaJIT process. Every part runs in RUNS (20) processes; `pairs` orders are sorted,
// a value that differs between processes is stored as {"$oneOf": [sorted alternatives]} (class `lj-strhash`; the recursion stops at a
// gem instance / statMap entry / mod list), the FindSkillGem "Ambiguous" text is checked against `matches` and stored as "Ambiguous".
// Two runs give byte-identical files (barring an alternative seen in none of 20 processes). Also writes the data patch
// `data-patches/skill-levels-shape.json` (`#levels`, `next(levels)` per effect -- integer keys, equal in all processes).
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-gemdata.ts -> crates/pob-data/tests/fixtures/gemdata-{gems,skills-1,skills-2,helpers,groups}.json
//   (full canonical texts of the hashed parts -> tools/.scratch/gemdata-full.json, raw processes -> tools/.scratch/gemdata-runs.json)
import { execFile, execFileSync } from 'node:child_process'
import { promisify } from 'node:util'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line

if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-data/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const RUNS = 20
const PARALLEL = 4
const roots = resolveRoots()
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
const sha = (s: string): string => createHash('sha256').update(s, 'utf8').digest('hex')

// --- the docker twin of export-env.ts ljEval (LUT mount + io.open/NewFileSearch shim) -----------------------------------------
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
const execFileP = promisify(execFile)
async function ljEval(code: string, lutsDir: string, extra: Record<string, string> = {}): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r62-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    for (const [f, text] of Object.entries(extra)) writeFileSync(resolve(tmp, f), text)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${lutsDir}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = (await execFileP('docker', args, { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024 })).stdout } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
const luaStr = (s: string): string => luaBytes(Buffer.from(s, 'utf8'))

// --- shared Lua helpers ---------------------------------------------------------------------------------------------------------
const LUA_HELPERS = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local function N(x) if type(x) == "number" then return RAWF("%.17g", x) end return tostring(x) end
local function sortedKeys(t)
  local ks = {}
  for k in pairs(t) do ks[#ks + 1] = k end
  table.sort(ks, function(a, b) if type(a) == type(b) then return a < b end return type(a) == "number" end)
  return ks
end
local MOD_KEYS = { name = true, type = true, value = true, flags = true, keywordFlags = true, source = true }
-- formatSourceMod (as export-env.ts) + any extra named field; "ERR" when formatSourceMod itself raises (L24 malformed mods)
local function fmtMod(m)
  local ok, s = pcall(modLib.formatSourceMod, m)
  if not ok then s = "ERR:" .. tostring(s) end
  for _, k in ipairs(sortedKeys(m)) do
    if type(k) == "string" and not MOD_KEYS[k] then s = s .. "|" .. k .. "=" .. N(m[k]) end
  end
  return s
end
local function fmtElem(e)
  if e.name then return fmtMod(e) end
  local p = {}
  for i, m in ipairs(e) do p[i] = fmtMod(m) end
  local s = "{" .. table.concat(p, ";") .. "}"
  for _, k in ipairs(sortedKeys(e)) do if type(k) == "string" then s = s .. k .. "=" .. N(e[k]) end end
  return s
end
local function fmtList(list)
  if not list then return nil end
  local out = {}
  for i, e in ipairs(list) do out[i] = fmtElem(e) end
  return out
end
local function fmtEntry(e)
  if not e then return false end
  local out = {}
  for _, x in ipairs(e) do out[#out + 1] = fmtElem(x) end
  for _, k in ipairs(sortedKeys(e)) do if type(k) == "string" then out[#out + 1] = k .. "=" .. N(e[k]) end end
  return out
end
local function fmtLevel(lv)
  local p = {}
  for i = 1, #lv do p[#p + 1] = N(lv[i]) end
  local s = table.concat(p, ",")
  for _, k in ipairs(sortedKeys(lv)) do
    if type(k) ~= "number" then
      local v = lv[k]
      if type(v) == "table" then
        local q = {}
        for _, kk in ipairs(sortedKeys(v)) do q[#q + 1] = tostring(kk) .. "=" .. N(v[kk]) end
        v = "{" .. table.concat(q, ",") .. "}"
      else
        v = N(v)
      end
      s = s .. ";" .. k .. "=" .. v
    elseif k > #lv then
      s = s .. ";[" .. k .. "]=" .. N(lv[k])
    end
  end
  return s
end
local function setList(t)
  if not t then return nil end
  local out = {}
  for _, k in ipairs(sortedKeys(t)) do out[#out + 1] = tostring(k) .. "=" .. tostring(t[k]) end
  return out
end
local function numList(t)
  if not t then return nil end
  local out = {}
  for i, v in ipairs(t) do out[i] = N(v) end
  return out
end
local function pairList(t)
  if not t then return nil end
  local out = {}
  for i, v in ipairs(t) do out[i] = tostring(v[1]) .. "=" .. N(v[2]) end
  return out
end
`

// --- (1)+(2): data + helpers --------------------------------------------------------------------------------------------------
const DATA_LUA = String.raw`
local skillsTabCls = common.classes.SkillsTab
if not skillsTabCls then LoadModule("Classes/SkillsTab"); skillsTabCls = common.classes.SkillsTab end
local orders = { gems = {}, skills = {} }
for id in pairs(data.gems) do orders.gems[#orders.gems + 1] = id end
for id in pairs(data.skills) do orders.skills[#orders.skills + 1] = id end
local gems = {}
for id, g in pairs(data.gems) do
  local tags = {}
  for _, k in ipairs(sortedKeys(g.tags or {})) do tags[#tags + 1] = k end
  gems[id] = {
    id = g.id, grantedEffect = g.grantedEffect and g.grantedEffect.id, grantedEffectId = g.grantedEffectId,
    secondaryGrantedEffect = g.secondaryGrantedEffect and g.secondaryGrantedEffect.id or nil, secondaryGrantedEffectId = g.secondaryGrantedEffectId,
    grantedEffectList = #g.grantedEffectList, naturalMaxLevel = N(g.naturalMaxLevel), name = g.name, gameId = g.gameId, variantId = g.variantId,
    vaalGem = g.vaalGem, baseTypeName = g.baseTypeName, tagString = g.tagString, tags = tags, secondaryEffectName = g.secondaryEffectName,
    req = N(g.reqStr) .. "," .. N(g.reqDex) .. "," .. N(g.reqInt),
  }
end
local gemForSkill, nGemForSkill = {}, 0
for ge, id in pairs(data.gemForSkill) do gemForSkill[ge.id] = id; nGemForSkill = nGemForSkill + 1 end
local byGe = {}
for id, g in pairs(data.gems) do byGe[g.grantedEffect] = byGe[g.grantedEffect] or {}; table.insert(byGe[g.grantedEffect], id) end
local collisions = {}
for ge, ids in pairs(byGe) do if #ids > 1 then collisions[ge.id] = { ids = ids, winner = data.gemForSkill[ge] } end end
local gemsByGameId = {}
for gid, vs in pairs(data.gemsByGameId) do
  local ord = {}
  for vid, g in pairs(vs) do ord[#ord + 1] = vid .. "=" .. g.id end
  gemsByGameId[gid] = ord
end
local skills = {}
local SCALARS = { "name", "color", "support", "plusVersionOf", "cannotBeSupported", "supportGemsOnly", "fromItem", "fromTree", "ignoreMinionTypes",
  "isTrigger", "baseEffectiveness", "incrementalEffectiveness", "modSource", "id", "hidden", "castTime", "unsupported", "legacy", "skillTotemId" }
for id, ge in pairs(data.skills) do
  local levels = {}
  for _, k in ipairs(sortedKeys(ge.levels)) do levels[#levels + 1] = tostring(k) .. ":" .. fmtLevel(ge.levels[k]) end
  local statMap = {}
  local function resolve(stat) if statMap[stat] == nil then statMap[stat] = fmtEntry(ge.statMap[stat]) end end
  for _, s in ipairs(ge.stats or {}) do resolve(s) end
  for _, s in ipairs(ge.constantStats or {}) do resolve(s[1]) end
  for _, s in ipairs(ge.qualityStats or {}) do resolve(s[1]) end
  local sc = {}
  for _, k in ipairs(SCALARS) do if ge[k] ~= nil then sc[#sc + 1] = k .. "=" .. N(ge[k]) end end
  skills[id] = {
    levels = table.concat(levels, "\n"), nLevels = #ge.levels, nextLevel = (next(ge.levels)),
    stats = ge.stats, constantStats = pairList(ge.constantStats), qualityStats = pairList(ge.qualityStats),
    baseMods = fmtList(ge.baseMods), qualityMods = fmtList(ge.qualityMods), levelMods = fmtList(ge.levelMods), statMap = statMap,
    scalars = sc, hasGlobalEffect = ge.hasGlobalEffect, skillTypes = setList(ge.skillTypes), minionSkillTypes = setList(ge.minionSkillTypes),
    addSkillTypes = numList(ge.addSkillTypes), excludeSkillTypes = numList(ge.excludeSkillTypes), requireSkillTypes = numList(ge.requireSkillTypes),
    weaponTypes = setList(ge.weaponTypes), notMinionStat = ge.notMinionStat,
  }
end

-- (2) helpers
local H
do local f = assert(io.open("/code/cases.json", "r")); H = require("dkjson").decode(f:read("*a")); f:close() end
local function copy(t) local o = {} for k, v in pairs(t or {}) do o[k] = v end return o end
local supportOut = {}
for i, c in ipairs(H.support) do
  local sup, act = data.skills[c.s], data.skills[c.a]
  local actor = { weaponData1 = { type = c.w1, countsAsAll1H = c.all1h or nil }, weaponData2 = { type = c.w2 } }
  actor.enemy = { player = c.notPlayer and {} or actor }
  local activeSkill = {
    activeEffect = { grantedEffect = act, gemData = c.gem and {} or nil, srcInstance = c.srcFromItem and { fromItem = true } or nil },
    skillTypes = copy(act.skillTypes), minionSkillTypes = act.minionSkillTypes and copy(act.minionSkillTypes) or nil, actor = actor,
  }
  local ok, r = pcall(calcLib.canGrantedEffectSupportActiveSkill, sup, activeSkill, c.imbued, c.granted)
  local req = sup.requireSkillTypes and sup.requireSkillTypes[1] and calcLib.doesTypeExpressionMatch(sup.requireSkillTypes, act.skillTypes or {}, act.minionSkillTypes)
  local exc = sup.excludeSkillTypes and sup.excludeSkillTypes[1] and calcLib.doesTypeExpressionMatch(sup.excludeSkillTypes, act.skillTypes or {})
  supportOut[i] = (ok and tostring(r) or "ERR") .. "|" .. tostring(req) .. "|" .. tostring(exc)
end
local reqOut = {}
for i, c in ipairs(H.req) do reqOut[i] = N(calcLib.getGemStatRequirement(c[1], c[2], c[3])) end
local statsOut = {}
for i, c in ipairs(H.stats) do
  local ok, st = pcall(calcLib.buildSkillInstanceStats, { level = c.level, quality = c.quality, actorLevel = c.actorLevel }, data.skills[c.id])
  if ok then
    local p = {}
    for _, k in ipairs(sortedKeys(st)) do p[#p + 1] = k .. "=" .. N(st[k]) end
    statsOut[i] = table.concat(p, ";")
  else
    statsOut[i] = "ERR"
  end
end
local gemLevel = {}
local MODES = { "normalMaximum", "corruptedMaximum", "awakenedMaximum", "levelOne", "characterLevel" }
for id, g in pairs(data.gems) do
  local r = {}
  for _, mode in ipairs(MODES) do
    for _, cl in ipairs(mode == "characterLevel" and { 1, 12, 38, 70, 100 } or { 1 }) do
      local ok, v = pcall(skillsTabCls.ProcessGemLevel, { defaultGemLevel = mode, build = { characterLevel = cl } }, g)
      r[#r + 1] = ok and N(v) or "ERR"
    end
  end
  r[#r + 1] = N(skillsTabCls.ProcessGemLevel({ defaultGemLevel = "normalMaximum", build = {} }, g, true))
  gemLevel[id] = table.concat(r, ",")
end
-- the pattern list of SkillsTab.lua:1105-1111, VERBATIM: the names matching the first pattern level that matches anything (order-free)
local function findMatches(nameSpec)
  local patternList = {
    "^ "..nameSpec:gsub("%a", function(a) return "["..a:upper()..a:lower().."]" end).."$", -- Exact match (case-insensitive)
    "^"..nameSpec:gsub("%a", " %0%%l+").."$", -- Simple abbreviation ("CtF" -> "Cold to Fire")
    "^ "..nameSpec:gsub(" ",""):gsub("%l", "%%l*%0").."%l+$", -- Abbreviated words ("CldFr" -> "Cold to Fire")
    "^"..nameSpec:gsub(" ",""):gsub("%a", ".*%0"), -- Global abbreviation ("CtoF" -> "Cold to Fire")
    "^"..nameSpec:gsub(" ",""):gsub("%a", function(a) return ".*".."["..a:upper()..a:lower().."]" end), -- Case insensitive global abbreviation ("ctof" -> "Cold to Fire")
  }
  for _, pattern in ipairs(patternList) do
    local m = {}
    for _, gemData in pairs(data.gems) do if (" "..gemData.name):match(pattern) then m[#m + 1] = gemData.name end end
    if #m > 0 then table.sort(m) return m end
  end
  return {}
end
local find = {}
for i, spec in ipairs(H.find) do
  local ok, err, gem = pcall(skillsTabCls.FindSkillGem, { build = { data = data } }, spec)
  find[i] = ok and { err = err, gem = gem and gem.id, matches = findMatches(spec) } or { err = "ERR:" .. tostring(err) }
end
-- L24 malformed mods: what LuaJIT does on meeting them -- formatSourceMod, the lazy statMap copy (processMod), a ModList query
local malformed = {}
local function perr(f) local ok, e = pcall(f) return ok and "ok" or tostring(e) end
for _, stat in ipairs({ "cannot_be_stunned_while_leeching", "spell_damage_+%_per_10_int" }) do
  local raw = data.skillStatMap[stat][1]
  malformed[stat] = {
    format = perr(function() return modLib.formatSourceMod(raw) end),
    lazy = perr(function() return data.skills.Fireball.statMap[stat] end),
    query = perr(function() local l = new("ModList"):ModList(); l:AddMod(copyTable(raw)); l:Sum("BASE", nil, raw.name); l:List(nil, raw.name) end),
  }
end
do
  local m = data.skills.SupportBloodsoakedBanner.baseMods[3]
  malformed.SupportBloodsoakedBanner = {
    format = perr(function() return modLib.formatSourceMod(m) end),
    query = perr(function() local l = new("ModList"):ModList(); l:AddMod(copyTable(m)); l:Sum("BASE", nil, m.name) end),
  }
end
return {
  malformed = malformed,
  jit = jit.version, orders = orders, gems = gems, gemForSkill = gemForSkill, nGemForSkill = nGemForSkill, collisions = collisions,
  gemForBaseName = data.gemForBaseName, gemsByGameId = gemsByGameId, gemGrantedEffectIdForVaalGemId = data.gemGrantedEffectIdForVaalGemId,
  gemVaalGemIdForBaseGemId = data.gemVaalGemIdForBaseGemId, skills = skills,
  helpers = { support = supportOut, req = reqOut, stats = statsOut, gemLevel = gemLevel, find = find },
}
`

// --- (3) goldens: build.skillsTab right after SkillsTab:Load ---------------------------------------------------------------------
const GROUPS_LUA = String.raw`
local cls = common.classes.SkillsTab
if not cls then LoadModule("Classes/SkillsTab"); cls = common.classes.SkillsTab end
local function keysOf(t) local ks = {} for _, k in ipairs(sortedKeys(t)) do ks[#ks + 1] = tostring(k) end return ks end
local function dumpGem(g)
  local d = { keys = keysOf(g) }
  for k, v in pairs(g) do
    if k == "gemData" then d.gemData = v.id
    elseif k == "grantedEffect" then d.grantedEffect = v.id
    elseif type(v) ~= "table" and type(v) ~= "function" then d[k] = N(v) end
  end
  return d
end
local function dumpGroup(sg)
  local d = { keys = keysOf(sg), gems = {} }
  for k, v in pairs(sg) do if type(v) ~= "table" then d[k] = N(v) end end
  for i, g in ipairs(sg.gemList) do d.gems[i] = dumpGem(g) end
  return d
end
local SNAP
local origLoad = cls.Load
cls.Load = function(self, xml, fileName)
  origLoad(self, xml, fileName)
  local sets = {}
  for _, id in ipairs(self.skillSetOrderList) do
    local s = self.skillSets[id]
    local groups = {}
    for i, sg in ipairs(s.socketGroupList) do groups[i] = dumpGroup(sg) end
    sets[#sets + 1] = { id = N(id), title = s.title, groups = groups }
  end
  local imbued = {}
  for slot, ge in pairs(self.imbuedSupportBySlot) do imbued[slot] = ge.id end
  SNAP = { sets = sets, activeSkillSetId = N(self.activeSkillSetId), active = #self.socketGroupList, imbued = imbued,
    defaultGemLevel = self.defaultGemLevel, defaultGemQuality = N(self.defaultGemQuality), characterLevel = N(self.build.characterLevel) }
end
loadBuildFromXML(BUILD_XML, BUILD_NAME)
if __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end
return SNAP
`

// --- helper cases (TS side) -------------------------------------------------------------------------------------------------------
const skillsJson: Rec = JSON.parse(readFileSync(resolve(ROOT, 'data/skills.json'), 'utf8'))
const gemsJson: Rec = JSON.parse(readFileSync(resolve(ROOT, 'data/gems.json'), 'utf8'))
let seed = 0x6b5a_2025
const rnd = (): number => { seed = (seed + 0x6d2b79f5) >>> 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296 }
const pick = <T>(a: T[]): T => a[Math.floor(rnd() * a.length)]
const WEAPONS = [undefined, 'Bow', 'Claw', 'Dagger', 'One Handed Sword', 'Two Handed Mace', 'Wand', 'Staff', 'Shield']
const goldenXml = GOLDEN_BUILDS.map((b: Rec) => ({ name: b.name as string, xml: readFileSync(b.xmlPath(roots)) }))
// --- (3) first: the goldens (RUNS processes each, R13), their resolved socket groups give the golden active x support pairs --
const luts = await prepareLuts()
async function pool<T>(jobs: (() => Promise<T>)[]): Promise<T[]> {
  const out: T[] = new Array(jobs.length)
  let next = 0
  await Promise.all(Array.from({ length: PARALLEL }, async () => { while (next < jobs.length) { const i = next++; out[i] = await jobs[i]() } }))
  return out
}
// R13: where LuaJIT itself differs between processes, a value becomes {"$oneOf": [sorted distinct alternatives]}; the recursion stops at
// the units whose fields vary together (a gem instance, a statMap entry, a mod list) so that no impossible mix is recorded.
const STOP_AT = [/^builds\.[^.]+\.sets\.\d+\.groups\.\d+\.gems\.\d+$/, /^skills\.[^.]+\.statMap\.[^.]+$/, /^skills\.[^.]+\.(baseMods|qualityMods|levelMods)$/]
const oneOfPaths: string[] = []
function merge(vals: unknown[], path: string): unknown {
  const ser = vals.map((v) => JSON.stringify(v === undefined ? null : v))
  if (ser.every((x) => x === ser[0])) return vals[0]
  if (!STOP_AT.some((r) => r.test(path)) && vals.every((v) => v !== null && typeof v === 'object')) {
    if (vals.every(Array.isArray) && vals.every((v) => (v as unknown[]).length === (vals[0] as unknown[]).length)) {
      return (vals[0] as unknown[]).map((_, i) => merge(vals.map((v) => (v as unknown[])[i]), `${path}.${i}`))
    }
    if (!vals.some(Array.isArray)) {
      const keys = [...new Set(vals.flatMap((v) => Object.keys(v as Rec)))].sort()
      return Object.fromEntries(keys.map((k) => [k, merge(vals.map((v) => (v as Rec)[k]), path ? `${path}.${k}` : k)]))
    }
  }
  oneOfPaths.push(path)
  return { $oneOf: [...new Set(ser)].sort().map((x) => JSON.parse(x)) }
}
const groupJobs = goldenXml.flatMap(({ name, xml }) => Array.from({ length: RUNS }, () => () =>
  ljEval(LUA_HELPERS + `local BUILD_XML, BUILD_NAME = ${luaBytes(xml)}, ${luaStr(name)}\n` + GROUPS_LUA, luts)))
let t0 = performance.now()
const groupOut = await pool(groupJobs)
console.log(`goldens: ${goldenXml.length} x ${RUNS} processes, ${((performance.now() - t0) / 1000).toFixed(0)} s`)
const groupRuns: Rec = {}
goldenXml.forEach(({ name }, b) => { groupRuns[name] = groupOut.slice(b * RUNS, (b + 1) * RUNS) })
const groups = merge(Array.from({ length: RUNS }, (_, i) => ({ builds: Object.fromEntries(goldenXml.map(({ name }) => [name, groupRuns[name][i]])) })), '') as Rec
const firstAlt = (v: Rec): Rec => (v && v.$oneOf ? v.$oneOf[0] : v)
const goldenPairs: [string, string][] = []
const goldenSpecs = new Set<string>()
for (const { xml } of goldenXml) for (const m of xml.toString('utf8').matchAll(/<Gem [^>]*?nameSpec="([^"]*)"/g)) goldenSpecs.add(m[1])
for (const name of Object.keys(groups.builds)) {
  for (const set of groups.builds[name].sets) {
    for (const g of set.groups) {
      const ids = (g.gems as Rec[]).map(firstAlt).map((x) => (x.gemData ? gemsJson.gems[x.gemData].grantedEffectId : x.grantedEffect)).filter((id) => id && skillsJson[id])
      const act = ids.filter((id: string) => !skillsJson[id].support)
      const sup = ids.filter((id: string) => skillsJson[id].support)
      for (const a of act) for (const sp of sup) goldenPairs.push([a, sp])
    }
  }
}
const actives = Object.keys(skillsJson).filter((id) => !skillsJson[id].support && skillsJson[id].skillTypes)
const supports = Object.keys(skillsJson).filter((id) => skillsJson[id].support)
const supportCases: Rec[] = []
const variant = (a: string, s: string): Rec => ({
  a, s, imbued: rnd() < 0.3, granted: rnd() < 0.2, gem: rnd() < 0.8, srcFromItem: rnd() < 0.1, notPlayer: rnd() < 0.1,
  w1: pick(WEAPONS), w2: pick(WEAPONS), all1h: rnd() < 0.1,
})
for (const [a, s] of goldenPairs) supportCases.push(variant(a, s))
while (supportCases.length < 2500) supportCases.push(variant(pick(actives), pick(supports)))
const reqSet = new Map<string, [number, boolean, number]>()
for (const g of Object.values(gemsJson.gems) as Rec[]) {
  const ge = skillsJson[g.grantedEffectId]
  for (const lv of ge.levels) {
    if (lv.levelRequirement === undefined) continue
    for (const m of [g.reqStr, g.reqDex, g.reqInt]) reqSet.set(`${lv.levelRequirement}|${!!ge.support}|${m}`, [lv.levelRequirement, !!ge.support, m])
  }
}
const reqCases = [...reqSet.values()]
const statsCases: Rec[] = []
for (const id of Object.keys(skillsJson)) {
  const lv = skillsJson[id].levels.map((l: Rec) => l.level)
  statsCases.push({ id, level: lv[0], quality: 0 })
  statsCases.push({ id, level: lv[Math.floor(lv.length / 2)], quality: 20, actorLevel: 50 })
  statsCases.push({ id, level: lv[lv.length - 1], quality: 23 })
}
const findCases = [...goldenSpecs].sort()
for (const g of Object.values(gemsJson.gems) as Rec[]) findCases.push(g.name)
findCases.push('CtF', 'ctof', 'CldFr', 'fireball', 'Added Fire', 'Faster', 'Melee Phys', 'vaal', 'Arc', 'Ice', 'Raise Zombie', 'Nonexistent Gem', 'Lightning', 'Summon')
const helperCases = { support: supportCases, req: reqCases, stats: statsCases, find: findCases }

// --- run ----------------------------------------------------------------------------------------------------------------------------
const dataChunk = LUA_HELPERS + DATA_LUA
const casesJson = JSON.stringify(helperCases)
t0 = performance.now()
const runs: Rec[] = await pool(Array.from({ length: RUNS }, () => () => ljEval(dataChunk, luts, { 'cases.json': casesJson })))
console.log(`data: ${RUNS} processes, ${((performance.now() - t0) / 1000).toFixed(0)} s`)
mkdirSync(SCRATCH, { recursive: true })
writeFileSync(resolve(SCRATCH, 'gemdata-runs.json'), JSON.stringify({ runs, groupRuns }))
// orders are sorted (R13: a string-keyed `pairs` order is per process); the FindSkillGem "Ambiguous" text names two of `matches` in
// `pairs` order -- every observed text is checked against the order-free `matches` and replaced by the marker "Ambiguous".
for (const r of runs) {
  r.orders.gems.sort(); r.orders.skills.sort()
  for (const k of Object.keys(r.gemsByGameId)) r.gemsByGameId[k].sort()
  for (const c of Object.values(r.collisions) as Rec[]) c.ids.sort()
  r.helpers.find.forEach((f: Rec, i: number) => {
    if (typeof f.err !== 'string' || !f.err.startsWith('Ambiguous')) return
    const m = /^Ambiguous gem name '(.*)': matches '(.*)', '(.*)'$/.exec(f.err)
    const spec = helperCases.find[i]
    if (!m || m[1] !== spec || m[2] === m[3] || !f.matches.includes(m[2]) || !f.matches.includes(m[3])) throw new Error(`find ${spec}: unexpected text ${f.err}`)
    f.err = 'Ambiguous'
  })
}
const d = merge(runs.map((r) => ({ ...r, jit: 0 })), '') as Rec
d.jit = runs[0].jit
console.log(`$oneOf (R13 lj-strhash) at ${oneOfPaths.length} paths:`, oneOfPaths)

// --- fixtures -----------------------------------------------------------------------------------------------------------------------
const meta = { generator: 'tools/lua-corpus/export-gemdata.ts', pobSha: git(roots.pobRoot), luajit: d.jit, processes: RUNS, oneOf: oneOfPaths }
const full: Rec = {}
const shapes: Rec = {}
const skills: Rec = {}
for (const id of Object.keys(d.skills).sort()) {
  const s = d.skills[id]
  if (typeof s.levels !== 'string') throw new Error(`${id}: levels differ between processes`)
  full[id] = s.levels
  const { nLevels, nextLevel, ...rest } = s
  shapes[id] = { len: nLevels, next: nextLevel }
  skills[id] = { ...rest, levels: sha(s.levels) }
}
const statsFull = d.helpers.stats as string[]
if (statsFull.some((x) => typeof x !== 'string')) throw new Error('buildSkillInstanceStats differs between processes')
const helpers = {
  meta,
  support: supportCases.map((c, i) => ({ ...c, out: d.helpers.support[i] })),
  req: reqCases.map((c, i) => [...c, d.helpers.req[i]]),
  stats: statsCases.map((c, i) => ({ ...c, sha: sha(statsFull[i]), n: statsFull[i].startsWith('ERR') ? -1 : statsFull[i].split(';').length })),
  gemLevel: d.helpers.gemLevel,
  malformed: d.malformed,
  find: findCases.map((spec, i) => ({ spec, ...d.helpers.find[i] })),
}
const gemsFix = {
  meta, orders: d.orders, gems: d.gems, gemForSkill: d.gemForSkill, nGemForSkill: d.nGemForSkill, collisions: d.collisions, gemForBaseName: d.gemForBaseName,
  gemsByGameId: d.gemsByGameId, gemGrantedEffectIdForVaalGemId: d.gemGrantedEffectIdForVaalGemId, gemVaalGemIdForBaseGemId: d.gemVaalGemIdForBaseGemId,
}
mkdirSync(FIX, { recursive: true })
mkdirSync(SCRATCH, { recursive: true })
writeFileSync(resolve(SCRATCH, 'gemdata-full.json'), JSON.stringify({ levels: full, stats: statsCases.map((c, i) => ({ ...c, out: statsFull[i] })) }))
// skills split in two halves by id to stay under MAX_BYTES per file
const ids = Object.keys(skills)
const half = Math.ceil(ids.length / 2)
const files: [string, unknown][] = [
  ['gemdata-gems.json', gemsFix],
  ['gemdata-skills-1.json', { meta, skills: Object.fromEntries(ids.slice(0, half).map((k) => [k, skills[k]])) }],
  ['gemdata-skills-2.json', { meta, skills: Object.fromEntries(ids.slice(half).map((k) => [k, skills[k]])) }],
  ['gemdata-helpers.json', helpers],
  ['gemdata-groups.json', { meta, builds: groups.builds }],
]
// #levels / next(levels) (integer keys -- deterministic, but the converter did not keep the table shape): a data patch, taken here
const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8'))
for (const [id, sh] of Object.entries(shapes) as [string, Rec][]) if (typeof sh.len !== 'number' || typeof sh.next !== 'number') throw new Error(`${id}: levels shape differs between processes`)
const shapePath = resolve(ROOT, 'data-patches/skill-levels-shape.json')
writeFileSync(shapePath, JSON.stringify({
  format: 1, producer: { tool: 'tools/lua-corpus/export-gemdata.ts', luajit: d.jit, pobSha: git(roots.pobRoot), processes: RUNS },
  manifestSha256: lock.manifestSha256, shapes,
}) + '\n')
console.log(`skill-levels-shape.json sha256 ${createHash('sha256').update(readFileSync(shapePath)).digest('hex')}`)
for (const [f, j] of files) {
  const p = resolve(FIX, f)
  writeFileSync(p, JSON.stringify(j) + '\n')
  const n = statSync(p).size
  console.log(`${f}: ${n} bytes sha256 ${createHash('sha256').update(readFileSync(p)).digest('hex')}${n > MAX_BYTES ? '  <-- OVER LIMIT' : ''}`)
}
console.log(`gems ${Object.keys(d.gems).length}, effects ${ids.length}, gemForSkill ${d.nGemForSkill}, collisions ${Object.keys(d.collisions).length}, ` +
  `support cases ${supportCases.length} (golden pairs ${goldenPairs.length}), req ${reqCases.length}, stats ${statsCases.length}, find ${findCases.length}`)
