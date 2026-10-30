// R3.1: fixtures for the R3 gate -- the four pob-ts probes (tree / items+applyRange / config / fresh initEnv) dumped for the 6 golden
// builds from (a) REAL LuaJIT (docker, PoB boot + loadBuildFromXML in the chunk) and (b) pob-ts, plus a self-check that replays the
// classification of the pob-ts oracle tests on LuaJIT <-> pob-ts, plus the perf "XML -> env". Truth = LuaJIT.
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-env.ts          -> crates/pob-calc/tests/fixtures/<build>.{luajit,pobts}.json (+ self-check report on stdout)
//   node tools/lua-corpus/export-env.ts perf     -> tools/.scratch/perf-env.json
// Lua probes are the pob-ts test strings VERBATIM (file:line next to each), wrapped in a function; TS halves are the tests' TS code.
// Only normalisation: `function: 0x<heap address>` (a JewelFunc mod) -> `function: <addr>` on BOTH sides (initEnv-oracle.test.ts:178).
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const T0 = performance.now()
const { activeSpec, loadBuildXml } = await import(`${P}/src/build/index.ts`)
const { applyRange } = await import(`${P}/src/items/applyRange.ts`)
const { Item } = await import(`${P}/src/items/item.ts`)
const { buildConfigModLists, configStateFromBuild } = await import(`${P}/src/config/index.ts`)
const { initEnv } = await import(`${P}/src/calc/initEnv.ts`)
const { formatG } = await import(`${P}/src/lua/index.ts`)
const { formatSourceMod } = await import(`${P}/src/mods/format.ts`)
const { parseModStats, resetParseModStats } = await import(`${P}/src/mods/parseMod.ts`)
const { PassiveSpec, loadSpec } = await import(`${P}/src/tree/spec.ts`)
const { loadPassiveTree } = await import(`${P}/src/tree/tree.ts`)
const { modCacheEntry } = await import(`${P}/src/data/modCache.ts`)
const { APPLY_RANGE_CASES } = await import(`${P}/test/items/cases.ts`)
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const IMPORT_MS = performance.now() - T0
type Rec = Record<string, any> // eslint-disable-line

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const POB_TS = 'C:/pob-work/pob-ts'
const MAX_BYTES = 1_450_000
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const roots = resolveRoots()
const tree = loadPassiveTree('3_13')
const { LUAJIT_IMAGE } = await import(`${P}/tools/oracle/luajit.ts`)
// Timeless-jewel LUTs: HeadlessWrapper's Inflate is a stub, so (like luajit-dump17.lua:35-59, pob-ts luajit.ts) the .bin comes from a mounted
// dir via an io.open/NewFileSearch shim. pob-ts' luajitEval has no such mount, hence this local docker twin of it. LUT dir stays in tools/.scratch.
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
/** luajitEval + the LUT mount; one `docker run` per call. */
function ljEval(code: string, lutsDir: string): Rec {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r31-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${lutsDir}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = execFileSync('docker', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 }) } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}

