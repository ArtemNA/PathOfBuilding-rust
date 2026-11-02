// R-tree-legacy: the shape of every passive tree as desktop LuaJIT builds it (`main:LoadTree(ver)` for each version of GameVersions.lua
// `treeVersionList`) -- the truth for crates/pob-tree/tests/tree_versions.rs: node / group / socket counts and the node-id sum after the
// legacy alternate ascendancies are dropped (PassiveTree.lua:132-176), the alternate ascendancies left (`key:id:startNodeId`) and
// `secondaryAscendNameMap`'s ids (:208-225).
// Run from the pob-rust root (Node 24, docker):  node tools/lua-corpus/export-tree-shape.ts  -> crates/pob-tree/tests/fixtures/tree-shape.json
import '../upstream.ts'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-tree/tests/fixtures/tree-shape.json')
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')

const LUA = String.raw`
local out = {}
for _, ver in ipairs(treeVersionList) do
  local t0 = os.clock()
  local tree = main:LoadTree(ver)
  local ms = (os.clock() - t0) * 1000
  local nodes, sum, legacyLeft = 0, 0, 0
  for _, node in pairs(tree.nodes) do
    nodes = nodes + 1
    sum = sum + node.id
    if node.ascendancyName == "Warden" or node.ascendancyName == "Warlock" or node.ascendancyName == "Primalist" then legacyLeft = legacyLeft + 1 end
  end
  local groups = 0
  for _ in pairs(tree.groups) do groups = groups + 1 end
  local sockets = {}
  for id in pairs(tree.sockets) do sockets[#sockets + 1] = id end
  table.sort(sockets)
  local alt
  if tree.alternate_ascendancies then
    alt = {}
    for k, a in pairs(tree.alternate_ascendancies) do alt[#alt + 1] = string.format("%d:%s:%s", k, tostring(a.id), tostring(a.startNodeId)) end
    table.sort(alt)
  end
  local sec
  if tree.secondaryAscendNameMap then
    sec = {}
    for id in pairs(tree.secondaryAscendNameMap) do sec[#sec + 1] = id end
    table.sort(sec)
  end
  out[#out + 1] = { ver = ver, nodes = nodes, nodeIdSum = sum, legacyLeft = legacyLeft, groups = groups, sockets = sockets,
    alternateAscendancies = alt or "nil", secondaryAscendNames = sec or "nil", ms = ms }
  main.tree[ver] = nil
  collectgarbage()
end
return { jit = jit.version, trees = out }
`

type Shape = { ver: string; nodes: number; nodeIdSum: number; legacyLeft: number; groups: number; sockets: number[]; alternateAscendancies: string[] | 'nil'; secondaryAscendNames: string[] | 'nil'; ms: number }
const res = luajitEval<{ jit: string; trees: Shape[] }>(LUA, { roots: resolveRoots() })
const lock = JSON.parse(readFileSync(resolve(ROOT, 'data.lock'), 'utf8')) as { pobSha: string }
// dkjson writes an empty Lua table as [] -- a tree without sockets would read as an empty list either way
const arr = (v: unknown): unknown => (v === 'nil' ? null : v)
const lines = res.trees.map((t) =>
  JSON.stringify({ ver: t.ver, nodes: t.nodes, nodeIdSum: t.nodeIdSum, legacyLeft: t.legacyLeft, groups: t.groups, sockets: t.sockets,
    alternateAscendancies: arr(t.alternateAscendancies), secondaryAscendNames: arr(t.secondaryAscendNames) }))
const text = `{"producer":{"tool":"tools/lua-corpus/export-tree-shape.ts","luajit":${JSON.stringify(res.jit)},"pobSha":${JSON.stringify(lock.pobSha)}},\n"trees":[\n${lines.join(',\n')}\n]}\n`
mkdirSync(dirname(OUT), { recursive: true })
writeFileSync(OUT, text, 'utf8')
const ms = res.trees.map((t) => t.ms).sort((a, b) => a - b)
console.log(`LuaJIT ${res.jit}: ${res.trees.length} trees -> ${OUT}`)
console.log(`main:LoadTree ms: median ${ms[Math.floor(ms.length / 2)]!.toFixed(0)}, min ${ms[0]!.toFixed(0)}, max ${ms[ms.length - 1]!.toFixed(0)}; 3_29 ${res.trees.find((t) => t.ver === '3_29')?.ms.toFixed(0)}`)
