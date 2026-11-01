// R8.8: LuaJIT truth for the pob-engine API (boot.lua `engine_*`, the logic copied as export-r8.ts does) on the 26 R8.1 scenarios (6 goldens + 20 R7-synth).
// Per scenario, in the LuaJIT docker (mechanics = export-r8.ts: LUT mount, `%.17g` numbers, `$str:` marker for numeric-looking strings):
//   readers  = engine_outputJSON (every scalar of mainOutput + _meta), engine_skillListJSON, engine_fullDPSJSON, engine_configListJSON (with `eligible`),
//              engine_statsForJSON(g) for every socket group (as a diff against the base output);
//   mutators = boot.lua's own paths (allocNode/deallocNode: spec:AllocNode/DeallocNode + BuildAllDependsAndPaths; setConfig: configTab.input + BuildModList;
//              setGem: boot.lua:405-460; setMainSocketGroup) + recalc (wipeGlobalCache + calcsTab:BuildOutput), then the inverse; each recorded as
//              { op, args, changed, after = diff vs base, backEqual, back = diff vs base }. Candidate choice = the Rust test's rule (lowest id first, until the output changes);
//   perf     = os.clock CPU ms (jit.on): loadBuildFromXML, recalc, recalc after a mutation (median of the scenario's mutations).
// Optional (tools/.scratch/rust-engine-codes.json from `cargo test -p pob-engine --test engine_luajit -- --ignored write_rust_codes`): LuaJIT common.base64 decodes the
// Rust build code (bytes = Rust zlib; inflated by Node, HeadlessWrapper's Inflate is a stub) and loads the Rust SaveDB XML -> its output vs the base output.
// Usage (Node 24, repo root): node tools/lua-corpus/export-engine.ts [name-filter]  -> crates/pob-engine/tests/fixtures/engine/<slug>.engine.json (2 passes, must be identical
//                             apart from the perf block, which goes to tools/.scratch/perf-engine.json)
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { promisify } from 'node:util'
import { inflateSync } from 'node:zlib'
const run = promisify(execFile)
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-engine/tests/fixtures/engine')
const SYNTH = resolve(ROOT, 'crates/pob-calc/tests/fixtures/offence-synth')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const JOBS = Number(process.env.R8_JOBS ?? 4)
const roots = resolveRoots()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const median = (xs: number[]): number => { const s = [...xs].sort((x, y) => x - y); return s[Math.floor(s.length / 2)] ?? 0 }
const SYNTH_SET = ['fd-gen-melee', 'fd-occ-spells', 'fd-occ-dot', 'fd-occ-trap-mine-totem', 'fd-occ-minions', 'fd-mir-mirage', 'fd-mir-bow-ballista', 'fd-cos-coc', 'fd-occ-poison-ignite', 'fd-all-ds',
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
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r88-'))
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

const PROBE = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local function N(v) if v ~= v then return "nan" end return RAWF("%.17g", v) end
local function S(v) if v == "nan" or v == "inf" or v == "-inf" or v:find("^%-?[%d%.]+[eE]?[%+%-]?%d*$") then return "$str:" .. v end return v end
local function now() return os.clock() * 1000 end
jit.on()
local P = { t = {} }
local t0 = now()
loadBuildFromXML(XML, NAME)
if __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end
P.t.load = now() - t0
local function recalc() wipeGlobalCache(); build.calcsTab:BuildOutput() end
local t1 = now(); recalc(); P.t.recalc = now() - t1
-- engine_outputJSON (boot.lua:790-822)
local function flat()
  local o = obj()
  for k, v in pairs(build.calcsTab.mainOutput) do
    local tv = type(v)
    if tv == "number" then o[k] = N(v) elseif tv == "string" then o[k] = S(v) elseif tv == "boolean" then o[k] = v end
  end
  return o
end
local function diff(a, b)
  local d, n = obj(), 0
  for k, v in pairs(b) do if a[k] ~= v then d[k] = v n = n + 1 end end
  for k in pairs(a) do if b[k] == nil then d[k] = "$nil" n = n + 1 end end
  return d, n
end
local base = flat()
P.output = base
local nAlloc = 0 for _ in pairs(build.spec.allocNodes) do nAlloc = nAlloc + 1 end
local nItems = 0 for _ in pairs(build.itemsTab.items) do nItems = nItems + 1 end
P.meta = { allocNodes = nAlloc, items = nItems, socketGroups = #build.skillsTab.socketGroupList, mainSocketGroup = N(build.mainSocketGroup), treeVersion = build.spec.treeVersion, className = build.spec.curClassName, ascendName = build.spec.curAscendClassName }
-- engine_skillListJSON (boot.lua:369-403)
local sl = {}
for index, group in ipairs(build.skillsTab.socketGroupList) do
  local gems = {}
  for _, gi in ipairs(group.gemList) do
    local ge = (gi.gemData and gi.gemData.grantedEffect) or gi.grantedEffect
    -- R13 (SkillsTab.lua:472-477): a saved gem whose gameId has several variants with this grantedEffectId resolves per process -> canonical name
    local name = gi.nameSpec
    if gi.gemData and gi.skillId and data.gemsByGameId[gi.gemData.gameId] then
      local c = 0
      for _, v in pairs(data.gemsByGameId[gi.gemData.gameId]) do if v.grantedEffectId == gi.skillId then c = c + 1 end end
      if c > 1 then name = "<R13-ambiguous>" end
    end
    gems[#gems + 1] = { name = name, level = gi.level and N(gi.level), quality = gi.quality and N(gi.quality), enabled = gi.enabled and true or false, isSupport = (ge and ge.support) and true or false }
  end
  sl[#sl + 1] = { index = index, label = group.displayLabel or group.label or ("Group " .. index), enabled = group.enabled and true or false, includeInFullDPS = group.includeInFullDPS and true or false, isMainGroup = (index == build.mainSocketGroup), gems = gems }
end
P.skillList = sl
-- engine_fullDPSJSON (boot.lua:506-520)
local fd = {}
for _, s in ipairs(build.calcsTab.mainOutput.SkillDPS or {}) do fd[#fd + 1] = { name = s.name, dps = N(s.dps), count = N(s.count), trigger = s.trigger, skillPart = s.skillPart and tostring(s.skillPart), source = s.source } end
P.fullDPS = fd
-- engine_configListJSON (boot.lua:249-322)
local configTab = build.configTab
local cl, section = {}, nil
local function J(v) if type(v) == "number" then return N(v) elseif type(v) == "string" then return S(v) end return v end
for _, varData in ipairs(require("Modules.ConfigOptions")) do
  if varData.section then section = varData.section
  elseif varData.var and varData.type then
    local cur = configTab.input[varData.var]
    local value = cur
    if cur == nil then value = configTab:GetDefaultState(varData.var, type(cur)) end
    local raw = configTab.input[varData.var]
    local control = configTab.varControls[varData.var]
    local e = { id = varData.var, type = varData.type, label = varData.label and StripEscapes(varData.label) or varData.var, section = section, value = J(value),
      isSet = raw ~= nil and raw ~= configTab.defaultState[varData.var], eligible = control and control:GetProperty("shown") and true or false }
    if varData.type == "list" and varData.list then
      local opts = {}
      for _, opt in ipairs(varData.list) do opts[#opts + 1] = { value = J(opt.val), label = opt.label and StripEscapes(opt.label) or tostring(opt.val) } end
      e.options = opts
    end
    cl[#cl + 1] = e
  end
end
P.configList = cl
-- engine_statsForJSON(g) (boot.lua:482-500) as diffs
local sf = {}
local orig = build.mainSocketGroup
for g = 1, #build.skillsTab.socketGroupList do
  build.mainSocketGroup = g; recalc()
  local d, n = diff(base, flat())
  build.mainSocketGroup = orig; recalc()
  local _, nb = diff(base, flat())
  sf[g] = { after = d, changed = n, backEqual = nb == 0 }
end
P.statsFor = sf
-- mutators: { op, args, changed, after, backEqual, back }
local muts, tm = {}, {}
local function try(op, args, mutate, revert)
  local ok, err = pcall(function() local t = now(); mutate(); tm[#tm + 1] = now() - t end)
  if not ok then muts[#muts + 1] = { op = op, args = args, error = tostring(err) } return false end
  local a, n = diff(base, flat())
  revert()
  local b, nb = diff(base, flat())
  muts[#muts + 1] = { op = op, args = args, changed = n, after = a, backEqual = nb == 0, back = b }
  return n > 0
end
local spec = build.spec
local function ids(pred)
  local l = {}
  for id, node in pairs(spec.nodes) do if pred(node) then l[#l + 1] = id end end
  table.sort(l)
  return l
end
local function plain(node) return node.id < 65536 and node.type ~= "Mastery" and node.type ~= "ClassStart" and node.type ~= "AscendClassStart" and not node.expansionJewel and not node.ascendancyName end
local function alloc(id) local n = spec.nodes[id] if not n.alloc then spec:AllocNode(n); spec:BuildAllDependsAndPaths(); recalc() end end
local function dealloc(id) local n = spec.nodes[id] if n.alloc then spec:DeallocNode(n); spec:BuildAllDependsAndPaths(); recalc() end end
for i, id in ipairs(ids(function(n) return not n.alloc and n.path and #n.path == 1 and plain(n) end)) do
  if i > 12 or try("allocNode", { id = id }, function() alloc(id) end, function() dealloc(id) end) then break end
end
for i, id in ipairs(ids(function(n) return n.alloc and #n.depends == 1 and plain(n) and not n.isJewelSocket end)) do
  if i > 12 or try("deallocNode", { id = id }, function() dealloc(id) end, function() alloc(id) end) then break end
end
local function setConfig(id, v) configTab.input[id] = v; configTab:BuildModList(); recalc() end
for _, kind in ipairs({ { "check", { "conditionKilledRecently", "buffOnslaught", "conditionFullLife", "usePowerCharges", "conditionEnemyBlinded" } },
                        { "count", { "enemyLevel", "enemyPhysicalReduction", "enemyArmour", "enemyEvasion", "enemyChaosResist" } } }) do
  for _, var in ipairs(kind[2]) do
    local before = configTab.input[var]
    local v
    if kind[1] == "check" then v = not (before == true) else v = (before == 70) and 60 or 70 end
    if try("setConfig", { id = var, value = v }, function() setConfig(var, v) end, function() setConfig(var, before) end) then break end
  end
end
local function setGem(g, k, prop, value)
  local group = build.skillsTab.socketGroupList[g]
  local gi = group.gemList[k]
  if prop == "enabled" and gi.gemData and not gi.gemData.vaalGem then gi.enableGlobal1 = true gi.enableGlobal2 = true end
  gi[prop] = value
  build.skillsTab:ProcessSocketGroup(group)
  build.skillsTab:UpdateSocketGroups()
  recalc()
end
local main = build.mainSocketGroup
local group = build.skillsTab.socketGroupList[main]
if group then
  for k, gi in ipairs(group.gemList) do
    if gi.level and gi.quality then
      local l, q = gi.level, gi.quality
      local nl = l > 1 and l - 1 or l + 1
      local nq = q > 0 and 0 or 20
      try("setGem", { group = main, gem = k, prop = "level", value = nl }, function() setGem(main, k, "level", nl) end, function() setGem(main, k, "level", l) end)
      try("setGem", { group = main, gem = k, prop = "quality", value = nq }, function() setGem(main, k, "quality", nq) end, function() setGem(main, k, "quality", q) end)
      break
    end
  end
  for k, gi in ipairs(group.gemList) do
    local ge = (gi.gemData and gi.gemData.grantedEffect) or gi.grantedEffect
    if gi.enabled and ge and ge.support then
      try("setGem", { group = main, gem = k, prop = "enabled", value = false }, function() setGem(main, k, "enabled", false) end, function() setGem(main, k, "enabled", true) end)
      break
    end
  end
end
if #build.skillsTab.socketGroupList >= 2 then
  local other = main == 1 and 2 or 1
  try("setMainSocketGroup", { group = other }, function() build.mainSocketGroup = other; recalc() end, function() build.mainSocketGroup = main; recalc() end)
end
-- engine_addItemText (boot.lua:1336-1395) of a fixed rare ring into "Ring 1" (CopyAnointsAndEldritchImplicits: no "Ring" slot -> no-op);
-- inverse: the previous ring back (ItemSlotControl:SetSelItemId + PopulateSlots), the new item stays in the list
local RING = table.concat({ "Rarity: RARE", "Engine Test Loop", "Iron Ring", "Implicits: 1", "Adds 1 to 4 Physical Damage to Attacks", "+30 to maximum Life", "+20% to Fire Resistance" }, string.char(10))
local itemsTab = build.itemsTab
local prevRing = itemsTab.slots["Ring 1"].selItemId
try("addItemText", { text = RING, slotName = "Ring 1", prev = prevRing }, function()
  itemsTab:SetDisplayItem()
  itemsTab:CreateDisplayItemFromRaw(sanitiseText(RING), true)
  local item = itemsTab.displayItem
  itemsTab:AddDisplayItem(true)
  itemsTab.slots["Ring 1"]:SetSelItemId(item.id)
  itemsTab:PopulateSlots()
  recalc()
end, function() itemsTab.slots["Ring 1"]:SetSelItemId(prevRing); itemsTab:PopulateSlots(); recalc() end)
P.mutations = muts
table.sort(tm)
P.t.mutationRecalc = tm[math.floor(#tm / 2) + 1] or 0
return P
`
// Rust build code -> LuaJIT base64 decode (hex) ; then LuaJIT loads the Rust XML
const CODE_PROBE = String.raw`
local bytes = common.base64.decode((CODE:gsub("-", "+"):gsub("_", "/")))
return { hex = (bytes:gsub(".", function(c) return string.format("%02x", c:byte()) end)) }
`
const LOAD_RUST_PROBE = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function N(v) if v ~= v then return "nan" end return RAWF("%.17g", v) end
local function S(v) if v == "nan" or v == "inf" or v == "-inf" or v:find("^%-?[%d%.]+[eE]?[%+%-]?%d*$") then return "$str:" .. v end return v end
loadBuildFromXML(XML, NAME)
if __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end
wipeGlobalCache(); build.calcsTab:BuildOutput()
local o = setmetatable({}, OBJ)
for k, v in pairs(build.calcsTab.mainOutput) do local tv = type(v) if tv == "number" then o[k] = N(v) elseif tv == "string" then o[k] = S(v) elseif tv == "boolean" then o[k] = v end end
return o
`

function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
type Scenario = { slug: string; name: string; xml: Buffer; kind: 'golden' | 'synth' }
function scenarios(filter?: string): Scenario[] {
  const s: Scenario[] = GOLDEN_BUILDS.map((b: Rec) => ({ slug: b.name.replace(/ /g, '-'), name: b.name, xml: readFileSync(b.xmlPath(roots)), kind: 'golden' as const }))
  for (const n of SYNTH_SET) s.push({ slug: n, name: n, xml: readFileSync(resolve(SYNTH, `${n}.xml`)), kind: 'synth' })
  return filter ? s.filter((x) => x.slug.includes(filter)) : s
}
async function pool<T, R>(xs: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(xs.length)
  let next = 0
  await Promise.all(Array.from({ length: JOBS }, async () => { for (;;) { const i = next++; if (i >= xs.length) return; res[i] = await f(xs[i]!); console.error(`  done ${i + 1}/${xs.length}`) } }))
  return res
}
const chunk = (s: Scenario, body: string, extra = ''): string => `local XML = ${luaBytes(s.xml)}\nlocal NAME = ${JSON.stringify(s.name)}\n${extra}local function probe()\n${body}\nend\nreturn probe()\n`

async function main(filter?: string): Promise<void> {
  LUTS = await prepareLuts()
  const list = scenarios(filter)
  const passes: Rec[][] = []
  for (const p of [1, 2]) { console.error(`pass ${p} (${list.length} scenarios)`); passes.push(await pool(list, (s) => ljEval(chunk(s, PROBE)))) }
  const producer = { generator: 'tools/lua-corpus/export-engine.ts', image: LUAJIT_IMAGE, note: 'LuaJIT docker (Alpine/musl); boot.lua engine_* logic copied; numbers %.17g, numeric-looking strings $str:' }
  const files = new Map<string, string>()
  const perf: Rec = {}
  const differ: string[] = []
  list.forEach((s, i) => {
    const [a, b] = [passes[0]![i]!, passes[1]![i]!]
    const { t: ta, ...ra } = a
    const { t: tb, ...rb } = b
    if (ser(ra) !== ser(rb)) {
      differ.push(s.slug)
      mkdirSync(SCRATCH, { recursive: true })
      for (const [k, v] of Object.entries(ra)) if (ser(v) !== ser((rb as Rec)[k])) { writeFileSync(resolve(SCRATCH, `engine-dbg-a-${s.slug}-${k}.json`), JSON.stringify(v, null, 1)); writeFileSync(resolve(SCRATCH, `engine-dbg-b-${s.slug}-${k}.json`), JSON.stringify((rb as Rec)[k], null, 1)) }
    }
    perf[s.slug] = { load: [ta.load, tb.load], recalc: [ta.recalc, tb.recalc], mutationRecalc: [ta.mutationRecalc, tb.mutationRecalc] }
    files.set(`${s.slug}.engine.json`, ser({ format: 1, producer, build: s.name, kind: s.kind, xmlSha256: sha(s.xml), ...ra }) + '\n')
  })
  if (differ.length) { console.log(JSON.stringify({ STOP: 'passes differ', differ }, null, 1)); process.exitCode = 1; return }
  // optional: the Rust build codes
  const codesPath = resolve(SCRATCH, 'rust-engine-codes.json')
  let codeReport: Rec | undefined
  if (existsSync(codesPath)) {
    const codes: Rec = JSON.parse(readFileSync(codesPath, 'utf8'))
    const sel = list.filter((s) => codes[s.slug])
    const decoded = await pool(sel, (s) => ljEval(`local CODE = ${JSON.stringify(codes[s.slug].code)}\n${CODE_PROBE}`))
    const loaded = await pool(sel, (s) => ljEval(chunk({ ...s, xml: Buffer.from(codes[s.slug].xml, 'utf8') }, LOAD_RUST_PROBE)))
    codeReport = {}
    sel.forEach((s, i) => {
      const xml = inflateSync(Buffer.from(decoded[i]!.hex, 'hex')).toString('utf8')
      const base = passes[0]![list.indexOf(s)]!.output
      const out = loaded[i]!
      const keys = new Set([...Object.keys(base), ...Object.keys(out)])
      const diffKeys = [...keys].filter((k) => base[k] !== out[k])
      codeReport![s.slug] = { luajitDecodeInflateEqualsRustXml: xml === codes[s.slug].xml, luajitLoadOfRustXmlOutputDiffKeys: diffKeys.slice(0, 10), luajitLoadOfRustXmlOutputDiffCount: diffKeys.length, keys: keys.size }
    })
    files.set('rust-codes.report.json', ser({ format: 1, producer, rustCodesSha256: sha(readFileSync(codesPath)), scenarios: codeReport }) + '\n')
  }
  mkdirSync(OUT, { recursive: true })
  for (const [n, t] of files) writeFileSync(resolve(OUT, n), t, 'utf8')
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-engine.json'), JSON.stringify(perf, null, 1) + '\n')
  const med = (k: string): Rec => Object.fromEntries(Object.entries(perf).map(([s, v]: [string, any]) => [s, +median(v[k]).toFixed(2)])) // eslint-disable-line
  console.log(JSON.stringify({ scenarios: list.length, files: files.size, passesIdentical: true, codeReport, perfMs: { load: med('load'), recalc: med('recalc'), mutationRecalc: med('mutationRecalc') } }, null, 1))
}
await main(process.argv[2])