// --- Lua probes (VERBATIM from pob-ts tests) --------------------------------------------------------------------
// test/tree/spec.test.ts:153-198
const TREE_LUA = `
local function upval(f, want)
  local i = 1
  while true do
    local n, v = debug.getupvalue(f, i)
    if not n then return nil end
    if n == want then return v end
    i = i + 1
  end
end
local RAWF = upval(string.format, "s_format") or string.format
local function fmt(list) local t = {} for i, m in ipairs(list) do t[i] = modLib.formatSourceMod(m) end return t end
local alloc = {}
for id, n in pairs(build.spec.allocNodes) do
  alloc[tostring(id)] = { type = n.type, dn = n.dn, mods = fmt(n.modList), ks = n.keystoneMod and modLib.formatSourceMod(n.keystoneMod) or nil }
end
local ks = {}
for dn, n in pairs(build.spec.tree.keystoneMap) do ks[dn] = fmt(n.modList) end
local notables = {}
for dn, n in pairs(build.spec.tree.notableMap) do notables[dn] = n.id end
local nodeTypes, coords, nTree = {}, {}, 0
for id, n in pairs(build.spec.tree.nodes) do
  nTree = nTree + 1
  nodeTypes[n.type or "?"] = (nodeTypes[n.type or "?"] or 0) + 1
  if n.x then coords[tostring(id)] = RAWF("%.17g", n.x) .. "|" .. RAWF("%.17g", n.y) end
end
local nSpec = 0
for _ in pairs(build.spec.nodes) do nSpec = nSpec + 1 end
local radius = nil
local socket = build.spec.nodes[61834]
if socket and socket.nodesInRadius then
  radius = {}
  for radiusIndex, list in ipairs(socket.nodesInRadius) do
    local ids = {}
    for nodeId in pairs(list) do ids[#ids + 1] = nodeId end
    table.sort(ids)
    radius[radiusIndex] = ids
  end
end
return {
  alloc = alloc, keystones = ks, notables = notables, nodeTypes = nodeTypes, coords = coords, nTree = nTree, nSpec = nSpec, radius = radius,
  counts = { build.spec.allocatedNotableCount, build.spec.allocatedKeystoneCount, build.spec.allocatedMasteryCount, build.spec.allocatedMasteryTypeCount },
  className = build.spec.curClassName, ascendName = build.spec.curAscendClassName,
}
`
const LINE_FLAGS = ['crafted', 'crucible', 'custom', 'disabled', 'eater', 'enchant', 'exarch', 'fractured', 'implicit', 'scourge', 'synthesis', 'mutated', 'unscalable', 'prefix', 'suffix', 'unveiled', 'vestigial'] as const
const LINE_LISTS = ['enchantModLines', 'scourgeModLines', 'classRequirementModLines', 'implicitModLines', 'explicitModLines', 'crucibleModLines', 'buffModLines'] as const
const INFLUENCES = ['shaper', 'elder', 'adjudicator', 'basilisk', 'crusader', 'eyrie', 'cleansing', 'tangle'] as const
// test/items/oracle.test.ts:114-119
const FIND_SUBSTRING_PROBES: [string, string][] = [
  ['life', 'body armour'], ['life', 'amulet'], ['life', 'belt'], ['life', 'ring'], ['life', 'helmet'],
  ['explicit life', 'body armour'], ['evasion', 'ring'], ['defence', 'ring'],
  ['critical strike', 'ring'], ['attack', 'weapon 1'],
  ['%a+ damage', 'ring'], ['^adds', 'ring'], ['damage$', 'ring'], ['-1', 'ring'], ['%d+%% increased', 'ring'],
]
// test/items/oracle.test.ts:121-232 (ITEM_DUMP)
const ITEM_LUA = `
local function upval(f, want)
  local i = 1
  while true do
    local n, v = debug.getupvalue(f, i)
    if not n then return nil end
    if n == want then return v end
    i = i + 1
  end
end
local RAWF = upval(string.format, "s_format") or string.format
local function N(v)
  if v == nil then return nil end
  if type(v) == "number" then return RAWF("%.17g", v) end
  if type(v) == "boolean" then return v end
  return tostring(v)
end
local function fmt(list)
  if not list then return nil end
  local t = {}
  for i, m in ipairs(list) do t[i] = modLib.formatSourceMod(m) .. "|" .. tostring(m.sourceSlot) end
  return t
end
local function tbl(t)
  if not t then return nil end
  local o = {}
  for k, v in pairs(t) do
    if type(v) == "table" then
      local inner = {}
      for k2, v2 in pairs(v) do inner[tostring(k2)] = N(v2) end
      o[tostring(k)] = inner
    else
      o[tostring(k)] = N(v)
    end
  end
  return o
end
local FLAGS = { ${LINE_FLAGS.map((f) => `"${f}"`).join(',')} }
local function lines(list)
  if not list then return nil end
  local t = {}
  for i, ml in ipairs(list) do
    local e = { line = ml.line, mods = fmt(ml.modList), extra = ml.extra,
      range = N(ml.range), valueScalar = N(ml.valueScalar), corruptedRange = N(ml.corruptedRange),
      modTags = ml.modTags and table.concat(ml.modTags, ",") or nil,
      modGroup = ml.modGroup, showSlider = ml.showSlider }
    local fl = {}
    for _, f in ipairs(FLAGS) do if ml[f] then fl[#fl+1] = f end end
    e.flags = table.concat(fl, ",")
    local vl = {}
    if ml.variantList then for k in pairs(ml.variantList) do vl[#vl+1] = k end table.sort(vl) end
    e.variantList = #vl > 0 and table.concat(vl, ",") or nil
    t[i] = e
  end
  return t
end
local out = {}
for id, item in pairs(build.itemsTab.items) do
  local socks = {}
  for i, s in ipairs(item.sockets) do socks[i] = s.color .. tostring(s.group) end
  local gs = {}
  for i, s in ipairs(item.grantedSkills or {}) do
    gs[i] = tostring(s.skillId) .. "|" .. tostring(s.level) .. "|" .. tostring(s.triggered) .. "|" .. tostring(s.source) .. "|" .. tostring(s.noSupports) .. "|" .. tostring(s.triggerChance)
  end
  local slot = nil
  if item.slotModList then
    slot = {}
    for i, l in pairs(item.slotModList) do slot[tostring(i)] = fmt(l) end
  end
  local infl = {}
  for _, k in ipairs({ ${INFLUENCES.map((i) => `"${i}"`).join(',')} }) do
    if item[k] then infl[#infl+1] = k end
  end
  local fms = {}
  for i, probe in ipairs({ ${FIND_SUBSTRING_PROBES.map(([s, slot]) => `{${JSON.stringify(s)},${JSON.stringify(slot)}}`).join(',')} }) do
    fms[i] = item:FindModifierSubstring(probe[1], probe[2]) and "1" or "0"
  end
  out[tostring(id)] = {
    findModifierSubstring = table.concat(fms, ""),
    name = item.name, title = item.title, baseName = item.baseName, type = item.type, rarity = item.rarity,
    namePrefix = item.namePrefix, nameSuffix = item.nameSuffix,
    quality = N(item.quality), itemLevel = N(item.itemLevel), corrupted = item.corrupted and true or false,
    split = item.split and true or false, mirrored = item.mirrored and true or false,
    fractured = item.fractured and true or false, synthesised = item.synthesised and true or false,
    crafted = item.crafted and true or false, advancedCopy = item.advancedCopy and true or false,
    isUnique = item.isUnique and true or false, vestigial = item.vestigial and true or false,
    foulborn = item.foulborn and true or false, hasModTags = item.hasModTags and true or false,
    veiled = item.veiled and true or false,
    influences = table.concat(infl, ","),
    uniqueID = item.uniqueID, league = item.league, classRestriction = item.classRestriction,
    limit = N(item.limit), jewelRadiusIndex = N(item.jewelRadiusIndex), jewelRadiusLabel = item.jewelRadiusLabel,
    catalyst = N(item.catalyst), catalystQuality = N(item.catalystQuality), talismanTier = N(item.talismanTier),
    abyssalSocketCount = N(item.abyssalSocketCount), selectableSocketCount = N(item.selectableSocketCount),
    defaultSocketColor = item.defaultSocketColor, socketedJewelEffectModifier = N(item.socketedJewelEffectModifier),
    craftedQuality = N(item.craftedQuality), corruptImplicitCount = N(item.corruptImplicitCount),
    modSource = item.modSource, clusterJewelSkill = item.clusterJewelSkill, clusterJewelNodeCount = N(item.clusterJewelNodeCount),
    variant = N(item.variant), variantAlt = N(item.variantAlt), selectedVersion = N(item.selectedVersion),
    usesVariantGroups = item.usesVariantGroups and true or false,
    requirements = tbl(item.requirements),
    sockets = table.concat(socks, " "),
    armourData = tbl(item.armourData), weaponData = tbl(item.weaponData), flaskData = tbl(item.flaskData),
    tinctureData = tbl(item.tinctureData), jewelData = tbl(item.jewelData),
    grantedSkills = table.concat(gs, "; "),
    modList = fmt(item.modList), slotModList = slot, baseModList = fmt(item.baseModList),
    buffModList = fmt(item.buffModList),
    ${LINE_LISTS.map((l) => `${l} = lines(item.${l})`).join(', ')},
    rangeLineCount = N(#(item.rangeLineList or {})),
  }
end
return out
`
// test/items/oracle.test.ts:63-76 (applyRangeCode)
interface RangeCase { line: string; range: number; valueScalar?: number; baseValueScalar?: number }
const luaStr = (s: string): string => JSON.stringify(s)
const luaVal = (v: number | undefined): string => (v === undefined ? 'nil' : String(v))
const applyRangeCode = (cases: RangeCase[]): string =>
  `local cases = {${cases.map((c) => `{${luaStr(c.line)},${luaVal(c.range)},${luaVal(c.valueScalar)},${luaVal(c.baseValueScalar)}}`).join(',\n')}}\n` +
  'local out = {}\nfor i, c in ipairs(cases) do out[i] = itemLib.applyRange(c[1], c[2], c[3], c[4]) end\nreturn out\n'
