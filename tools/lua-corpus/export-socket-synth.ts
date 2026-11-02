// R-tree-legacy: LuaJIT truth for a socket slot holding an item it does not accept, or one that does not exist (ItemSlotControl.lua:105-106 ->
// SetSelItemId(0) :72-79, spec.jewels[nodeId] = 0 and BuildClusterJewelGraphs, on TreeTab:Load's SetActiveSpec and on TreeTab:PostLoad), and for a
// build without any item (TreeTab.lua:573 `itemOrderList[1]`). Scenarios: edits of the public golden `Dual Wield Cospris CoC` (upstream
// spec/TestBuilds/3.13). Each is run twice (the slot order of PopulateSlots is per process); differing passes are an error.
// Run from the pob-rust root (Node 24, docker):  node tools/lua-corpus/export-socket-synth.ts
//   -> crates/pob-engine/tests/fixtures/socket-synth/<scenario>.{xml,luajit.json}; gate crates/pob-engine/tests/socket_synth.rs
import { POB } from '../upstream.ts'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { luajitEval } = await import(`${P}/tools/oracle/luajit-eval.ts`)
const { dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
const { resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-engine/tests/fixtures/socket-synth')
const base = readFileSync(`${POB}/spec/TestBuilds/3.13/Dual Wield Cospris CoC.xml`, 'latin1').replace(/\r\n/g, '\n')
const socket = (xml: string, node: number, item: number): string => {
  const re = new RegExp(`<Socket nodeId="${node}" itemId="\\d+"/>`)
  if (!re.test(xml)) throw new Error(`no socket ${node}`)
  return xml.replace(re, `<Socket nodeId="${node}" itemId="${item}"/>`)
}
// An ALLOCATED socket pointing at a missing item is not a scenario: LuaJIT raises at PassiveSpec.lua:1587-1588 in BuildAllDependsAndPaths
// (`items[itemId] and (...) or (items[itemId].jewelData ...)` indexes nil) before any Populate -- such a build does not load (R-tree-legacy report).
const noSockets = (xml: string): string => xml.replace(/<Socket nodeId="(\d+)" itemId="\d+"\/>/g, '<Socket nodeId="$1" itemId="0"/>')
const SCENARIOS: Record<string, string> = {
  // an unallocated plain socket pointing at an item that does not exist
  'missing-unalloc': socket(base, 2491, 9999),
  // a Large Cluster Jewel moved from its socket into a plain one: `item.clusterJewel and not node.expansionJewel` (ItemsTab.lua:2634)
  'cluster-in-plain': socket(socket(base, 21984, 0), 33989, 2),
  // a Large Cluster Jewel in a Medium socket (ItemsTab.lua:2642)
  'large-in-medium': socket(base, 64583, 8),
  // no item at all: no Populate before PostLoad (TreeTab.lua:573); the held id goes at TreeTab:PostLoad (:532)
  'no-items': socket(noSockets(base).replace(/<Item [\s\S]*?<\/Item>\s*/g, ''), 2491, 5),
}
const luaBytes = (s: string): string => {
  let o = '"'
  for (const b of Buffer.from(s, 'latin1')) o += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return o + '"'
}
const chunk = (xml: string): string => String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
loadBuildFromXML(${luaBytes(xml)}, "synth")
if __mainObject__.promptMsg then error(tostring(__mainObject__.promptMsg)) end
local t = { elem = "Tree" }
build.treeTab:Save(t)
local text = require("xml").ComposeXML(t)
local jewels = {}
for k, v in pairs(build.spec.jewels) do if v ~= 0 then jewels[#jewels + 1] = RAWF("%d=%d", k, v) end end
table.sort(jewels)
wipeGlobalCache()
local env = build.calcsTab.calcs.buildOutput(build, "MAIN")
local nums, strs, bools = {}, {}, {}
for k, v in pairs(env.player.output) do
  if type(v) == "number" then nums[#nums + 1] = k .. "=" .. RAWF("%.17g", v)
  elseif type(v) == "string" then strs[#strs + 1] = k .. "=" .. v
  elseif type(v) == "boolean" then bools[#bools + 1] = k .. "=" .. tostring(v) end
end
table.sort(nums) table.sort(strs) table.sort(bools)
local n = 0 for _ in pairs(build.spec.allocNodes) do n = n + 1 end
return { tree = text, jewels = jewels, allocNodes = n, nums = nums, strs = strs, bools = bools }
`
mkdirSync(OUT, { recursive: true })
const roots = resolveRoots()
for (const [name, xml] of Object.entries(SCENARIOS)) {
  const a = luajitEval(chunk(xml), { roots }) as Record<string, unknown>
  const b = luajitEval(chunk(xml), { roots }) as Record<string, unknown>
  if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${name}: the two LuaJIT passes differ`)
  writeFileSync(resolve(OUT, `${name}.xml`), xml, 'latin1')
  writeFileSync(resolve(OUT, `${name}.luajit.json`), JSON.stringify(a) + '\n')
  console.log(`${name}: jewels ${JSON.stringify(a.jewels)}, allocNodes ${a.allocNodes}, ${(a.nums as string[]).length} numbers`)
}
