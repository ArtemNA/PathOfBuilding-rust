// R3.1: fixtures for the R3 gate -- the four probes (tree / items+applyRange / config / fresh initEnv) of pob-ts' oracle tests dumped
// for the 6 golden builds from REAL LuaJIT (docker, PoB boot + loadBuildFromXML in the chunk), plus the perf "XML -> env" of LuaJIT.
// Truth = LuaJIT. Until R-INDEPENDENT-UPDATE A4 this probe also ran pob-ts' TS halves (`<build>.pobts.json` + a self-check); pob-ts
// is no longer a comparison subject, and nothing here imports it: the Lua probes were copied verbatim from pob-ts tests (file:line
// next to each, pob-ts commit 70911c1b629cfe6887795cb40356c6d298c1922e), the applyRange inputs are the `{range:}` lines of the
// golden XMLs (read as pob-ts `loadBuildXml` + test/items/oracle.test.ts:49-61 did) + the 23 synthetic cases of pob-ts
// test/items/cases.ts (copied below).
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-env.ts          -> crates/pob-calc/tests/fixtures/<build>.luajit.json
//   node tools/lua-corpus/export-env.ts perf     -> tools/.scratch/perf-env.json
//   `--out <dir>`: write the fixtures there instead (a re-take to compare)
// Only normalisation: `function: 0x<heap address>` (a JewelFunc mod) -> `function: <addr>` (initEnv-oracle.test.ts:178).
import { POB as UPSTREAM_POB } from '../upstream.ts'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import('../oracle/paths.ts')
type Rec = Record<string, any> // eslint-disable-line

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = (() => {
  const i = process.argv.indexOf('--out')
  if (i < 0) return resolve(ROOT, 'crates/pob-calc/tests/fixtures')
  const d = resolve(process.argv[i + 1] ?? '')
  process.argv.splice(i, 2)
  return d
})()
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_450_000
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const roots = resolveRoots()
const { LUAJIT_IMAGE } = await import('../oracle/luajit.ts')
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
  const { findRequiredJewelLuts } = await import('../oracle/jewel-luts.ts')
  const { unpackJewelData } = await import('../oracle/jewel-luts.ts')
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


// --- applyRange inputs -----------------------------------------------------------------------------------------
// pob-ts test/items/cases.ts APPLY_RANGE_CASES (P4.3a), without `expected` (that was the wasm oracle's answer; LuaJIT answers here).
const APPLY_RANGE_CASES: RangeCase[] = [
  { line: '(40-50)% increased Physical Damage', range: 0.5 },
  { line: '(40-50)% increased Physical Damage', range: 1 },
  { line: 'Adds (16-22) to (40-45) Physical Damage', range: 0.5 },
  { line: '(8-12)% increased Attack Speed', range: 0.5 },
  { line: '(8-12)% increased Critical Strike Chance', range: 0.5 },
  { line: '(200-250)% increased Physical Damage', range: 0.5 },
  { line: 'Critical Strike Chance is (20-30)% for Hits with this Weapon', range: 0.5 },
  { line: '(10-15)% increased Energy Shield Recharge Rate', range: 0.37 },
  { line: '-(10-15)% increased Energy Shield Recharge Rate', range: 1 },
  { line: '(-10--5)% increased Energy Shield Recharge Rate', range: 0 },
  { line: '-15% more Attack Speed', range: 1 },
  { line: '+(-10-15)% to Fire Resistance', range: 0 },
  { line: '+(-10-15)% to Fire Resistance', range: 1 },
  { line: 'Regenerate (60-120) Life per minute', range: 0.5 },
  { line: '(0.4-0.6)% of Physical Attack Damage Leeched as Life', range: 0.5 },
  { line: '(1.5-2.5)% of Physical Attack Damage Leeched as Mana', range: 0.37 },
  { line: '+(0.2-0.4)% to Critical Strike Chance', range: 0.5 },
  { line: 'You have no Intelligence', range: 0.5 },
  { line: '(8-12)% increased Attack Speed', range: 0.5, valueScalar: 1.2 },
  { line: '(8-12)% increased Attack Speed', range: 0.5, baseValueScalar: 1.2 },
  { line: '+(20-30) to maximum Life', range: 0.5, valueScalar: 1.2, baseValueScalar: 1.1 },
  { line: '(15-25)% increased Cooldown Recovery Rate', range: 0.5, valueScalar: 1.2 },
  { line: 'Adds (1-2) to (3-4) Fire Damage to Attacks', range: 0.25 },
]
/** runtime/lua/xml.lua:9-15 `decodeContent`: the five entities, any other `&...;` -> "" (pob-ts src/build/xml.ts, MINEFIELD D22). */
const ENT: Record<string, string> = { quot: '"', apos: "'", lt: '<', gt: '>', amp: '&' }
const decodeContent = (s: string): string => s.replace(/&(.*?);/g, (_, e: string) => ENT[e] ?? '')
/** The text of every `<Item>` of `<Items>`, in XML order (child elements such as `<ModRange/>` and comments dropped). */
function itemTexts(xml: string): string[] {
  const items = /<Items\b[^>]*>([\s\S]*?)<\/Items>/.exec(xml.replace(/<!--[\s\S]*?-->/g, ''))?.[1] ?? ''
  return [...items.matchAll(/<Item\b[^>]*?(?:\/>|>([\s\S]*?)<\/Item>)/g)].map((m) => decodeContent((m[1] ?? '').replace(/<[^>]*>/g, '')))
}
/** test/items/oracle.test.ts:49-61 -- every `{range:X}` mod line of a build. */
function rangeLinesOf(xml: string): RangeCase[] {
  const out: RangeCase[] = []
  for (const raw of itemTexts(xml)) {
    for (const l of raw.split('\n')) {
      const line = l.trim()
      const r = /\{range:([^}]+)\}/.exec(line)
      if (r !== null) out.push({ line: line.replace(/\{[A-Za-z]*:?[^}]*\}/g, '').replace(/ \([a-z]+\)/g, ''), range: Number(r[1]) })
    }
  }
  return out
}