// test/config/oracle.test.ts:31-50
const CONFIG_LUA = `
local function fmt(list)
  local t = { }
  for i, m in ipairs(list) do t[i] = modLib.formatSourceMod(m) end
  return t
end
local ct = build.configTab
return {
  mods = fmt(ct.modList),
  enemyMods = fmt(ct.enemyModList),
  enemyLevel = ct.enemyLevel,
  input = ct.input,
  placeholder = ct.placeholder,
  characterLevel = build.characterLevel,
  buildPantheonMajorGod = build.pantheonMajorGod,
  inputPantheonMajorGod = ct.input.pantheonMajorGod,
  inputPantheonMinorGod = ct.input.pantheonMinorGod,
  inputBandit = ct.input.bandit,
}
`
// test/calc/initEnv-oracle.test.ts:88-116 -- FRESH calcs.initEnv(build, "MAIN"), never mainEnv (it mutates: runs last)
const ENV_LUA = `
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local function fmt(list) local t = {} for i, m in ipairs(list) do t[i] = modLib.formatSourceMod(m) end return t end
local calcs = build.calcsTab.calcs
local env = calcs.initEnv(build, "MAIN")
local function num(v) if type(v) == "number" then return RAWF("%.17g", v) else return v end end
local function db(d)
  local mods = {} for name, bucket in pairs(d.mods) do mods[name] = fmt(bucket) end
  local srcs = {} for name, bucket in pairs(d.mods) do local t = {} for i, m in ipairs(bucket) do t[i] = m.source or "" end srcs[name] = t end
  local mult = {} for k, v in pairs(d.multipliers) do mult[k] = num(v) end
  local cond = {} for k, v in pairs(d.conditions) do cond[k] = v end
  return { mods = mods, srcs = srcs, conditions = cond, multipliers = mult }
end
local function wd(w) if not w then return nil end local t = {} for k, v in pairs(w) do t[k] = num(v) end return t end
local slots = {} for i, s in ipairs(build.itemsTab.orderedSlots) do slots[i] = s.slotName end
local itemSlots = {} for k, it in pairs(env.player.itemList) do itemSlots[k] = it.name end
local alloc = {} for id in pairs(env.allocNodes) do alloc[#alloc+1] = id end table.sort(alloc)
local ks = {} for k in pairs(env.keystonesAdded) do ks[#ks+1] = k end table.sort(ks)
local gp = {} for id in pairs(env.grantedPassives) do gp[#gp+1] = id end table.sort(gp)
return { modDB = db(env.modDB), enemyDB = db(env.enemyDB),
  enemyLevel = env.enemyLevel, itemSlots = itemSlots, alloc = alloc,
  weapon1 = wd(env.player.weaponData1), weapon2 = wd(env.player.weaponData2),
  mainSocketGroup = env.mainSocketGroup, orderedSlots = slots, keystonesAdded = ks,
  grantedPassives = gp, grantedSkills = #env.grantedSkills,
  reqItems = #env.requirementsTableItems, reqGems = #env.requirementsTableGems,
  initialNodeMods = #env.initialNodeModDB, classId = env.classId, level = build.characterLevel,
  notableCount = env.spec.allocatedNotableCount, keystoneCount = env.spec.allocatedKeystoneCount }
`
/** XML bytes -> a Lua string literal (decimal escapes for everything but printable ASCII; long brackets would normalise CRLF). */
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
function luajitChunk(xml: Buffer, name: string, cases: RangeCase[]): string {
  const fn = (n: string, body: string): string => `local function ${n}()\n${body}\nend\n`
  return `loadBuildFromXML(${luaBytes(xml)}, ${luaStr(name)})\n` +
    'if __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n' +
    fn('pTree', TREE_LUA) + fn('pItems', ITEM_LUA) + fn('pRange', applyRangeCode(cases)) + fn('pConfig', CONFIG_LUA) + fn('pEnv', ENV_LUA) +
    'local tree, items, applyRange, config = pTree(), pItems(), pRange(), pConfig()\n' +
    'return { jit = jit.version, tree = tree, items = items, applyRange = applyRange, config = config, env = pEnv() }\n'
}

// --- the pob-ts halves (the TS code of the same tests) -----------------------------------------------------------
const rawf = (n: number): string => formatG(n, 17)
const N = (v: unknown): unknown => (v === undefined || v === null ? undefined : typeof v === 'number' ? rawf(v) : typeof v === 'boolean' ? v : String(v))
const fmtTs = (list: readonly Rec[] | undefined): string[] | undefined =>
  list === undefined ? undefined : list.map((m) => formatSourceMod(m) + '|' + (m.sourceSlot ?? 'nil'))
