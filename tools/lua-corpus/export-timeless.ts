// R5.5c: LuaJIT truth for the timeless jewels (MINEFIELD W6 / MINEFIELD-RUST R5 -- every probe through the docker twin of ljEval with
// the LUTs mounted and the io.open/NewFileSearch shim of export-env.ts).
//   (1) LUT gate: `data.readLUT(seed, nodeId, jewelType)` for every jewel type 1-6, three seeds each (SeedMin, SeedMax and the
//       middle floor((min+max)/2) -- for Elegant Hubris the conqueror id, i.e. x20, as BuildAllDependsAndPaths passes it) x every node of
//       data.nodeIDList; plus the time of the first readLUT of each type (os.clock, docker) and the sha256 of every mounted .bin
//       (R2: the bytes the shim hands LuaJIT -- Rust inflates the same files itself and compares).
//   (2) Builds: Lethal Pride golden + the same XML with the jewel (item 3) replaced by a synthetic Glorious Vanity / Brutal Restraint /
//       Militant Faith / Elegant Hubris / Heroic Tragedy (generated here, only the item text goes into the fixture). After the load
//       (BuildAllDependsAndPaths ran with the items, Build.lua:655-677 defers the tree), every spec node in nodesInRadius[jewelRadiusIndex]
//       of socket 61834: conqueredBy (id, conqueror type/id), alloc, type, dn, sd, modList (formatSourceMod, list order), keystoneMod.
// Run from the pob-rust root (Node 24):
//   (3) D13: the `pairs` order of 4 000 integer-keyed tables filled like `additions` (PassiveSpec.lua:1302-1314) -- the corpus of
//       pob-tree's LuaJIT table model (`lj_table.rs`).
//   node tools/lua-corpus/export-timeless.ts -> crates/pob-tree/tests/fixtures/timeless-lut.json, timeless-pairs.json, timeless-builds.json
import '../upstream.ts'
import { execFileSync } from 'node:child_process'
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
const FIX = resolve(ROOT, 'crates/pob-tree/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const roots = resolveRoots()
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()

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
const STEMS = ['GloriousVanity', 'LethalPride', 'BrutalRestraint', 'MilitantFaith', 'ElegantHubris', 'HeroicTragedy']
async function prepareLuts(): Promise<{ dir: string; bins: Rec }> {
  const dir = resolve(SCRATCH, 'luts-all')
  mkdirSync(dir, { recursive: true })
  const { unpackJewelData } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/nodeJewelData.mjs')).href)
  const bins: Rec = {}
  for (const stem of STEMS) {
    const u = unpackJewelData(roots.pobRoot, stem)
    writeFileSync(resolve(dir, `${stem}.bin`), u.bytes)
    bins[stem] = { bytes: u.bytes.length, sha256: createHash('sha256').update(u.bytes).digest('hex'), parts: u.partCount, compressedSha256: u.compressedSha256 }
  }
  return { dir, bins }
}
function ljEval(code: string, lutsDir: string): Rec {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r55c-'))
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

// --- (1) the LUT gate ---------------------------------------------------------------------------------------------------------------
const LUT_PROBE = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local ids = {}
for k, v in pairs(data.nodeIDList) do if type(k) == "number" and type(v) == "table" then ids[#ids + 1] = k end end
table.sort(ids)
local out = {}
for jewelType = 1, 6 do
  local mn, mx = data.timelessJewelSeedMin[jewelType], data.timelessJewelSeedMax[jewelType]
  local seeds = { mn, mx, math.floor((mn + mx) / 2) }
  if jewelType == 5 then for i = 1, 3 do seeds[i] = seeds[i] * 20 end end
  local t0 = os.clock()
  data.readLUT(seeds[1], ids[1], jewelType)
  local firstMs = (os.clock() - t0) * 1000
  local perSeed = {}
  local t1 = os.clock()
  for si, seed in ipairs(seeds) do
    local rows = {}
    for i, id in ipairs(ids) do
      local r = data.readLUT(seed, id, jewelType)
      local parts = {}
      for j = 1, #r do parts[j] = RAWF("%d", r[j]) end
      rows[i] = table.concat(parts, ",")
    end
    perSeed[si] = { seed = seed, rows = table.concat(rows, ";") }
  end
  out[jewelType] = { name = data.timelessJewelTypes[jewelType], seedMin = mn, seedMax = mx, firstReadMs = RAWF("%.3f", firstMs),
    restMs = RAWF("%.3f", (os.clock() - t1) * 1000), calls = 3 * #ids, seeds = perSeed }
end
return { jit = jit.version, ids = ids, types = out }
`

// --- (2) the build probe ------------------------------------------------------------------------------------------------------------
const BUILD_PROBE = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local spec = build.spec
local socket = spec.nodes[61834]
local item = build.itemsTab.items[spec.jewels[61834]]
local cbItem = item.jewelData and item.jewelData.conqueredBy
local ri = item.jewelRadiusIndex
local ids = {}
for id in pairs(socket.nodesInRadius[ri]) do ids[#ids + 1] = id end
table.sort(ids)
local function fmt(list) local t = {} for i, m in ipairs(list) do t[i] = modLib.formatSourceMod(m) end return t end
local nodes = {}
for _, id in ipairs(ids) do
  local n = spec.nodes[id]
  if n then
    local cb = n.conqueredBy
    local sd = {} for i, l in ipairs(n.sd or {}) do sd[i] = l end
    nodes[#nodes + 1] = { id = id, alloc = n.alloc and true or false, type = n.type, dn = n.dn, sd = sd, mods = fmt(n.modList),
      ks = n.keystoneMod and modLib.formatSourceMod(n.keystoneMod) or nil, modKey = n.modKey,
      cb = cb and { id = RAWF("%.17g", cb.id), type = cb.conqueror.type, cid = tostring(cb.conqueror.id), same = (cb == cbItem) } or nil }
  end
end
return { jit = jit.version, item = item.name, radiusIndex = ri, seed = cbItem and RAWF("%.17g", cbItem.id), conqueror = cbItem and (cbItem.conqueror.type .. "/" .. tostring(cbItem.conqueror.id)),
  inRadius = #ids, nodes = nodes }
`
// --- (3) D13: the pairs order of `additions = {}` filled like PassiveSpec.lua:1302-1313 (integer keys, repeats update) ----------
const PAIRS_PROBE = String.raw`
local seqs, out = {}, {}
local state = 12345
local function rnd(n) state = (state * 1103515245 + 12345) % 2147483648 return state % n end
for c = 1, 4000 do
  local len = 1 + rnd(8)
  local range = (c % 4 == 0) and 400 or ((c % 4 == 1) and 40 or 130)
  local seq = {}
  for i = 1, len do seq[i] = rnd(range) end
  local additions = {}
  for i, val in ipairs(seq) do
    local roll = 1 + rnd(50)
    if not additions[val] then additions[val] = roll else additions[val] = additions[val] + roll end
  end
  local order = {}
  for add in pairs(additions) do order[#order + 1] = add end
  out[c] = table.concat(seq, ",") .. ">" .. table.concat(order, ",")
end
return { jit = jit.version, cases = out }
`
const SYNTH: { key: string; text: string }[] = [
  { key: 'glorious-vanity', text: 'Rarity: UNIQUE\nGlorious Vanity\nTimeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nRadius: Large\nImplicits: 0\nBathed in the blood of 2500 sacrificed in the name of Xibaqua\nPassives in radius are Conquered by the Vaal\nHistoric' },
  { key: 'brutal-restraint', text: 'Rarity: UNIQUE\nBrutal Restraint\nTimeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nRadius: Large\nImplicits: 0\nDenoted service of 5000 dekhara in the akhara of Asenath\nPassives in radius are Conquered by the Maraketh\nHistoric' },
  { key: 'militant-faith', text: 'Rarity: UNIQUE\nMilitant Faith\nTimeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nRadius: Large\nImplicits: 0\nCarved to glorify 6000 new faithful converted by High Templar Avarius\nPassives in radius are Conquered by the Templars\nHistoric' },
  { key: 'elegant-hubris', text: 'Rarity: UNIQUE\nElegant Hubris\nTimeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nRadius: Large\nImplicits: 0\nCommissioned 60000 coins to commemorate Cadiro\nPassives in radius are Conquered by the Eternal Empire\nHistoric' },
  { key: 'heroic-tragedy', text: 'Rarity: UNIQUE\nHeroic Tragedy\nTimeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nRadius: Large\nImplicits: 0\nRemembrancing 4000 songworthy deeds by the line of Vorana\nPassives in radius are Conquered by the Kalguur\nHistoric' },
]
/** The LP XML with item 3's text replaced (the Rust test does the same: from `<Item id="3">` to the next `</Item>`). */
function withJewel(xml: string, text: string): string {
  const open = '<Item id="3">'
  const a = xml.indexOf(open)
  const b = xml.indexOf('</Item>', a)
  if (a < 0 || b < 0) throw new Error('item 3 not found')
  return xml.slice(0, a + open.length) + '\n' + text + '\n' + xml.slice(b)
}

const { dir, bins } = await prepareLuts()
let digest = ''
try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
const producer = { generator: 'tools/lua-corpus/export-timeless.ts', image: digest || LUAJIT_IMAGE, pobSha: git(roots.pobRoot) }
mkdirSync(FIX, { recursive: true })
const write = (name: string, o: Rec): void => {
  const file = resolve(FIX, name)
  writeFileSync(file, JSON.stringify(o) + '\n')
  const size = statSync(file).size
  if (size > MAX_BYTES) throw new Error(`${name}: ${size} bytes > ${MAX_BYTES}`)
  console.log(`${name}: ${size} bytes, sha256 ${createHash('sha256').update(readFileSync(file)).digest('hex')}`)
}

const lut = ljEval(`return (function()\n${LUT_PROBE}\nend)()\n`, dir)
write('timeless-lut.json', { producer: { ...producer, luajit: lut.jit }, bins, ids: lut.ids, types: lut.types })
for (const [i, t] of Object.entries(lut.types as Rec)) console.log(`type ${i} ${t.name}: first readLUT ${t.firstReadMs} ms, ${t.calls} calls ${t.restMs} ms`)

const pairs = ljEval(`return (function()
${PAIRS_PROBE}
end)()
`, dir)
write('timeless-pairs.json', { producer: { ...producer, luajit: pairs.jit }, cases: pairs.cases })

const lp = GOLDEN_BUILDS.find((b: Rec) => b.name.includes('Lethal Pride'))
const xml = readFileSync(lp.xmlPath(roots), 'utf8')
const builds: Rec[] = []
for (const s of [{ key: 'lethal-pride', text: undefined as string | undefined }, ...SYNTH]) {
  const x = s.text === undefined ? xml : withJewel(xml, s.text)
  const r = ljEval(`loadBuildFromXML(${luaBytes(Buffer.from(x, 'utf8'))}, ${JSON.stringify(s.key)})\n` +
    'if __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n' +
    `return (function()\n${BUILD_PROBE}\nend)()\n`, dir)
  builds.push({ key: s.key, jewel: s.text, item: r.item, radiusIndex: r.radiusIndex, seed: r.seed, conqueror: r.conqueror, inRadius: r.inRadius, nodes: r.nodes })
  const conq = (r.nodes as Rec[]).filter((n) => n.cb !== undefined)
  console.log(`${s.key}: ${r.item} seed ${r.seed} ${r.conqueror}: in radius ${r.inRadius}, spec nodes ${(r.nodes as Rec[]).length}, conquered ${conq.length}`)
}
write('timeless-builds.json', { producer: { ...producer }, build: lp.name, xmlSha256: createHash('sha256').update(xml).digest('hex'), builds })