// --- perf --------------------------------------------------------------------------------------------------------
const RUNS = 5
const WARM = 5
const numAsc = (a: number, b: number): number => a - b
const median = (a: number[]): number => [...a].sort(numAsc)[Math.floor(a.length / 2)]!
const stat = (a: number[]): Rec => ({ median: median(a), min: Math.min(...a), max: Math.max(...a), n: a.length })
const XML_OF = (name: string): Buffer => readFileSync(GOLDEN_BUILDS.find((b: Rec) => b.name === name)!.xmlPath(roots))

/** LuaJIT "XML -> env": loadBuildFromXML + fresh initEnv, per golden (pob-ts' half of this perf is gone with A4). */
async function perfAll(): Promise<void> {
  const luts = await prepareLuts()
  const out: Rec = { runs: RUNS, warmPerRun: WARM, builds: {} }
  for (const b of GOLDEN_BUILDS) {
    const xml = XML_OF(b.name)
    const code = `local t0 = os.clock()\nloadBuildFromXML(${luaBytes(xml)}, ${luaStr(b.name)})\nlocal load = os.clock() - t0\n` +
      'if __mainObject__.promptMsg then error("FATAL " .. tostring(__mainObject__.promptMsg)) end\ncollectgarbage()\nlocal calcs, init = build.calcsTab.calcs, {}\n' +
      `for i = 1, ${WARM + 1} do local t = os.clock(); local env = calcs.initEnv(build, "MAIN"); init[i] = os.clock() - t end\nreturn { jit = jit.version, load = load, init = init }\n`
    const lj = Array.from({ length: RUNS }, () => ljEval(code, luts))
    const ms = (s: number): number => s * 1000
    out.builds[b.name] = {
      luajit: {
        version: lj[0].jit, loadBuildFromXML_ms: stat(lj.map((r) => ms(r.load))), initEnvFirst_ms: stat(lj.map((r) => ms(r.init[0]))),
        initEnvWarm_ms: stat(lj.flatMap((r) => (r.init as number[]).slice(1).map(ms))),
        xmlToEnv_ms: stat(lj.map((r) => ms(r.load + r.init[0]))),
      },
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
  const generator = 'tools/lua-corpus/export-env.ts'
  const report: Rec = {}
  let first = true
  for (const b of GOLDEN_BUILDS) {
    const xmlBuf = XML_OF(b.name)
    const cases: RangeCase[] = [...rangeLinesOf(xmlBuf.toString('utf8')), ...(first ? APPLY_RANGE_CASES : [])]
    // one docker run: XML + 4 probes, fresh initEnv last
    const raw = ljEval(luajitChunk(xmlBuf, b.name, cases), luts)
    const lj = normDeep(raw) as Rec
    const head = { build: b.name, format: 1, xmlSha256: createHash('sha256').update(xmlBuf).digest('hex') }
    const slug = b.name.replace(/ /g, '-')
    const files = writeFixture(slug, 'luajit', { ...head, producer: { generator, image: digest || LUAJIT_IMAGE, luajit: raw.jit, pobSha: git(UPSTREAM_POB), probes: 'pob-ts test/{tree/spec,items/oracle,config/oracle,calc/initEnv-oracle}.test.ts, Lua verbatim' } },
      { tree: lj.tree, items: lj.items, applyRange: { cases, results: lj.applyRange }, config: lj.config, env: lj.env })
    report[b.name] = { files, applyRangeCases: cases.length }
    first = false
    console.error(`${b.name}: done`)
  }
  console.log(JSON.stringify({ builds: report }, null, 1))
}

if (process.argv[2] === 'perf') await perfAll()
else await fixtures()