function tblTs(t: Rec | undefined): Rec | undefined {
  if (t === undefined) return undefined
  const out: Rec = {}
  for (const [k, v] of Object.entries(t)) {
    if (v === undefined) continue
    if (Array.isArray(v)) {
      const inner: Rec = {}
      v.forEach((x, i) => { if (x !== undefined) inner[String(i + 1)] = N(x) })
      out[k] = inner
    } else if (v !== null && typeof v === 'object') {
      const inner: Rec = {}
      for (const [k2, v2] of Object.entries(v as Rec)) if (v2 !== undefined) inner[k2] = N(v2)
      out[k] = inner
    } else out[k] = N(v)
  }
  return out
}
function linesTs(list: Rec[] | undefined): unknown[] | undefined {
  if (list === undefined) return undefined
  return list.map((ml) => {
    const variants = ml.variantList === undefined ? [] : Object.keys(ml.variantList).map(Number).sort((a, b) => a - b)
    return {
      line: ml.line, mods: fmtTs(ml.modList), extra: ml.extra, range: N(ml.range), valueScalar: N(ml.valueScalar), corruptedRange: N(ml.corruptedRange),
      modTags: ml.modTags.join(','), modGroup: ml.modGroup, showSlider: ml.showSlider,
      flags: LINE_FLAGS.filter((f) => ml[f] === true).join(','), variantList: variants.length > 0 ? variants.join(',') : undefined,
    }
  })
}
const itemOpts = (im: Rec): Rec => ({
  id: im.id, variant: im.variant, variantAlt: im.variantAlt, variantAlt2: im.variantAlt2, variantAlt3: im.variantAlt3,
  variantAlt4: im.variantAlt4, variantAlt5: im.variantAlt5, modRanges: im.modRanges,
})
/** test/items/oracle.test.ts:303-362; `missesByItem` records each item's ModCache misses. */
function tsItems(model: Rec, missesByItem: Record<string, string[]>): Rec {
  const out: Rec = {}
  for (const im of model.items) {
    resetParseModStats()
    const item = Item.parse(im.raw, itemOpts(im))
    missesByItem[String(im.id)] = parseModStats().missLines
    if (item.base === undefined) continue
    let slot: Rec | undefined
    if (item.slotModList !== undefined) {
      slot = {}
      item.slotModList.forEach((l: Rec[] | undefined, i: number) => { if (l !== undefined) slot![String(i)] = fmtTs(l) })
    }
    out[String(im.id)] = {
      name: item.name, title: item.title, baseName: item.baseName, type: item.type, rarity: item.rarity,
      namePrefix: item.namePrefix, nameSuffix: item.nameSuffix,
      quality: N(item.quality), itemLevel: N(item.itemLevel), corrupted: item.corrupted === true,
      split: item.split === true, mirrored: item.mirrored === true, fractured: item.fractured === true, synthesised: item.synthesised === true,
      crafted: item.crafted === true, advancedCopy: item.advancedCopy, isUnique: item.isUnique, vestigial: item.vestigial,
      foulborn: item.foulborn, hasModTags: item.hasModTags === true, veiled: item.veiled === true,
      influences: INFLUENCES.filter((k) => item.influence[k] === true).join(','),
      uniqueID: item.uniqueID, league: item.league, classRestriction: item.classRestriction,
      limit: N(item.limit), jewelRadiusIndex: N(item.jewelRadiusIndex), jewelRadiusLabel: item.jewelRadiusLabel,
      catalyst: N(item.catalyst), catalystQuality: N(item.catalystQuality), talismanTier: N(item.talismanTier),
      abyssalSocketCount: N(item.abyssalSocketCount), selectableSocketCount: N(item.selectableSocketCount),
      defaultSocketColor: item.defaultSocketColor, socketedJewelEffectModifier: N(item.socketedJewelEffectModifier),
      craftedQuality: N(item.craftedQuality), corruptImplicitCount: N(item.corruptImplicitCount),
      modSource: item.modSource, clusterJewelSkill: item.clusterJewelSkill, clusterJewelNodeCount: N(item.clusterJewelNodeCount),
      variant: N(item.variant), variantAlt: N(item.variantAlt), selectedVersion: N(item.selectedVersion), usesVariantGroups: item.usesVariantGroups,
      requirements: tblTs(item.requirements), sockets: item.sockets.map((s: Rec) => s.color + String(s.group)).join(' '),
      armourData: tblTs(item.armourData), weaponData: tblTs(item.weaponData), flaskData: tblTs(item.flaskData),
      tinctureData: tblTs(item.tinctureData), jewelData: tblTs(item.jewelData),
      grantedSkills: item.grantedSkills
        .map((s: Rec) => [s.skillId, s.level, s.triggered, s.source, s.noSupports, s.triggerChance].map((x) => (x === undefined ? 'nil' : String(x))).join('|'))
        .join('; '),
      modList: fmtTs(item.modList), slotModList: slot, baseModList: fmtTs(item.baseModList?.mods), buffModList: fmtTs(item.buffModList),
      ...Object.fromEntries(LINE_LISTS.map((l) => [l, linesTs(item[l])])),
      rangeLineCount: N(item.rangeLineList.length),
      findModifierSubstring: FIND_SUBSTRING_PROBES.map(([s, sl]) => (item.FindModifierSubstring(s, sl) ? '1' : '0')).join(''),
    }
  }
  return out
}
/** test/items/oracle.test.ts:49-61 -- every `{range:X}` mod line of a build. */
function rangeLinesOf(model: Rec): RangeCase[] {
  const out: RangeCase[] = []
  for (const item of model.items) {
    for (const raw of item.raw.split('\n')) {
      const line = raw.trim()
      const r = /\{range:([^}]+)\}/.exec(line)
      if (r !== null) out.push({ line: line.replace(/\{[A-Za-z]*:?[^}]*\}/g, '').replace(/ \([a-z]+\)/g, ''), range: Number(r[1]) })
    }
  }
  return out
}
function specFor(build: Rec): Rec {
  const model = activeSpec(build)
  const spec = new PassiveSpec(tree)
  spec.allocOrder = 'idAsc'
  spec.importFromNodeList(model.classId, model.ascendClassId, model.secondaryAscendClassId, model.nodes, model.masteryEffects)
  return spec
}
function tsTree(model: Rec): { dump: Rec; spec: Rec } {
  const spec = specFor(model)
  const alloc: Rec = {}
  for (const [id, n] of spec.allocNodes) {
    alloc[String(id)] = { type: n.type, dn: n.dn, mods: n.modList.mods.map(formatSourceMod), ks: n.keystoneMod === undefined ? undefined : formatSourceMod(n.keystoneMod) }
  }
  const keystones: Rec = {}
  for (const [dn, n] of tree.keystoneMap) keystones[dn] = n.modList.mods.map(formatSourceMod)
  const notables: Rec = {}
  for (const [dn, n] of tree.notableMap) notables[dn] = n.id
  const nodeTypes: Rec = {}
  const coords: Rec = {}
  for (const n of tree.nodes.values()) {
    nodeTypes[n.type] = (nodeTypes[n.type] ?? 0) + 1
    if (n.x !== undefined) coords[String(n.id)] = rawf(n.x) + '|' + rawf(n.y)
  }
  const dump = {
    alloc, keystones, notables, nodeTypes, coords, nTree: tree.nodes.size, nSpec: spec.nodes.size, radius: undefined, // P5: no nodesInRadius
    counts: [spec.allocatedNotableCount, spec.allocatedKeystoneCount, spec.allocatedMasteryCount, spec.allocatedMasteryTypeCount],
    className: spec.curClassName, ascendName: spec.curAscendClassName,
  }
  return { dump, spec }
}
function tsConfig(model: Rec): Rec {
  const state = configStateFromBuild(model)
  const result = buildConfigModLists(state, { characterLevel: model.level })
  return {
    dump: {
      mods: result.modList.mods.map(formatSourceMod), enemyMods: result.enemyModList.mods.map(formatSourceMod), enemyLevel: state.enemyLevel,
      input: state.input, placeholder: state.placeholder, characterLevel: model.level, buildPantheonMajorGod: model.pantheonMajorGod,
      inputPantheonMajorGod: state.input['pantheonMajorGod'], inputPantheonMinorGod: state.input['pantheonMinorGod'], inputBandit: state.input['bandit'],
    },
    result,
  }
}
/** The TS side of the initEnv probe: parse items (recording misses), loadSpec, a fresh initEnv, dumped in the probe's shape. */
function tsEnvRun(model: Rec): { env: Rec; spec: Rec; items: Map<number, Rec>; missItems: Set<number> } {
  const items = new Map<number, Rec>()
  const missItems = new Set<number>()
  for (const im of model.items) {
    resetParseModStats()
    const item = Item.parse(im.raw, itemOpts(im))
    if (parseModStats().missLines.length > 0) missItems.add(im.id)
    if (item.base !== undefined) items.set(im.id, item)
  }
  const spec = loadSpec(activeSpec(model))
  return { env: initEnv(model, { spec, items }), spec, items, missItems }
}
function tsEnvDump(env: Rec, model: Rec): Rec {
  const db = (d: Rec): Rec => {
    const mods: Rec = {}
    const srcs: Rec = {}
    for (const [name, bucket] of d.mods as Map<string, Rec[]>) { mods[name] = bucket.map(formatSourceMod); srcs[name] = bucket.map((m) => m.source ?? '') }
    const multipliers: Rec = {}
    for (const [k, v] of Object.entries(d.multipliers)) multipliers[k] = typeof v === 'number' ? rawf(v) : v
    return { mods, srcs, conditions: { ...d.conditions }, multipliers }
  }
  const wd = (w: Rec | undefined): Rec | undefined => {
    if (w === undefined) return undefined
    const t: Rec = {}
    for (const [k, v] of Object.entries(w)) if (v !== undefined) t[k] = typeof v === 'number' ? rawf(v) : v
    return t
  }
  const itemSlots: Rec = {}
  for (const [k, it] of Object.entries(env.player.itemList as Rec)) itemSlots[k] = (it as Rec).name
  return {
    modDB: db(env.modDB), enemyDB: db(env.enemyDB), enemyLevel: env.enemyLevel, itemSlots, alloc: [...env.allocNodes.keys()].sort((a: number, b: number) => a - b),
    weapon1: wd(env.player.weaponData1), weapon2: wd(env.player.weaponData2), mainSocketGroup: env.mainSocketGroup,
    orderedSlots: env.orderedSlots.map((s: Rec) => s.slotName), keystonesAdded: Object.keys(env.keystonesAdded).sort(),
    grantedPassives: [...env.grantedPassives].sort((a: number, b: number) => a - b), grantedSkills: env.grantedSkills.length,
    reqItems: env.requirementsTableItems.length, reqGems: env.requirementsTableGems.length, initialNodeMods: env.initialNodeModDB.mods.length,
    classId: env.classId, level: model.level, notableCount: env.spec.allocatedNotableCount, keystoneCount: env.spec.allocatedKeystoneCount,
  }
}

// --- deterministic JSON ------------------------------------------------------------------------------------------
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
const normAddr = (s: string): string => s.replace(/(function|table): 0x[0-9a-f]+/g, '$1: <addr>') // heap addresses (JewelFunc mod, jewelData.funcList)
function normDeep(v: any): any { // eslint-disable-line
  if (typeof v === 'string') return normAddr(v)
  if (Array.isArray(v)) return v.map(normDeep)
  if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [normAddr(k), normDeep(x)]))
  return v
}
const PROBES = ['applyRange', 'config', 'env', 'items', 'tree'] as const
/** One file per build (all probes) or, when > 1.5 MB, one file per probe `<build>.<probe>.<kind>.json`. */
function writeFixture(slug: string, kind: string, head: Rec, probes: Rec): { file: string; bytes: number }[] {
  mkdirSync(FIX, { recursive: true })
  const text = (o: Rec): string => `{\n${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',\n')}\n}\n`
  const rel = (f: string): string => f.replace(`${ROOT}\\`, '').replace(/\\/g, '/')
  const whole = text({ ...head, ...probes })
  if (Buffer.byteLength(whole) <= MAX_BYTES) {
    const file = resolve(FIX, `${slug}.${kind}.json`)
    writeFileSync(file, whole, 'utf8')
    return [{ file: rel(file), bytes: statSync(file).size }]
  }
  return PROBES.map((p) => {
    const file = resolve(FIX, `${slug}.${p}.${kind}.json`)
    writeFileSync(file, text({ ...head, probe: p, [p]: probes[p] }), 'utf8')
    return { file: rel(file), bytes: statSync(file).size }
  })
}

