// LuaJIT truth for items pasted in the in-game advanced copy format (Ctrl+Alt+C: `{ <name> - <tags> }` modifier headers,
// Item.lua:584-641 and :1103-1154). Mechanics = export-buildraw.ts (docker LuaJIT, HeadlessWrapper boot).
// Per case, like the Items tab paste (ItemsTab.lua:2079-2086 CreateDisplayItemFromRaw): `new("Item"):Item(raw)`; b1 = BuildRaw();
// BuildAndParseRaw(); b2 = BuildRaw(). Two docker passes must be byte-identical.
// Usage (Node 24, repo root):  node tools/lua-corpus/export-advanced-copy.ts  -> crates/pob-items/tests/fixtures/advanced-copy.luajit.json
import '../upstream.ts'
import { execFile } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
const run = promisify(execFile)
const { REPO_ROOT, resolveRoots } = await import('../oracle/paths.ts')
const { LUAJIT_IMAGE, dockerAvailable } = await import('../oracle/luajit.ts')
type Rec = Record<string, any> // eslint-disable-line
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-items/tests/fixtures/advanced-copy.luajit.json')
const roots = resolveRoots()

const CASES: Record<string, string> = {
  // eldritch implicits under their own headers, a fractured prefix, an essence suffix, a bench craft (no affix of that name)
  'rare-eldritch-fractured': `Item Class: Gloves
Rarity: Rare
Viper Paw
Warlock Gloves
--------
Quality: +20% (augmented)
Energy Shield: 268 (augmented)
--------
Requirements:
Level: 84
Int: 144
--------
Sockets: W-W-W-W
--------
Item Level: 85
--------
{ Searing Exarch Implicit Modifier (Lesser) — Attack }
Non-Vaal Strike Skills target 1 additional nearby Enemy
{ Eater of Worlds Implicit Modifier (Lesser) }
Inflict Lightning Exposure on Hit, applying -11% to Lightning Resistance
(Lightning Exposure subtracts from Lightning Resistance for 4 seconds)
--------
{ Fractured Prefix Modifier "Seething" (Tier: 1) — Defences, Energy Shield }
+44(39-49) to maximum Energy Shield
{ Prefix Modifier "Indomitable" (Tier: 2) — Defences, Energy Shield }
91(80-91)% increased Energy Shield
{ Prefix Modifier "Sparking" (Tier: 2) — Damage, Elemental, Lightning, Attack }
Adds 3(1-3) to 33(33-34) Lightning Damage to Attacks
{ Suffix Modifier "of the Essence" — Attack, Speed }
18(17-18)% increased Attack Speed
{ Suffix Modifier "of Haast" (Tier: 1) — Elemental, Cold, Resistance }
+47(46-48)% to Cold Resistance
{ Master Crafted Suffix Modifier "of Craft" — Attack, Caster, Speed }
32(31-36)% increased Attack and Cast Speed while Focused
Searing Exarch Item
Eater of Worlds Item
--------
Fractured Item
`,
  // a base implicit under a plain implicit header, a magic item, a two-line prefix
  'magic-two-lines': `Item Class: Rings
Rarity: Magic
Vaporous Two-Stone Ring of the Lynx
--------
Requirements:
Level: 36
--------
Item Level: 84
--------
{ Implicit Modifier — Elemental, Cold, Fire, Resistance }
+14(12-16)% to Fire and Cold Resistances
--------
{ Prefix Modifier "Vaporous" (Tier: 3) — Defences, Evasion }
+20(19-24) to Evasion Rating
{ Suffix Modifier "of the Lynx" (Tier: 4) — Attribute }
+30(28-32) to Dexterity
`,
  // a unique with headers (stat order of data.itemMods.ItemExclusive, Item.lua:1372-1381)
  'unique-headers': `Item Class: Body Armours
Rarity: Unique
Doppelgänger Guise
Sadist Garb
--------
Evasion Rating: 437
Energy Shield: 95
--------
Requirements:
Level: 68
Dex: 103
Int: 109 (unmet)
--------
Sockets: W-W-W
--------
Item Level: 86
--------
{ Unique Modifier }
Grants Level 20 ​Unhinge — Unscalable Value
{ Unique Modifier — Critical }
60(40-60)% more Critical Strike Chance while Insane
{ Unique Modifier }
Enemies Killed by your Hits are destroyed while Insane
{ Unique Modifier — Physical, Chaos }
39(40-30)% less Physical and Chaos Damage Taken while Sane
{ Unique Modifier — Life }
Regenerate 10% Life over one second when Hit while Sane
--------
Within each of us exists a being of pure malice,
held back only by the lies we tell ourselves.
`,
}

function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}

const names = Object.keys(CASES)
const CODE = String.raw`
local cases = { ${names.map((n) => luaBytes(Buffer.from(CASES[n], 'utf8'))).join(',\n')} }
local res = {}
for i, raw in ipairs(cases) do
  local item = new("Item"):Item(raw)
  local r = { hasBase = item.base ~= nil, implicits = #item.implicitModLines, explicits = #item.explicitModLines }
  r.b1 = item:BuildRaw()
  local ok, e = pcall(item.BuildAndParseRaw, item)
  if ok then r.b2 = item:BuildRaw() else r.err = tostring(e) end
  res[i] = r
end
return res
`

async function ljEval(code: string): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-advcopy-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    try { const { stdout } = await run('docker', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 }); return JSON.parse(stdout.trim().split('\n').pop()!).result } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}

const a = JSON.stringify(await ljEval(CODE))
const b = JSON.stringify(await ljEval(CODE))
if (a !== b) { writeFileSync(resolve(ROOT, "tools/.scratch/advcopy-a.json"), a); writeFileSync(resolve(ROOT, "tools/.scratch/advcopy-b.json"), b); throw new Error("two LuaJIT passes differ -- STOP") }
const results = JSON.parse(a) as Rec[]
const out = names.map((name, i) => ({ name, raw: CASES[name], ...results[i] }))
writeFileSync(OUT, JSON.stringify({ cases: out }, null, 1) + '\n')
console.log(`advanced copy: ${out.length} cases -> ${OUT}`)
for (const c of out) console.log(`${c.name}: implicits ${c.implicits}, explicits ${c.explicits}${c.err ? ', err ' + c.err : ''}`)
