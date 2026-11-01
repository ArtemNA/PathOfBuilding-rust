// R8.7: LuaJIT truth for node power on the 6 goldens (R8-ARCH §1 p.7, gate (г)): after loadBuildFromXML (BuildOutput already made
// `calcsTab.miscCalculator`), for each power stat -- the default `powerStat = nil` (Offence/Defence, CalcsTab.lua:547) and `FullDPS`
// (data.powerStatList, the singleStat branch :538) -- run `build.calcsTab:PowerBuilder()` directly (no coroutine: `coroutine.running()`
// is nil in the main thread, so it never yields; GetTime is the headless stub) and dump, per spec node: `node.power` (singleStat, pathPower,
// offence, defence, distance, masteryEffects), the cluster notables' power, `powerMax`, `TreeTab:BuildPowerReportList(stat)`; once per
// build: the tree paths (`pathDist`, `path` ids in order, `depends` ids), `pairs` orders of `spec.nodes` / `spec.allocNodes`, the
// `main.*` number-format settings; perf: `os.clock` CPU ms of the whole PowerBuilder and the number of calcFunc calls.
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-power.ts run [golden name ...]  -> tools/.scratch/power-raw/<build>.json (one docker run per golden)
//   node tools/lua-corpus/export-power.ts pack                   -> crates/pob-calc/tests/fixtures/power/<build>.json (<= 1.5 MB)
// Encoding: numbers = `%.17g` strings ("nan"/"-nan"/"inf" as LuaJIT prints them), node ids = numbers, strings = strings.
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line

if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures/power')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const RAW = resolve(SCRATCH, 'power-raw')
const MAX_BYTES = 1_500_000
const roots = resolveRoots()
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const slugOf = (name: string): string => name.replace(/ /g, '-')