// --- self-check: the classification of the four pob-ts oracle tests, replayed on LuaJIT <-> pob-ts -------------------
const uncachedLines = (id: number): string[] => (tree.nodes.get(id)?.sd ?? []).filter((l: string) => modCacheEntry(l) === undefined)
const numAsc = (a: number, b: number): number => a - b

/** test/tree/spec.test.ts:218-264 -- the build-independent half (once). */
function selfTreeGlobal(lj: Rec): Rec {
  const v: string[] = []
  const tsTypes: Rec = {}
  for (const n of tree.nodes.values()) tsTypes[n.type] = (tsTypes[n.type] ?? 0) + 1
  if (lj.nTree !== tree.nodes.size) v.push(`nTree ${tree.nodes.size} != ${lj.nTree}`)
  if (ser(tsTypes) !== ser(lj.nodeTypes)) v.push('nodeTypes differ')
  const notableDiff = Object.entries(lj.notables).filter(([dn, id]) => tree.notableMap.get(dn)?.id !== id).length
  let same = 0
  let ulp = 0
  let maxDelta = 0
  for (const [idStr, xy] of Object.entries(lj.coords as Rec)) {
    const node = tree.nodes.get(Number(idStr))
    const [ox, oy] = (xy as string).split('|').map(Number)
    const d = Math.max(Math.abs(node.x - ox!), Math.abs(node.y - oy!))
    if (d === 0) same++
    else { ulp++; maxDelta = Math.max(maxDelta, d) }
  }
  const names = Object.keys(lj.keystones)
  const missing = names.filter((dn) => !tree.keystoneMap.has(dn))
  let ksSame = 0
  let ksMiss = 0
  let ksOther = 0
  for (const dn of names) {
    const node = tree.keystoneMap.get(dn)
    if (node === undefined) continue
    if (ser(node.modList.mods.map(formatSourceMod)) === ser(lj.keystones[dn])) ksSame++
    else if (uncachedLines(node.id).length > 0) ksMiss++
    else ksOther++
  }
  if (notableDiff + ksOther > 0) v.push(`notableDiff ${notableDiff}, keystones unexplained ${ksOther}`)
  return { violations: v, coords: { bitIdentical: same, total: same + ulp, maxAbsDelta: maxDelta }, keystoneMap: { tsKeys: tree.keystoneMap.size, luajitKeys: names.length, luajitOnly: missing.length, same: ksSame, cacheMiss: ksMiss, unexplained: ksOther }, notableDiff }
}
/** test/tree/spec.test.ts:267-331. */
function selfTree(lj: Rec, ts: Rec, spec: Rec, name: string): Rec {
  const v: string[] = []
  const ljIds = Object.keys(lj.alloc).map(Number).sort(numAsc)
  const tsIds = [...spec.allocNodes.keys()].sort(numAsc) as number[]
  const tsSet = new Set(tsIds)
  const onlyTs = tsIds.filter((id) => !(id in lj.alloc))
  const onlyLj = ljIds.filter((id) => !tsSet.has(id))
  const cluster = onlyLj.filter((id) => id >= 65536)
  const expansion = onlyLj.filter((id) => id < 65536 && tree.nodes.has(id) && !spec.nodes.has(id))
  const unexplainedIds = onlyLj.filter((id) => !cluster.includes(id) && !expansion.includes(id))
  if (onlyTs.length + unexplainedIds.length > 0) v.push(`allocNodes: ts-only ${onlyTs.length}, luajit-only unexplained ${unexplainedIds.length}`)
  const radiusAll = new Set<number>(name.includes('Lethal Pride') ? ((lj.radius ?? []) as number[][]).flat() : [])
  let same = 0
  let cacheMiss = 0
  let timeless = 0
  const timelessIds: number[] = []
  for (const id of tsIds) {
    const o = lj.alloc[String(id)]
    const node = spec.allocNodes.get(id)
    if (o === undefined) continue
    if (node.type !== o.type || node.dn !== o.dn) v.push(`node ${id}: type/dn`)
    if ((node.keystoneMod === undefined ? undefined : formatSourceMod(node.keystoneMod)) !== (o.ks ?? undefined)) v.push(`node ${id}: keystoneMod`)
    const mods = node.modList.mods.map(formatSourceMod)
    if (ser(mods) === ser(o.mods)) same++
    else if (uncachedLines(id).length > 0) cacheMiss++
    else if (radiusAll.has(id)) { timeless++; timelessIds.push(id) }
    else v.push(`node ${id} ${node.dn}: mods differ, unexplained`)
  }
  if (spec.curClassName !== lj.className || spec.curAscendClassName !== lj.ascendName) v.push('class/ascendancy name')
  const counts = [spec.allocatedNotableCount, spec.allocatedKeystoneCount, spec.allocatedMasteryCount, spec.allocatedMasteryTypeCount]
  if (cluster.length === 0 && expansion.length === 0) {
    if (ser(counts) !== ser(lj.counts)) v.push(`counters ${counts} != ${lj.counts}`)
    if (spec.nodes.size !== lj.nSpec) v.push('spec.nodes size')
  } else {
    counts.forEach((c, i) => { if (c > lj.counts[i]) v.push(`counter ${i} ts ${c} > luajit ${lj.counts[i]}`) })
    if (spec.nodes.size >= lj.nSpec) v.push('spec.nodes not smaller')
  }
  let outside = 0
  if (name.includes('Lethal Pride')) { const r3 = new Set<number>(lj.radius?.[2] ?? []); outside = timelessIds.filter((id) => !r3.has(id)).length; if (outside > 0) v.push('timeless outside radius') }
  return { violations: v, alloc: [tsIds.length, ljIds.length], clusterIds: cluster.length, expansionSockets: expansion.length, modsIdentical: same, 'node-miss': cacheMiss, 'tree-jewel': timeless, timelessOutsideRadius: outside, counters: [counts, lj.counts], specNodes: [spec.nodes.size, lj.nSpec] }
}

