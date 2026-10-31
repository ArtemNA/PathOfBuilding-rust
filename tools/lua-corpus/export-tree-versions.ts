// R6-fix: data patch `data-patches/tree-versions.json` -- `treeVersionList` and `treeVersions[ver].{display,num}` of GameVersions.lua taken
// from LuaJIT (docker, PoB boot). pob-ts converts the tree data but not this table (its `TREE_VERSION_NUM` knows 4 versions by hand); the
// `num` decides migration branches (`>= 3.10`, PassiveTree.lua / PassiveSpec.lua), so it is data, never a parse of the version name.
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-tree-versions.ts   -> data-patches/tree-versions.json (deterministic)
// Numbers travel as `%.17g` strings (the raw lj_strfmt) and are written as the shortest JS repr (exact round trip).
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'data-patches/tree-versions.json')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const roots = resolveRoots()

const LUA = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local list, vers, extra = {}, {}, {}
for i, ver in ipairs(treeVersionList) do
  list[i] = ver
  local t = treeVersions[ver]
  vers[i] = { ver = ver, display = t.display, num = RAWF("%.17g", t.num) }
end
local n = 0
for ver in pairs(treeVersions) do n = n + 1 end
return { jit = jit.version, list = list, vers = vers, nVersions = n, latest = latestTreeVersion, default = defaultTreeVersion }
`

type Ver = { ver: string; display: string; num: string }
const res = luajitEval<{ jit: string; list: string[]; vers: Ver[]; nVersions: number; latest: string; default: string }>(LUA, { roots })
if (res.nVersions !== res.list.length) throw new Error(`treeVersions has ${res.nVersions} entries, treeVersionList ${res.list.length}`)
const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8')) as { manifestSha256: string; pobSha: string }
const manifest = JSON.parse(readFileSync(resolve(ROOT, 'data/manifest.json'), 'utf8')) as { treeVersionList: string[] }
if (JSON.stringify(manifest.treeVersionList) !== JSON.stringify(res.list)) throw new Error('data/manifest.json treeVersionList differs from LuaJIT')
const producer = { tool: 'tools/lua-corpus/export-tree-versions.ts', luajit: res.jit, pobSha: lock.pobSha, source: 'GameVersions.lua' }
const canonVer = (v: Ver): string => {
  const num = Number(v.num)
  if (!Number.isFinite(num)) throw new Error(`${v.ver}: num ${v.num}`)
  return `{"display":${JSON.stringify(v.display)},"num":${JSON.stringify(num)},"ver":${JSON.stringify(v.ver)}}`
}
const text =
  `{"format":1,"producer":{"luajit":${JSON.stringify(producer.luajit)},"pobSha":${JSON.stringify(producer.pobSha)},` +
  `"source":${JSON.stringify(producer.source)},"tool":${JSON.stringify(producer.tool)}},` +
  `"manifestSha256":"${lock.manifestSha256}","pobSha":"${lock.pobSha}",` +
  `"latestTreeVersion":${JSON.stringify(res.latest)},"defaultTreeVersion":${JSON.stringify(res.default)},\n"treeVersions":[\n` +
  res.vers.map(canonVer).join(',\n') +
  '\n]\n}\n'
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, text, 'utf8')
console.log(`LuaJIT ${res.jit}: ${res.list.length} tree versions, latest ${res.latest}, default ${res.default}`)
console.log(`sha256 ${createHash('sha256').update(text).digest('hex')}  ${OUT}  (${Buffer.byteLength(text)} bytes)`)