// --- docker twin of export-env.ts ljEval (LUT mount + io.open/NewFileSearch shim, MINEFIELD-RUST R5) -----------------------------------------
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
async function prepareLuts(): Promise<string> {
  const dir = resolve(SCRATCH, 'luts')
  mkdirSync(dir, { recursive: true })
  const { findRequiredJewelLuts } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/timelessJewelLuts.mjs')).href)
  const { unpackJewelData } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/nodeJewelData.mjs')).href)
  for (const b of GOLDEN_BUILDS) for (const { fileStem } of findRequiredJewelLuts(readFileSync(b.xmlPath(roots), 'utf8'))) writeFileSync(resolve(dir, `${fileStem}.bin`), unpackJewelData(roots.pobRoot, fileStem).bytes)
  return dir
}
function ljEval(code: string, lutsDir: string, name: string): Rec {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r87-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '--name', `pob-r87-${name.replace(/[^A-Za-z0-9]/g, '')}`, '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`,
      '-v', `${lutsDir}:/jewel-data:ro`, '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE,
      'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = execFileSync('docker', args, { encoding: 'utf8', maxBuffer: 1024 * 1024 * 1024 }) } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
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

// --- Lua probe ----------------------------------------------------------------------------------------------------------------------------
const PROBE = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local ARR = { __jsontype = "array" }
local function obj() return setmetatable({}, OBJ) end
local function arr() return setmetatable({}, ARR) end
local function N(v) if v == nil then return nil end if v ~= v then return RAWF("%.17g", v) end return RAWF("%.17g", v) end
local spec, ct, tt = build.spec, build.calcsTab, build.treeTab
local res = obj()
res.jit = jit and jit.version or "?"
res.jitStatus = jit and jit.status() or false
res.main = { showThousandsSeparators = main.showThousandsSeparators and true or false, thousandsSeparator = tostring(main.thousandsSeparator), decimalSeparator = tostring(main.decimalSeparator) }
res.viewMode = tostring(build.viewMode)
res.nodePowerMaxDepth = ct.nodePowerMaxDepth and N(ct.nodePowerMaxDepth) or "nil"
local o = arr() for id in pairs(spec.nodes) do o[#o + 1] = id end res.nodesPairsOrder = o
o = arr() for id in pairs(spec.allocNodes) do o[#o + 1] = id end res.allocPairsOrder = o
o = arr() for id in pairs(ct.mainEnv.grantedPassives) do o[#o + 1] = id end table.sort(o) res.grantedPassives = o
o = arr() for name in pairs(spec.tree.clusterNodeMap) do o[#o + 1] = name end table.sort(o) res.clusterNodeNames = o
-- the tree paths (stat-independent)
local paths = obj()
for id, node in pairs(spec.nodes) do
  local e = obj()
  e.pathDist = N(node.pathDist)
  if node.path then local p = arr() for i, pn in ipairs(node.path) do p[i] = pn.id end e.path = p end
  if node.depends then local d = arr() for i, dn in ipairs(node.depends) do d[i] = dn.id end e.depends = d end
  e.alloc = node.alloc and true or false
  e.type = node.type
  e.modKeyEmpty = node.modKey == ""
  e.allMasteryOptions = node.allMasteryOptions and true or false
  e.ill = #(node.intuitiveLeapLikesAffecting or {})
  if node.distanceToClassStart then e.distanceToClassStart = N(node.distanceToClassStart) end
  paths[tostring(id)] = e
end
res.paths = paths
-- the base output the power is measured against
local calcBase = ct.miscCalculator[2]
res.base = { CombinedDPS = N(calcBase.CombinedDPS), FullDPS = N(calcBase.FullDPS), LifeUnreserved = N(calcBase.LifeUnreserved), Armour = N(calcBase.Armour), Evasion = N(calcBase.Evasion) }
local origFunc = ct.miscCalculator[1]
local calls = 0
ct.miscCalculator = { function(ov, u) calls = calls + 1 return origFunc(ov, u) end, calcBase }
local function dumpPower(p)
  local e = obj()
  for _, k in ipairs({ "singleStat", "pathPower", "offence", "defence", "distance" }) do if p[k] ~= nil then e[k] = N(p[k]) end end
  if p.masteryEffects then
    local m = obj()
    for eid, ep in pairs(p.masteryEffects) do
      local x = obj()
      for _, k in ipairs({ "singleStat", "pathPower", "offence", "defence" }) do if ep[k] ~= nil then x[k] = N(ep[k]) end end
      m[tostring(eid)] = x
    end
    e.masteryEffects = m
  end
  return e
end
local stats = obj()
for _, which in ipairs({ "OffDef", "FullDPS" }) do
  local statEntry = nil
  if which == "FullDPS" then for _, s in ipairs(data.powerStatList) do if s.stat == "FullDPS" then statEntry = s end end end
  ct.powerStat = statEntry
  calls = 0
  collectgarbage("collect")
  local t0 = os.clock()
  ct:PowerBuilder()
  local ms = (os.clock() - t0) * 1000
  local s = obj()
  s.ms, s.calcCalls = ms, calls
  local power = obj()
  for id, node in pairs(spec.nodes) do if node.power and next(node.power) ~= nil then power[tostring(id)] = dumpPower(node.power) end end
  s.power = power
  local cl = obj()
  for name, node in pairs(spec.tree.clusterNodeMap) do if node.power and next(node.power) ~= nil then local e = dumpPower(node.power) e.id = node.id cl[name] = e end end
  s.cluster = cl
  local pm = obj() for k, v in pairs(ct.powerMax) do pm[k] = N(v) end s.powerMax = pm
  -- the displayStats entry BuildPowerReportList formats with (TreeTab.lua:1072-1086), nil = its { fmt = ".1f" } fallback
  local ds = nil
  local cur = statEntry or data.powerStatList[1]
  for _, d in ipairs(build.displayStats) do if cur.stat and d.stat == cur.stat then ds = d break end end
  if ds then s.displayStat = { fmt = ds.fmt, pc = ds.pc and true or false, mod = ds.mod and true or false, lowerIsBetter = ds.lowerIsBetter and true or false } end
  local report = tt:BuildPowerReportList(statEntry or data.powerStatList[1])
  local r = arr()
  for i, e in ipairs(report) do
    r[i] = { id = e.id, name = e.name, power = N(e.power), powerStr = e.powerStr, pathPower = N(e.pathPower), pathPowerStr = e.pathPowerStr, allocated = e.allocated and true or false,
      type = e.type, pathDist = type(e.pathDist) == "number" and N(e.pathDist) or e.pathDist }
  end
  s.report = r
  stats[which] = s
end
res.stats = stats
return res
`

async function run(names: string[]): Promise<void> {
  const luts = await prepareLuts()
  mkdirSync(RAW, { recursive: true })
  const builds = names.length ? GOLDEN_BUILDS.filter((b) => names.includes(b.name)) : GOLDEN_BUILDS
  for (const b of builds) {
    const xml = readFileSync(b.xmlPath(roots))
    const t0 = Date.now()
    const r = ljEval(loadChunk(xml, b.name) + `local function probe()\n${PROBE}\nend\nreturn probe()\n`, luts, b.name)
    r.build = b.name
    r.xmlSha256 = sha(xml)
    writeFileSync(resolve(RAW, `${slugOf(b.name)}.json`), JSON.stringify(r) + '\n')
    console.error(`${b.name}: OffDef ${Number(r.stats.OffDef.ms).toFixed(0)} ms / ${r.stats.OffDef.calcCalls} calls, FullDPS ${Number(r.stats.FullDPS.ms).toFixed(0)} ms / ${r.stats.FullDPS.calcCalls} calls (wall ${((Date.now() - t0) / 1000).toFixed(0)} s)`)
  }
}

function ser(v: unknown): string {
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort().map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}

function pack(): void {
  mkdirSync(FIX, { recursive: true })
  let digest = ''
  try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
  const producer = { generator: 'tools/lua-corpus/export-power.ts', image: digest || LUAJIT_IMAGE, pobSha: git(roots.pobRoot), dataManifestSha256: sha(readFileSync(resolve(ROOT, 'data/manifest.json'))),
    libm: 'docker = Alpine/musl (MINEFIELD-RUST R2)' }
  const perf: Rec = {}
  for (const b of GOLDEN_BUILDS) {
    const f = resolve(RAW, `${slugOf(b.name)}.json`)
    if (!existsSync(f)) throw new Error(`${f} missing -- run 'run' first`)
    const r = JSON.parse(readFileSync(f, 'utf8'))
    const { stats, ...rest } = r
    const out: Rec = { format: 1, producer: { ...producer, jit: r.jit }, ...rest, stats: {} }
    for (const [k, s] of Object.entries(stats as Rec)) out.stats[k] = s
    const text = ser(out) + '\n'
    if (Buffer.byteLength(text) > MAX_BYTES) throw new Error(`${b.name}: ${Buffer.byteLength(text)} bytes > ${MAX_BYTES} -- STOP`)
    const file = resolve(FIX, `${slugOf(b.name)}.json`)
    writeFileSync(file, text, 'utf8')
    perf[b.name] = { bytes: statSync(file).size, OffDef_ms: Number(stats.OffDef.ms).toFixed(0), OffDef_calls: stats.OffDef.calcCalls, FullDPS_ms: Number(stats.FullDPS.ms).toFixed(0), FullDPS_calls: stats.FullDPS.calcCalls }
  }
  console.log(JSON.stringify(perf, null, 1))
  void readdirSync
}

if (process.argv[2] === 'run') await run(process.argv.slice(3))
else if (process.argv[2] === 'pack') pack()
else throw new Error('usage: export-power.ts run [golden ...] | pack')
