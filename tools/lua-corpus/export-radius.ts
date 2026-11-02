// R5.5b: LuaJIT truth for the radius jewels -- PassiveTree.lua:654-711 `nodesInRadius` (sockets + keystones) and CalcSetup.lua:871-935
// `env.radiusJewelList` / `env.extraRadiusNodeList`, plus the x/y of the nodes BuildSubgraph generates (R5.5a computed them with
// f64::sin/cos and never compared them). One docker run per golden (PoB boot + loadBuildFromXML, timeless LUTs mounted as in
// export-env.ts, MINEFIELD-RUST R5), probe after the load, then a FRESH calcs.initEnv(build, "MAIN").
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-radius.ts  -> crates/pob-tree/tests/fixtures/radius-<build>.json (one per golden)
//                                             + crates/pob-tree/tests/fixtures/radius-tree-<treeVersion>.json (every socket/keystone of each tree once)
// Numbers: ids as JSON numbers, coordinates as `%.17g` strings (the raw lj_strfmt, as export-env.ts). Docker = Alpine/musl libm
// (MINEFIELD-RUST R2): a coordinate difference is checked against the desktop LuaJIT before it is called a porting bug.
import '../upstream.ts'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line

if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-tree/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const roots = resolveRoots()
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()

// --- the docker twin of export-env.ts ljEval (LUT mount + io.open/NewFileSearch shim, luajit-dump17.lua:35-59) ----------------
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
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r55b-'))
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

// --- the probe ----------------------------------------------------------------------------------------------------------------------
const PROBE = (withTree: boolean): string => String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local spec = build.spec
local tree = spec.tree
local function ids(list) local t = {} for id in pairs(list) do t[#t + 1] = assert(tonumber(id)) end table.sort(t) return t end
local function radii(n)
  if not n.nodesInRadius then return nil end
  local o = {} for i, l in ipairs(n.nodesInRadius) do o[i] = ids(l) end
  return o
end
-- every jewel the spec holds (PassiveSpec.jewels: socket id -> item id), sorted by socket id
local sockets = {}
for nodeId, itemId in pairs(spec.jewels) do
  local item = build.itemsTab.items[itemId]
  if item then
    local n = spec.nodes[nodeId]
    sockets[#sockets + 1] = { node = assert(tonumber(nodeId)), keyType = type(nodeId), item = itemId, name = item.name, radiusIndex = item.jewelRadiusIndex,
      alloc = spec.allocNodes[nodeId] ~= nil, inSpec = n ~= nil, generated = n ~= nil and getmetatable(n) == nil,
      radius = n and radii(n) or nil }
  end
end
table.sort(sockets, function(a, b) return a.node < b.node end)
-- the x/y of every node BuildSubgraph generated (plain tables, no tree metatable)
local generated = {}
for id, n in pairs(spec.nodes) do
  if getmetatable(n) == nil then
    generated[#generated + 1] = { id = assert(tonumber(id)), type = n.type, x = n.x and RAWF("%.17g", n.x), y = n.y and RAWF("%.17g", n.y),
      angle = n.angle and RAWF("%.17g", n.angle), o = n.o, oidx = n.oidx }
  end
end
table.sort(generated, function(a, b) return a.id < b.id end)
-- CalcSetup.lua:871-935 as a fresh initEnv leaves them
local env = build.calcsTab.calcs.initEnv(build, "MAIN")
local rads = {}
for i, rad in ipairs(env.radiusJewelList) do
  rads[i] = { nodeId = rad.nodeId, type = rad.type, item = rad.item.name, nodes = ids(rad.nodes) }
end
local extra = ids(env.extraRadiusNodeList)
local whole = nil
if ${withTree ? 'true' : 'false'} then
  local s = {}
  for id, n in pairs(tree.sockets) do s[#s + 1] = { id = assert(tonumber(id)), charm = n.charmSocket or false, radius = radii(n) } end
  table.sort(s, function(a, b) return a.id < b.id end)
  local k, seen = {}, {}
  for name, n in pairs(tree.keystoneMap) do
    if not seen[n] then seen[n] = true k[#k + 1] = { id = n.id, idType = type(n.id), dn = n.dn, radius = radii(n) } end
  end
  table.sort(k, function(a, b) return tostring(a.id) < tostring(b.id) end)
  local jr = {}
  local maxR = 0
  for i, r in ipairs(data.jewelRadii[(function() local ma, mi = tree.treeVersion:match("(%d+)_(%d+)") if tonumber(ma) <= 3 and tonumber(mi) <= 15 then return "3_15" end return "3_16" end)()]) do
    jr[i] = { inner = r.inner, outer = r.outer, label = r.label }
  end
  whole = { sockets = s, keystones = k, jewelRadius = jr }
end
return { jit = jit.version, treeVersion = tree.treeVersion, sockets = sockets, generated = generated, radiusJewelList = rads,
  extraRadiusNodeList = extra, tree = whole }
`

const luts = await prepareLuts()
let digest = ''
try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
const producer = { generator: 'tools/lua-corpus/export-radius.ts', image: digest || LUAJIT_IMAGE, pobSha: git(roots.pobRoot), libm: 'docker = Alpine/musl (MINEFIELD-RUST R2)' }
mkdirSync(FIX, { recursive: true })
const treesDone = new Set<string>()
const summary: Rec = {}
for (const b of GOLDEN_BUILDS) {
  const xml = readFileSync(b.xmlPath(roots))
  // the tree-wide dump once per tree version (decided after the first run of that version)
  let r = ljEval(`loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(b.name)})\n` +
    'if __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n' +
    `local function probe(withTree)\n${PROBE(false)}\nend\nreturn probe()\n`, luts)
  if (!treesDone.has(r.treeVersion)) {
    r = ljEval(`loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(b.name)})\n` +
      'if __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n' +
      `local function probe()\n${PROBE(true)}\nend\nreturn probe()\n`, luts)
    treesDone.add(r.treeVersion)
    const t = { treeVersion: r.treeVersion, producer: { ...producer, luajit: r.jit, via: b.name }, ...r.tree }
    writeFileSync(resolve(FIX, `radius-tree-${r.treeVersion}.json`), JSON.stringify(t) + '\n')
  }
  const slug = b.name.replace(/ /g, '-')
  const out = { build: b.name, xmlSha256: createHash('sha256').update(xml).digest('hex'), treeVersion: r.treeVersion, producer: { ...producer, luajit: r.jit },
    sockets: r.sockets, generated: r.generated, radiusJewelList: r.radiusJewelList, extraRadiusNodeList: r.extraRadiusNodeList }
  writeFileSync(resolve(FIX, `radius-${slug}.json`), JSON.stringify(out) + '\n')
  summary[b.name] = { tree: r.treeVersion, sockets: (r.sockets as Rec[]).length, withRadius: (r.sockets as Rec[]).filter((s) => s.radiusIndex !== undefined).length,
    generated: (r.generated as Rec[]).length, radiusJewelList: (r.radiusJewelList as Rec[]).map((x) => `${x.nodeId}:${x.type}:${x.item}:${(x.nodes as number[]).length}`),
    extra: (r.extraRadiusNodeList as number[]).length }
  console.error(`${b.name}: done`)
}
console.log(JSON.stringify(summary, null, 1))