// test/items/oracle.test.ts:94-104 + 366-400 (norm / diff) + 449-510
const MISS_IMMUNE = ['name', 'title', 'baseName', 'type', 'rarity', 'namePrefix', 'nameSuffix', 'itemLevel', 'corrupted', 'split', 'mirrored', 'fractured', 'synthesised', 'crafted', 'advancedCopy', 'isUnique', 'vestigial', 'foulborn', 'veiled', 'influences', 'uniqueID', 'league', 'classRestriction', 'limit', 'jewelRadiusIndex', 'jewelRadiusLabel', 'catalyst', 'catalystQuality', 'talismanTier', 'defaultSocketColor', 'variant', 'variantAlt', 'selectedVersion', 'usesVariantGroups', 'modSource', 'findModifierSubstring']
const MISS_IMMUNE_LINE = ['modTags', 'modGroup', 'flags', 'variantList', 'corruptedRange']
function norm(v: any): any { // eslint-disable-line
  if (v === undefined || v === null) return undefined
  if (Array.isArray(v)) return v.length === 0 ? '[]' : v.map(norm)
  if (typeof v === 'object') {
    const out: Rec = {}
    for (const k of Object.keys(v).sort()) { const n = norm(v[k]); if (n !== undefined) out[k] = n }
    return Object.keys(out).length === 0 ? '[]' : out
  }
  return v === '' ? undefined : v
}
function diff(path: string, a: unknown, b: unknown, out: string[]): void {
  const na = norm(a)
  const nb = norm(b)
  if (typeof na !== 'object' || typeof nb !== 'object' || na === null || nb === null) { if (JSON.stringify(na) !== JSON.stringify(nb)) out.push(path); return }
  for (const k of [...new Set([...Object.keys(na), ...Object.keys(nb)])].sort()) diff(`${path}.${k}`, (na as Rec)[k], (nb as Rec)[k], out)
}
function selfItems(lj: Rec, ts: Rec, misses: Record<string, string[]>): Rec {
  const r = { items: 0, lines: 0, clean: 0, withMiss: 0, missLines: new Set<string>(), violations: [] as string[] }
  if (ser(Object.keys(ts).sort()) !== ser(Object.keys(lj).sort())) r.violations.push('item ids differ')
  for (const id of Object.keys(lj).sort((a, b) => Number(a) - Number(b))) {
    const l = lj[id]
    const t = ts[id] ?? {}
    r.items++
    for (const list of LINE_LISTS) r.lines += (l[list] ?? []).length
    const differences: string[] = []
    diff(id, l, t, differences)
    for (const list of LINE_LISTS) {
      const a: Rec[] = l[list] ?? []
      const b: Rec[] = t[list] ?? []
      if (a.map((x) => x.line).join('\n') !== b.map((x) => x.line).join('\n')) r.violations.push(`${id}.${list} text`)
      if (a.length === b.length) for (let i = 0; i < a.length; i++) for (const k of MISS_IMMUNE_LINE) diff(`${id}.${list}.${i}.${k}`, a[i]![k], b[i]![k], r.violations)
    }
    for (const k of MISS_IMMUNE) diff(`${id}.${k}`, l[k], t[k], r.violations)
    for (const k of ['level', 'str', 'dex', 'int']) if ((t.requirements ?? {})[k] !== (l.requirements ?? {})[k]) r.violations.push(`${id}.requirements.${k}`)
    const m = misses[id] ?? []
    if (m.length === 0) { r.clean++; if (differences.length > 0) r.violations.push(`${id} ${l.name}: no ModCache miss yet ${differences.length} diffs, first ${differences[0]}`) }
    else { r.withMiss++; for (const x of m) r.missLines.add(x) }
  }
  return { ...r, missLines: r.missLines.size }
}

// test/config/oracle.test.ts:52-89
const EXPECTED_COUNTS: Rec = { 'Dual Savior': [36, 17], 'Dual Wield Cospris CoC': [44, 21], 'Dual Wield Cospris CoC - Lethal Pride': [44, 21], 'Generals Perforate Zerker': [62, 21], 'Mirage Archer Toxic Rain': [47, 20], OccVortex: [39, 22] }
function selfConfig(lj: Rec, ts: Rec, result: Rec, name: string): Rec {
  const v: string[] = []
  const seq = (label: string, a: string[], b: string[]): void => { if (ser(a) !== ser(b)) v.push(`${label}: ${a.length} vs ${b.length} entries or different order`) }
  seq('modList', ts.mods, lj.mods)
  seq('enemyModList', ts.enemyMods, lj.enemyMods)
  if (ts.enemyLevel !== lj.enemyLevel) v.push(`enemyLevel ${ts.enemyLevel} != ${lj.enemyLevel}`)
  for (const tb of ['input', 'placeholder']) {
    for (const k of new Set([...Object.keys(ts[tb]), ...Object.keys(lj[tb] ?? {})])) if (JSON.stringify(ts[tb][k]) !== JSON.stringify((lj[tb] ?? {})[k])) v.push(`${tb}.${k}: ts ${JSON.stringify(ts[tb][k])} vs luajit ${JSON.stringify((lj[tb] ?? {})[k])}`)
  }
  if (ts.characterLevel !== lj.characterLevel) v.push('characterLevel')
  if (result.unimplemented.length + result.warnings.length > 0) v.push(`pob-ts unimplemented ${result.unimplemented.length}, warnings ${result.warnings.length}`)
  const counts = [lj.mods.length, lj.enemyMods.length]
  if (ser(counts) !== ser(EXPECTED_COUNTS[name])) v.push(`mod counts ${counts} != pinned ${EXPECTED_COUNTS[name]}`)
  return { violations: v, modCounts: counts, ignoredLegacy: result.ignoredLegacy, nonIntegerNumbersInInput: Object.values(lj.input).filter((x) => typeof x === 'number' && !Number.isInteger(x)).length }
}

// test/calc/initEnv-oracle.test.ts:120-236 (classification) + 238-280 (pins)
const GENERIC_ITEM_SOURCES = new Set(['Quality', 'Item Sockets', 'Unlinked Item Sockets'])
const CLASSES = ['item-miss', 'node-miss', 'cluster', 'granted-passive', 'alloc-count', 'tree-jewel']
const isGem = (k: string): boolean => /^SocketedGemsIn/.test(k) || /^Socketed(?:Red|Green|Blue)GemsIn/.test(k) || /^EmptySocketIn/.test(k) || /^Empty(?:Red|Green|Blue|White)SocketsInAnySlot$/.test(k)
const EXPECTED_ENV: Rec = {
  'Dual Savior': { classified: [0, 1, 0, 0, 0, 0], alloc: [8, 8], itemSlots: [2, 2], oracle: [176, 216, 109, 116] },
  'Dual Wield Cospris CoC': { classified: [72, 12, 34, 1, 2, 12], alloc: [98, 121], itemSlots: [16, 22], oracle: [243, 552, 113, 120] },
  'Dual Wield Cospris CoC - Lethal Pride': { classified: [72, 15, 34, 1, 2, 26], alloc: [98, 121], itemSlots: [16, 22], oracle: [243, 565, 113, 120] },
  'Generals Perforate Zerker': { classified: [80, 28, 49, 4, 4, 0], alloc: [97, 127], itemSlots: [14, 22], oracle: [267, 619, 113, 120] },
  'Mirage Archer Toxic Rain': { classified: [35, 13, 17, 3, 2, 0], alloc: [103, 118], itemSlots: [13, 17], oracle: [240, 534, 112, 119] },
  OccVortex: { classified: [36, 12, 0, 3, 0, 0], alloc: [132, 133], itemSlots: [13, 13], oracle: [252, 564, 114, 121] },
}
function selfEnv(lj: Rec, run: Rec, name: string): Rec {
  const { env, spec, items, missItems } = run
  const sourceRef = (s: string): Rec | undefined => {
    const i = /^Item:(\d+):/.exec(s)
    if (i !== null) return { kind: 'item', id: Number(i[1]) }
    const t = /^Tree:?(\d+)$/.exec(s)
    return t !== null ? { kind: 'tree', id: Number(t[1]) } : undefined
  }
  const oracleOnlyAlloc = new Set<number>(lj.alloc.filter((id: number) => !env.allocNodes.has(id)))
  const unequipped = new Set<number>()
  const v: string[] = []
  for (const slotName of Object.keys(lj.itemSlots)) {
    if (env.player.itemList[slotName] !== undefined) continue
    const m = /^Jewel (\d+)$/.exec(slotName)
    const itemId = m === null ? undefined : spec.jewels.get(Number(m[1]))
    if (itemId === undefined) v.push(`luajit equipped ${slotName}, pob-ts has no item for it`)
    else unequipped.add(itemId)
  }
  const grantedSet = new Set<number>(lj.grantedPassives)
  const hasTreeJewel = env.unimplemented.some((u: string) => u.includes('radius jewel')) || [...items.values()].some((i: Rec) => i.base?.subType === 'Timeless')
  const missNode = (id: number): boolean => { const n = spec.nodes.get(id) ?? spec.tree.nodes.get(id); return n !== undefined && (n.unknown === true || n.mods.some((m: Rec) => m !== undefined && m.extra !== undefined)) }
  const classify = (n: string, source: string, oracleOnly: boolean): string => {
    const ref = sourceRef(source)
    if (ref?.kind === 'item') { if (missItems.has(ref.id)) return 'item-miss'; if (oracleOnly && unequipped.has(ref.id)) return 'cluster' }
    if (ref?.kind === 'tree') {
      if (oracleOnly && oracleOnlyAlloc.has(ref.id)) return grantedSet.has(ref.id) ? 'granted-passive' : 'cluster'
      if (missNode(ref.id)) return 'node-miss'
      if (hasTreeJewel) return 'tree-jewel'
    }
    if (n.startsWith('Multiplier:Allocated') && oracleOnlyAlloc.size > 0) return 'alloc-count'
    if (oracleOnly && GENERIC_ITEM_SOURCES.has(source) && unequipped.size > 0) return 'cluster'
    return 'unexplained'
  }
  const cls: Record<string, string[]> = Object.fromEntries([...CLASSES, 'unexplained'].map((c) => [c, []]))
  const key = (n: string, f: string): string => JSON.stringify([n, f])
  const compareDb = (label: string, tsDB: Rec, oDB: Rec): void => {
    const tsMods: Rec = {}
    const tsSrc = new Map<string, string>()
    for (const [n, bucket] of tsDB.mods as Map<string, Rec[]>) {
      tsMods[n] = bucket.map((m) => normAddr(formatSourceMod(m)))
      for (const m of bucket) tsSrc.set(key(n, normAddr(formatSourceMod(m))), m.source ?? '')
    }
    const oSrc = new Map<string, string>()
    for (const n of Object.keys(oDB.mods)) (oDB.mods[n] as string[]).forEach((s, i) => oSrc.set(key(n, s), oDB.srcs[n]?.[i] ?? ''))
    const count = (l: string[]): Map<string, number> => { const m = new Map<string, number>(); for (const s of l) m.set(s, (m.get(s) ?? 0) + 1); return m }
    for (const n of [...new Set([...Object.keys(tsMods), ...Object.keys(oDB.mods)])].sort()) {
      const a = count(tsMods[n] ?? [])
      const b = count(oDB.mods[n] ?? [])
      for (const s of [...new Set([...a.keys(), ...b.keys()])].sort()) {
        const x = a.get(s) ?? 0
        const y = b.get(s) ?? 0
        if (x === y) continue
        const source = oSrc.get(key(n, s)) ?? tsSrc.get(key(n, s)) ?? ''
        cls[classify(n, source, y > x)]!.push(`${label}.mods[${n}] TSx${x} LJx${y} src=${source} :: ${s}`)
      }
    }
    for (const k of [...new Set([...Object.keys(tsDB.multipliers), ...Object.keys(oDB.multipliers)])].sort()) {
      if (isGem(k)) continue
      const raw = tsDB.multipliers[k]
      const a = raw === undefined ? undefined : formatG(raw, 17)
      const b = oDB.multipliers[k]
      if (a !== b) cls[classify('Multiplier:' + k, '', b !== undefined)]!.push(`${label}.multipliers[${k}]: TS ${a} != LJ ${b}`)
    }
    for (const k of new Set([...Object.keys(tsDB.conditions), ...Object.keys(oDB.conditions)])) {
      if (JSON.stringify(tsDB.conditions[k]) !== JSON.stringify(oDB.conditions[k])) cls['unexplained']!.push(`${label}.conditions[${k}]`)
    }
  }
  compareDb('modDB', env.modDB, lj.modDB)
  compareDb('enemyDB', env.enemyDB, lj.enemyDB)
  const chk = (ok: boolean, m: string): void => { if (!ok) v.push(m) }
  chk(env.enemyLevel === lj.enemyLevel, 'enemyLevel')
  chk(env.mainSocketGroup === lj.mainSocketGroup, 'mainSocketGroup')
  chk(ser(env.orderedSlots.map((s: Rec) => s.slotName)) === ser(lj.orderedSlots), 'orderedSlots')
  chk(env.classId === lj.classId, 'classId')
  chk(env.grantedSkills.length <= lj.grantedSkills && env.initialNodeModDB.mods.length <= lj.initialNodeMods, 'grantedSkills/initialNodeModDB over-count')
  for (const id of env.allocNodes.keys()) chk(lj.alloc.includes(id), `allocNodes: ts has ${id}, luajit not`)
  for (const id of oracleOnlyAlloc) chk(!spec.nodes.has(id) || spec.tree.nodes.get(id)?.expansionJewel?.parent !== undefined || grantedSet.has(id), `luajit-only node ${id} unexplained`)
  for (const s of Object.keys(env.player.itemList)) chk(lj.itemSlots[s] === env.player.itemList[s]?.name, `itemList[${s}]`)
  chk(oracleOnlyAlloc.size > 0 || ser(Object.keys(env.keystonesAdded).sort()) === ser(lj.keystonesAdded), 'keystonesAdded')
  for (const [slot, ours, theirs] of [['Weapon 1', env.player.weaponData1, lj.weapon1], ['Weapon 2', env.player.weaponData2, lj.weapon2]] as const) {
    const item = env.player.itemList[slot]
    if (item?.id !== undefined && missItems.has(item.id)) continue
    for (const k of new Set([...Object.keys(ours), ...Object.keys(theirs ?? {})])) {
      const a = ours[k]
      chk((typeof a === 'number' ? formatG(a, 17) : a === undefined ? undefined : String(a)) === theirs?.[k], `${slot} weaponData[${k}]`)
    }
  }
  const counts = CLASSES.map((c) => cls[c]!.length)
  const pin = EXPECTED_ENV[name]
  const modCount = (d: Rec): number => [...d.mods.values()].reduce((n: number, b: Rec[]) => n + b.length, 0)
  const ljCount = (d: Rec): number => Object.values(d.mods as Rec).reduce((n: number, b) => n + (b as unknown[]).length, 0)
  const oracle = [Object.keys(lj.modDB.mods).length, ljCount(lj.modDB), Object.keys(lj.enemyDB.mods).length, ljCount(lj.enemyDB)]
  return {
    violations: [...v, ...cls['unexplained']!.map((x) => 'UNEXPLAINED ' + x)],
    classes: Object.fromEntries(CLASSES.map((c, i) => [c, counts[i]])),
    matchesPob_tsWasmPins: ser(counts) === ser(pin.classified) && ser(oracle) === ser(pin.oracle) && ser([env.allocNodes.size, lj.alloc.length]) === ser(pin.alloc) &&
      ser([Object.keys(env.player.itemList).length, Object.keys(lj.itemSlots).length]) === ser(pin.itemSlots),
    pinned: { classes: pin.classified, luajitNamesModsEnemyNamesMods: pin.oracle }, luajitNamesModsEnemyNamesMods: oracle,
    pobtsNamesMods: [env.modDB.mods.size, modCount(env.modDB), env.enemyDB.mods.size, modCount(env.enemyDB)],
    alloc: [env.allocNodes.size, lj.alloc.length], itemSlots: [Object.keys(env.player.itemList).length, Object.keys(lj.itemSlots).length],
  }
}

// --- perf --------------------------------------------------------------------------------------------------------
const RUNS = 5
const WARM = 5
const median = (a: number[]): number => [...a].sort(numAsc)[Math.floor(a.length / 2)]!
const stat = (a: number[]): Rec => ({ median: median(a), min: Math.min(...a), max: Math.max(...a), n: a.length })
const XML_OF = (name: string): Buffer => readFileSync(GOLDEN_BUILDS.find((b: Rec) => b.name === name).xmlPath(roots))

/** One pob-ts "XML -> env": loadBuildXml + parse items + loadSpec + fresh initEnv (the same path as the oracle test). */
function perfChild(name: string): void {
  const xml = XML_OF(name).toString('utf8')
  const prep = (): Rec => { const model = loadBuildXml(xml); return { model, ...tsEnvRun(model) } }
  const t1 = performance.now()
  prep()
  const cold = performance.now() - t1
  const warm: number[] = []
  const initOnly: number[] = []
  for (let i = 0; i < WARM; i++) { const t = performance.now(); prep(); warm.push(performance.now() - t) }
  for (let i = 0; i < WARM; i++) {
    const model = loadBuildXml(xml)
    const items = new Map<number, Rec>()
    for (const im of model.items) { const it = Item.parse(im.raw, itemOpts(im)); if (it.base !== undefined) items.set(im.id, it) }
    const spec = loadSpec(activeSpec(model))
    const t = performance.now()
    initEnv(model, { spec, items })
    initOnly.push(performance.now() - t)
  }
  console.log(JSON.stringify({ importMs: IMPORT_MS, cold, warm, initOnly }))
}
async function perfAll(): Promise<void> {
  const luts = await prepareLuts()
  const out: Rec = { node: process.version, runs: RUNS, warmPerRun: WARM, builds: {} }
  for (const b of GOLDEN_BUILDS) {
    const xml = XML_OF(b.name)
    const code = `local t0 = os.clock()\nloadBuildFromXML(${luaBytes(xml)}, ${luaStr(b.name)})\nlocal load = os.clock() - t0\n` +
      'if __mainObject__.promptMsg then error("FATAL " .. tostring(__mainObject__.promptMsg)) end\ncollectgarbage()\nlocal calcs, init = build.calcsTab.calcs, {}\n' +
      `for i = 1, ${WARM + 1} do local t = os.clock(); local env = calcs.initEnv(build, "MAIN"); init[i] = os.clock() - t end\nreturn { jit = jit.version, load = load, init = init }\n`
    const lj = Array.from({ length: RUNS }, () => ljEval(code, luts))
    const ts = Array.from({ length: RUNS }, () => JSON.parse(execFileSync(process.execPath, [fileURLToPath(import.meta.url), 'perf-child', b.name], { encoding: 'utf8' }).trim().split('\n').pop()!) as Rec)
    const ms = (s: number): number => s * 1000
    out.builds[b.name] = {
      luajit: {
        version: lj[0].jit, loadBuildFromXML_ms: stat(lj.map((r) => ms(r.load))), initEnvFirst_ms: stat(lj.map((r) => ms(r.init[0]))),
        initEnvWarm_ms: stat(lj.flatMap((r) => (r.init as number[]).slice(1).map(ms))),
        xmlToEnv_ms: stat(lj.map((r) => ms(r.load + r.init[0]))),
      },
      pobts: { cold_ms: stat(ts.map((r) => r.cold)), warm_ms: stat(ts.flatMap((r) => r.warm)), initEnvOnly_ms: stat(ts.flatMap((r) => r.initOnly)), importModules_ms: stat(ts.map((r) => r.importMs)) },
    }
    console.error(`perf ${b.name}: done`)
  }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-env.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(out, null, 2))
}

// --- main --------------------------------------------------------------------------------------------------------
async function fixtures(): Promise<void> {
  const luts = await prepareLuts()
  let digest = ''
  try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
  const generator = `pob-ts ${git(POB_TS)}`
  const report: Rec = {}
  let first = true
  let global: Rec | undefined
  for (const b of GOLDEN_BUILDS) {
    const xmlBuf = XML_OF(b.name)
    const model = loadBuildXml(xmlBuf.toString('utf8'))
    const cases: RangeCase[] = [...rangeLinesOf(model), ...(first ? APPLY_RANGE_CASES.map((c: Rec) => ({ line: c.line, range: c.range, valueScalar: c.valueScalar, baseValueScalar: c.baseValueScalar })) : [])]
    // pob-ts half
    const { dump: treeTs, spec: treeSpec } = tsTree(model)
    const misses: Record<string, string[]> = {}
    const itemsTs = tsItems(model, misses)
    const rangeTs = cases.map((c) => applyRange(c.line, c.range, c.valueScalar, c.baseValueScalar))
    const { dump: configTs, result } = tsConfig(model)
    const run = tsEnvRun(model)
    const envTs = tsEnvDump(run.env, model)
    // LuaJIT half (one docker run: XML + 4 probes, fresh initEnv last)
    const raw = ljEval(luajitChunk(xmlBuf, b.name, cases), luts)
    const lj = normDeep(raw) as Rec
    const rangeLj = { cases, results: lj.applyRange }
    // self-check
    if (first) global = selfTreeGlobal(lj.tree)
    const rangeBad = cases.filter((_, i) => rangeTs[i] !== lj.applyRange[i]).length
    const synthWasm = first ? APPLY_RANGE_CASES.filter((c: Rec, i: number) => lj.applyRange[cases.length - APPLY_RANGE_CASES.length + i] !== c.expected).length : undefined
    const sc = {
      applyRange: { cases: cases.length, luajitVsPobts: rangeBad, syntheticVsWasmExpected: synthWasm },
      tree: selfTree(lj.tree, treeTs, treeSpec, b.name), items: selfItems(lj.items, normDeep(itemsTs), misses),
      config: selfConfig(lj.config, configTs, result, b.name), env: selfEnv(lj.env, run, b.name),
    }
    const head = { build: b.name, format: 1, xmlSha256: createHash('sha256').update(xmlBuf).digest('hex') }
    const slug = b.name.replace(/ /g, '-')
    const files = [
      ...writeFixture(slug, 'luajit', { ...head, producer: { generator, image: digest || LUAJIT_IMAGE, luajit: raw.jit, pobSha: git('C:/pob-work/project_15/PathOfBuilding'), probes: 'pob-ts test/{tree/spec,items/oracle,config/oracle,calc/initEnv-oracle}.test.ts, Lua verbatim' } },
        { tree: lj.tree, items: lj.items, applyRange: rangeLj, config: lj.config, env: lj.env }),
      ...writeFixture(slug, 'pobts', { ...head, producer: { generator, probes: 'TS halves of the same tests' } },
        normDeep({ tree: treeTs, items: itemsTs, applyRange: { cases, results: rangeTs }, config: configTs, env: envTs })),
    ]
    report[b.name] = { files, selfCheck: sc }
    first = false
    console.error(`${b.name}: done`)
  }
  console.log(JSON.stringify({ treeGlobal: global, builds: report }, null, 1))
}

if (process.argv[2] === 'perf-child') perfChild(process.argv[3]!)
else if (process.argv[2] === 'perf') await perfAll()
else await fixtures()
