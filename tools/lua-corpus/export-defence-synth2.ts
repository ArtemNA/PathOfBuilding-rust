// R4-synth-2: synthetic LuaJIT corpus for the CalcPerform.lua branches the 6 goldens never execute (reservations: BloodMagic/*Forced/cost-as-
// reservation/mines/Blood Sacrament/GrantReserved*AsAura; Shock/ScorchCanStack, Bonechill; several guards; Choir of the Storm; tincture limit ...).
// Each scenario = Dual Savior.xml with <Config> replaced (inputs + a <CustomModifierBlock>), optional extra gear items and extra <Skill> groups.
// Truth = desktop LuaJIT (docker): loadBuildFromXML -> (1) build.calcsTab.mainOutput scalars (%.17g), (2) a FRESH calcs.initEnv(build,"MAIN") + perform with
// CP1..CP4 (as export-skills.ts R6.1: wrappers of doActorLifeMana / doActorLifeManaReservation / defenceForConditionals / defence).
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-defence-synth2.ts            -> crates/pob-calc/tests/fixtures/defence-synth2/<scenario>.{xml,luajit.json}; 2 runs, must be identical
//   node tools/lua-corpus/export-defence-synth2.ts coverage   -> tools/.scratch/defence-synth2-coverage.json + table (debug.sethook line counter, CalcPerform.lua)
//   node tools/lua-corpus/export-defence-synth2.ts gaps       -> CalcPerform.lua :1286-3790 executable lines no golden executes (authoring aid)
//   node tools/lua-corpus/export-defence-synth2.ts parse [f]  -> which mod lines modLib.parseMod accepts (authoring aid)
import { POB as UPSTREAM_POB, ORACLE as UPSTREAM_ORACLE } from '../upstream.ts'
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
const run = promisify(execFile)
type Rec = Record<string, any> // eslint-disable-line
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures/defence-synth2')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const POB = UPSTREAM_POB
const POBTS = 'C:/pob-work/pob-ts'
const ORACLE = UPSTREAM_ORACLE
const IMAGE = 'ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest'
const MAX_BYTES = 1_500_000
const BASE_XML = `${POB}/spec/TestBuilds/3.13/Dual Savior.xml`
const PERFORM_LUA = `${POB}/src/Modules/CalcPerform.lua`
const BASE = 'Dual Savior'
const CONCURRENCY = 4
const RANGE: [number, number] = [1286, 3790]
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')

// --- scenarios --------------------------------------------------------------------------------------------------
// inputs: extra config <Input>s; mods: lines of the custom block; gear: normal-rarity bases into empty ItemSet slots; skills: extra socket groups
// (see grp); tree: treeVersion override; covers: substrings of CalcPerform.lua lines the scenario must execute (every needle must match >= 1 executed line).
interface Scenario { name: string; inputs?: Record<string, number | boolean | string>; mods?: string[]; gear?: Record<string, string>; skills?: string[]; tree?: string; covers: string[] }
// gem name -> [gemId, skillId, attrs] ; grp(['Hatred', 'Blood Magic Support'], {slot}) -> <Skill> block
const GEMS: Record<string, [string, string]> = Object.fromEntries(([
  ['Anger', 'Anger'], ['Arc', 'Arc'], ['Arcane Cloak', 'ArcaneCloak'], ['Clarity', 'Clarity'], ['Determination', 'Determination'], ['Discipline', 'Discipline'], ['Enfeeble', 'Enfeeble'],
  ['Frost Bomb', 'FrostBomb'], ['Frostbolt', 'FrostBolt'], ['Grace', 'Grace'], ['Hatred', 'Hatred'], ['Herald of Ash', 'HeraldOfAsh'], ['Herald of Ice', 'HeraldOfIce'], ['Herald of Thunder', 'HeraldOfThunder'],
  ['Ice Nova', 'IceNova'], ['Immortal Call', 'ImmortalCall'], ['Lightning Arrow', 'LightningArrow'], ['Molten Shell', 'MoltenShell'], ['Purity of Elements', 'PurityOfElements'],
  ['Righteous Fire', 'RighteousFire'], ['Cold Snap', 'ColdSnap'], ['Arctic Armour', 'ArcticArmour'], ['Split Arrow', 'SplitArrow'], ['Temporal Chains', 'TemporalChains'], ['Vaal Grace', 'VaalGrace'], ['Vaal Molten Shell', 'VaalMoltenShell'], ['Vitality', 'Vitality'], ['Wrath', 'Wrath'],
] as [string, string][]).map(([n, id]) => [n, [`Metadata/Items/Gems/SkillGem${id}`, id]])) // gem name -> [gemId, skillId] (`gems` mode)
Object.assign(GEMS, { Steelskin: ['Metadata/Items/Gems/SkillGemQuickGuard', 'QuickGuard'], Blasphemy: ['Metadata/Items/Gems/SkillGemSupportBlasphemy', 'SupportBlasphemy'], Bonechill: ['Metadata/Items/Gems/SkillGemSupportBonechill', 'SupportBonechill'],
  'Divine Blessing': ['Metadata/Items/Gems/SkillGemSupportDivineBlessing', 'SupportDivineBlessing'], Enlighten: ['Metadata/Items/Gems/SkillGemSupportEnlighten', 'SupportEnlighten'], Generosity: ['Metadata/Items/Gems/SkillGemSupportGenerosity', 'SupportGenerosity'],
  'Locus Mine': ['Metadata/Items/Gems/SkillGemSupportLocusMine', 'SupportLocusMine'], 'High-Impact Mine': ['Metadata/Items/Gems/SkillGemSupportHighImpactMine', 'SupportHighImpactMine'],
  'Blastchain Mine': ['Metadata/Items/Gems/SkillGemSupportBlastchainMine', 'SupportBlastchainMine'] })
const grp = (gems: string[], attrs = ''): string => `<Skill mainActiveSkillCalcs="1" label="" enabled="true" mainActiveSkill="1"${attrs}>${gems.map((g) => {
  const [n, extra = ''] = g.split('|'); const [id, sk] = GEMS[n!] ?? (() => { throw new Error(`unknown gem ${n}`) })()
  return `<Gem level="20" gemId="${id}" skillId="${sk}" enableGlobal1="true" qualityId="Default" quality="0" enabled="true" nameSpec="${n}"${extra ? ' ' + extra : ''}/>`
}).join('')}</Skill>`
const AURAS = (): string[] => [grp(['Hatred']), grp(['Wrath']), grp(['Vitality'])]
const CHILL = { conditionEnemyChilled: true, conditionEnemyChilledEffect: 30 }
const SCENARIOS: Scenario[] = [ // SCENARIOS-BEGIN
  // --- reservations (CalcPerform.lua :1967-2092 and the delayed GrantReserved*AsAura / Choir of the Storm) ---
  { name: 'res-auras-baseline', skills: AURAS(), covers: ['values.reservedPercent = m_max(round(modifiedPercent', 'env.player["reserved_"..name.."Percent"] = env.player["reserved_"..name.."Percent"] + values.reservedPercent'] },
  { name: 'res-blood-magic', skills: AURAS(), mods: ['Skills reserve Life instead of Mana'], covers: ['pool.Life.baseFlat = pool.Life.baseFlat + pool.Mana.baseFlat', 'activeSkill.skillData["LifeReservationPercentForced"] = '] },
  { name: 'res-blood-magic-curse-aura', skills: [grp(['Hatred']), grp(['Enfeeble', 'Blasphemy'])], mods: ['Curse Aura Skills reserve Life instead of Mana'], covers: ['pool.Life.basePercent = pool.Life.basePercent + pool.Mana.basePercent'] },
  { name: 'res-forced-herald', skills: [grp(['Herald of Ash']), grp(['Herald of Ice']), grp(['Hatred'])], mods: ['Mana Reservation of Herald Skills is always 25%'], covers: ['values.reservedPercent = activeSkill.skillData[name.."ReservationPercentForced"]'] },
  { name: 'res-forced-blood-magic', skills: [grp(['Herald of Ash']), grp(['Hatred'])], mods: ['Mana Reservation of Herald Skills is always 25%', 'Skills reserve Life instead of Mana'], covers: ['values.reservedPercent = activeSkill.skillData[name.."ReservationPercentForced"]', 'pool.Life.baseFlat = pool.Life.baseFlat + pool.Mana.baseFlat'] },
  { name: 'res-high-impact-mine', skills: [grp(['Arc|skillMineCount="3"', 'High-Impact Mine']), grp(['Hatred'])], covers: ['pool.Mana.baseFlat = skillModList:Sum("BASE", skillCfg, "ManaCostBase")', 'values.reservedFlat = values.reservedFlat * activeSkill.activeMineCount'] },
  { name: 'res-locus-mine-bow', gear: { 'Weapon 1': 'Rarity: NORMAL\nShort Bow' }, skills: [grp(['Lightning Arrow|skillMineCount="2"', 'Locus Mine']), grp(['Wrath'])], covers: ['pool.Mana.baseFlat = skillModList:Sum("BASE", skillCfg, "ManaCostBase")', 'values.reservedPercent = values.reservedPercent * activeSkill.activeMineCount'] },
  { name: 'res-locus-mine-bow-quiver', gear: { 'Weapon 1': 'Rarity: NORMAL\nShort Bow', 'Weapon 2': 'Rarity: NORMAL\nRugged Quiver' }, skills: [grp(['Lightning Arrow|skillMineCount="2"', 'Locus Mine']), grp(['Wrath'])], covers: ['pool.Mana.baseFlat = skillModList:Sum("BASE", skillCfg, "ManaCostBase")', 'values.reservedPercent = values.reservedPercent * activeSkill.activeMineCount'] },
  { name: 'res-blastchain-mine-blood-magic', skills: [grp(['Frostbolt|skillMineCount="2"', 'Blastchain Mine']), grp(['Wrath'])], mods: ['Your Skills that throw Mines reserve Life instead of Mana'], covers: ['pool.Life.baseFlat = skillModList:Sum("BASE", skillCfg, "LifeCostBase")', 'pool.Life.baseFlat = pool.Life.baseFlat + pool.Mana.baseFlat'] },
  { name: 'res-blood-sacrament', inputs: { bloodSacramentReservationEHP: true }, gear: { 'Gloves': 'Rarity: RARE\nTest Gloves\nIron Gauntlets\nImplicits: 0\nGrants Level 1 Blood Sacrament Skill' }, skills: [grp(['Hatred'])], covers: ['"HasUncancellableReservation"', 'values.reservedPercent = values.reservedPercent * (activeSkill.activeStageCount + 1)'] },
  { name: 'res-grant-reserved-life', skills: AURAS(), mods: ['Grants Armour equal to 30% of your Reserved Life to you and nearby Allies'], covers: ['"GrantReserved"..pool.."AsAura"'] },
  { name: 'res-grant-reserved-mana', skills: AURAS(), mods: ['Grants Armour equal to 40% of your Reserved Mana to you and nearby Allies', 'Grants Maximum Energy Shield equal to 20% of your Reserved Mana to you and nearby Allies'], covers: ['"GrantReserved"..pool.."AsAura"'] },
  { name: 'res-extra-life-reserved', skills: AURAS(), mods: ['Reserves 30% of Life'], covers: ['env.player.uncancellable_LifeReservation = modDB:Sum("BASE", nil, "ExtraLifeReserved")'] },
  { name: 'res-choir-of-the-storm', skills: [grp(['Hatred']), grp(['Vitality'])], mods: ['Mana is increased by 30% of Overcapped Lightning Resistance', '+60% to Lightning Resistance'], covers: ['tempResistActor.modDB.actor = tempResistActor', 'doActorLifeManaReservation(env.player, true)'] },
  { name: 'res-choir-grant-aura', skills: [grp(['Hatred']), grp(['Vitality'])], mods: ['Mana is increased by 30% of Overcapped Lightning Resistance', '+60% to Lightning Resistance', 'Grants Armour equal to 40% of your Reserved Mana to you and nearby Allies'], covers: ['doActorLifeMana(env.player)', '"GrantReserved"..pool.."AsAura"'] },
  { name: 'res-divine-blessing', skills: [grp(['Hatred', 'Divine Blessing']), grp(['Wrath'])], covers: ['not activeSkill.skillTypes[SkillType.ReservationBecomesCost]'] },
  // --- ailments: stacking shock/scorch, Bonechill, chill-effect damage-taken flags, Elemental Equilibrium ---
  { name: 'shock-can-stack', inputs: { conditionEnemyShocked: true, conditionShockEffect: 25, ShockStacks: 3 }, mods: ['You can apply up to 3 Shocks to each Enemy'], covers: ['output["CurrentShock"] = num * m_min('] },
  { name: 'shock-can-stack-over-max', inputs: { conditionEnemyShocked: true, conditionShockEffect: 20, ShockStacks: 7 }, mods: ['You can apply up to 4 Shocks to each Enemy'], covers: ['output["CurrentShock"] = num * m_min(', 'enemyDB:NewMod("Multiplier:ShockEffect"'] },
  { name: 'scorch-can-stack', inputs: { conditionEnemyScorched: true, conditionScorchedEffect: 15, ScorchStacks: 2 }, mods: ['You can inflict an additional Scorch on each Enemy'], covers: ['output["CurrentScorch"] = num * m_min('] },
  { name: 'shock-and-scorch-stack', inputs: { conditionEnemyShocked: true, conditionShockEffect: 30, ShockStacks: 2, conditionEnemyScorched: true, conditionScorchedEffect: 20, ScorchStacks: 3 }, mods: ['You can apply up to 5 Shocks to each Enemy', 'You can inflict an additional Scorch on each Enemy'], covers: ['output["CurrentShock"] = num * m_min(', 'output["CurrentScorch"] = num * m_min('] },
  { name: 'bonechill-chilling-area', skills: [grp(['Cold Snap', 'Bonechill'])], covers: ['hasGuaranteedBonechill = true', 'output.HasBonechill = true'] },
  { name: 'bonechill-config-chill', inputs: CHILL, skills: [grp(['Frostbolt', 'Bonechill'])], covers: ['output.HasBonechill = true', 't_insert(mods, modLib.createMod("ColdDamageTaken", "INC", num, "Bonechill"'] },
  { name: 'chill-effect-inc-damage-taken', inputs: CHILL, mods: ['Enemies Chilled by your Hits have Damage taken increased by Chill Effect'], covers: ['"Ahuana\'s Bite"'] },
  { name: 'chill-effect-inc-cold-damage-taken', inputs: CHILL, mods: ['Enemies Chilled by your Hits have Cold Damage taken increased by Chill Effect'], covers: ['"Chilled by Hits"'] },
  { name: 'chill-effect-less-damage-dealt', inputs: CHILL, mods: ['Enemies Chilled by your Hits lessen their Damage dealt by half of Chill Effect'], covers: ['"Shaper of Winter"'] },
  { name: 'ee-3-13-tree', inputs: { enemyConditionHitByFireDamage: true }, mods: ['Elemental Equilibrium', 'Nearby Enemies have -20% to Fire Resistance'], covers: ['tonumber(major) <= 3 and tonumber(minor) <= 15'] },
  { name: 'ee-3-29-tree-fire-hit', tree: '3_29', inputs: { enemyConditionHitByFireDamage: true }, mods: ['Elemental Equilibrium'], covers: ['element == "Fire" and not enemyDB:Flag(nil, "Condition:HitByFireDamage")', 'element == "Cold" and not enemyDB:Flag(nil, "Condition:HitByColdDamage")'] },
  { name: 'ee-3-29-tree-no-hit', tree: '3_29', mods: ['Elemental Equilibrium', 'Nearby Enemies have -20% to Fire Resistance'], covers: ['element == "Lightning" and not enemyDB:Flag(nil, "Condition:HitByLightningDamage")'] },
  // the 3.16 tree is in pob-ts data but not loadable by Rust initEnv (`unknown tree version '3_16'`): kept as the gate of that gap
  { name: 'ee-3-16-tree', tree: '3_16', inputs: { enemyConditionHitByFireDamage: true }, mods: ['Elemental Equilibrium'], covers: ['element == "Fire" and not enemyDB:Flag(nil, "Condition:HitByFireDamage")'] },
  // --- guards (pairs(guards), :3320) ---
  { name: 'guard-single-steelskin', skills: [grp(['Steelskin'])], covers: ['elseif not nonVaal then'] },
  { name: 'guard-vaal-molten-shell-wipes', skills: [grp(['Steelskin']), grp(['Molten Shell']), grp(['Vaal Molten Shell'])], covers: ['wipeTable(guardSlots)'] },
  { name: 'guard-molten-shell-single', skills: [grp(['Molten Shell'])], covers: ['modDB.conditions["AffectedByNonVaalGuardSkill"] = true'] },
  { name: 'guard-arcane-cloak-reserved', skills: [grp(['Hatred']), grp(['Arcane Cloak'])], covers: ['mergeBuff(guard.modList, buffs, guard.name)'] },
  // --- tinctures over the limit ---
  // two DIFFERENT tinctures over the limit are not a corpus entry: pairs(tinctures) is keyed by item tables, LuaJIT picks one per process (run1 != run2, D13); two identical ones are deterministic
  { name: 'tincture-over-limit', gear: { 'Flask 1': 'Rarity: NORMAL\nPrismatic Tincture\nImplicits: 1\n70% increased Elemental Damage with Melee Weapons', 'Flask 2': 'Rarity: NORMAL\nPrismatic Tincture\nImplicits: 1\n70% increased Elemental Damage with Melee Weapons' }, covers: ['if tinctureLimit <= 0 then'] },
  { name: 'tincture-extra-slot', gear: { 'Flask 1': 'Rarity: NORMAL\nPrismatic Tincture\nImplicits: 1\n70% increased Elemental Damage with Melee Weapons', 'Flask 2': 'Rarity: NORMAL\nRosethorn Tincture\nImplicits: 1\n100% increased Critical Strike Chance with Melee Weapons' }, mods: ['You can have an additional Tincture active'], covers: ['tinctureConditions["UsingTincture"] = true'] },
  { name: 'tincture-melee-to-ranged', gear: { 'Flask 1': 'Rarity: NORMAL\nPrismatic Tincture\nImplicits: 1\n70% increased Elemental Damage with Melee Weapons' }, mods: ['Tincture Effects also apply to Ranged Weapons', 'Tinctures applied to you have 50% increased Effect'], covers: ['newMod.flags = bor(band(newMod.flags, bnot(ModFlag.WeaponMelee)), ModFlag.WeaponRanged)'] },
] // SCENARIOS-END

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
async function ljEval(code: string): Promise<Rec> { // twin of pob-ts luajitEval; the LUT dir (Lethal Pride) is mounted
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r4s2-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const luts = resolve(SCRATCH, 'luts'); mkdirSync(luts, { recursive: true })
    const args = ['run', '--rm', '-v', `${POB}:/workdir:ro`, '-v', `${POBTS}/tools/oracle/lua:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${luts}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = (await run('docker', args, { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024 })).stdout } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
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

// --- XML of a scenario: Dual Savior with <Config> replaced, items/skills added ---
const baseXml = readFileSync(BASE_XML)
const BASE_INPUT = { conditionLeeching: true } as Record<string, number | boolean | string> // Dual Savior.xml:707-709
const xmlEsc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/"/g, '&quot;')
function scenarioXml(sc: Scenario): Buffer {
  const text = baseXml.toString('latin1')
  const m = /\t<Config>[\s\S]*?<\/Config>\r?\n/.exec(text)
  if (!m) throw new Error('no <Config> in the base build')
  const nl = text.includes('\r\n') ? '\r\n' : '\n'
  const inputs = { ...BASE_INPUT, ...(sc.inputs ?? {}) }
  const lines = ['\t<Config>']
  for (const k of Object.keys(inputs).sort(byteOrder)) {
    const v = inputs[k]!
    lines.push(`\t\t<Input name="${k}" ${typeof v === 'number' ? 'number' : typeof v === 'boolean' ? 'boolean' : 'string'}="${xmlEsc(String(v))}"/>`)
  }
  if (sc.mods?.length) lines.push(`\t\t<CustomModifierBlock title="synth" enabled="true">${xmlEsc(sc.mods.join('\n'))}</CustomModifierBlock>`)
  lines.push('\t</Config>')
  let out = text.slice(0, m.index) + lines.join(nl) + nl + text.slice(m.index + m[0].length)
  const slots = Object.keys(sc.gear ?? {})
  if (slots.length) {
    const items = slots.map((slot, i) => `		<Item id="${100 + i}">${nl}			${xmlEsc(sc.gear![slot]!.split('\n').join(nl))}${nl}		</Item>${nl}`).join('')
    const anchor = out.indexOf('		<Slot name="Weapon 1" itemId="1"/>')
    if (anchor < 0) throw new Error('no top-level <Slot name="Weapon 1"> anchor')
    out = out.slice(0, anchor) + items + out.slice(anchor)
    slots.forEach((slot, i) => {
      const re = new RegExp(`<Slot name="${slot}" itemId="[0-9]+"/>`, 'g') // the ItemSet slot (and the top-level one of Weapon 1/2); flasks/tinctures need active="true"
      if (!re.test(out)) throw new Error(`no ItemSet slot ${slot}`)
      out = out.replace(re, `<Slot name="${slot}" itemId="${100 + i}"${slot.startsWith('Flask') ? ' active="true"' : ''}/>`)
    })
  }
  if (sc.skills?.length) out = out.replace('\t</Skills>', sc.skills.map((s) => `\t\t${s}${nl}`).join('') + '\t</Skills>')
  if (sc.tree) out = out.replace('treeVersion="3_13"', `treeVersion="${sc.tree}"`)
  return Buffer.from(out, 'latin1')
}

// --- Lua probes ---
// the checkpoint wrapper of export-skills.ts R6.1 (CP1/CP2 = after the first top-level call, CP3/CP4 = entry; perform stopped at CP4)
const WRAP = String.raw`
local calcs = build.calcsTab.calcs
local STOP = {}
local function runPerform(env, onCp)
  local cnt, log = { lm = 0, lmr = 0, dfc = 0, def = 0 }, {}
  local oLM, oLMR, oDFC, oDEF = doActorLifeMana, doActorLifeManaReservation, calcs.defenceForConditionals, calcs.defence
  doActorLifeMana = function(actor, ...)
    local r = oLM(actor, ...)
    if actor == env.player then cnt.lm = cnt.lm + 1; if cnt.lm == 1 then onCp("CP1") end end
    return r
  end
  doActorLifeManaReservation = function(actor, ...)
    local r = oLMR(actor, ...)
    if actor == env.player then cnt.lmr = cnt.lmr + 1; if cnt.lmr == 1 then onCp("CP2") end end
    return r
  end
  calcs.defenceForConditionals = function(e, actor, ...)
    if e == env and actor == env.player then cnt.dfc = cnt.dfc + 1; if cnt.dfc == 1 then onCp("CP3") end end
    return oDFC(e, actor, ...)
  end
  calcs.defence = function(e, actor, ...)
    if e == env and actor == env.player then cnt.def = cnt.def + 1; if cnt.def == 1 then onCp("CP4") error(STOP, 0) end end
    return oDEF(e, actor, ...)
  end
  local ok, err = pcall(calcs.perform, env)
  doActorLifeMana, doActorLifeManaReservation, calcs.defenceForConditionals, calcs.defence = oLM, oLMR, oDFC, oDEF
  if not ok and err ~= STOP then error(err, 0) end
end
`
const DUMP = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local function N(v) return RAWF("%.17g", v) end
local fmtMod = modLib.formatSourceMod
local function className(t) local mt = getmetatable(t) if type(mt) == "table" then return rawget(mt, "_className") end end
local function isMod(t) return rawget(t, "name") ~= nil and rawget(t, "type") ~= nil and rawget(t, "flags") ~= nil and rawget(t, "keywordFlags") ~= nil end
-- ModDB bucket order depends on pairs() over string keys, per LuaJIT process (D13/R13): mod lists are sorted multisets
local function dbSet(db) local t = {} for _, bucket in pairs(db.mods) do for _, m in ipairs(bucket) do t[#t + 1] = fmtMod(m) end end return t end
local D
D = function(v, depth, seen)
  local tv = type(v)
  if tv == "number" then return N(v) end
  if tv == "string" or tv == "boolean" then return v end
  if tv ~= "table" then return "<" .. tv .. ">" end
  if isMod(v) then return fmtMod(v) end
  local cn = className(v)
  if cn then return "<class " .. cn .. ">" end
  if depth > 5 or seen[v] then return "<deep>" end
  seen[v] = true
  local o = obj()
  for k, x in pairs(v) do if type(k) ~= "table" and k ~= "actor" and k ~= "minion" then o[tostring(k)] = D(x, depth + 1, seen) end end
  seen[v] = nil
  return o
end
local function snapActor(a)
  local o, n = obj(), 0
  for _, bucket in pairs(a.modDB.mods) do n = n + #bucket end
  o.modDB, o.modCount, o.conditions, o.multipliers, o.output = dbSet(a.modDB), N(n), D(a.modDB.conditions, 1, {}), D(a.modDB.multipliers, 1, {}), D(a.output, 1, {})
  return o
end
local out = obj()
local mo = build.calcsTab.mainOutput
local nums, strs, bools = obj(), obj(), obj()
for k, v in pairs(mo) do
  local t = type(v)
  if t == "number" then nums[k] = N(v) elseif t == "string" then strs[k] = v elseif t == "boolean" then bools[k] = v end
end
out.output = { nums = nums, strs = strs, bools = bools }
wipeGlobalCache()
local env = calcs.initEnv(build, "MAIN")
out.checkpoints = obj()
runPerform(env, function(name) local s = obj() s.player, s.enemy = snapActor(env.player), snapActor(env.enemy) out.checkpoints[name] = s end)
return out
`
// executable lines of CalcPerform.lua from the bytecode of every nested prototype (jit.util), + the lines hit while ONE calcs.buildOutput(build,"MAIN") runs
const COVER_PRE = String.raw`
jit.off()
jit.flush()
local util = require("jit.util")
local executable = {}
local function walk(p)
  local fi = util.funcinfo(p)
  for pc = 1, fi.bytecodes do local l = util.funcinfo(p, pc).currentline; if l and l > 0 then executable[l] = true end end
  for n = -1, -(fi.gcconsts or 0), -1 do
    local k = util.funck(p, n)
    if type(k) == "proto" then walk(k) end
  end
end
walk(assert(loadfile("Modules/CalcPerform.lua")))
`
const COVER_POST = String.raw`
local hits, isDef = {}, {}
local function hook(_, line)
  local f = debug.getinfo(2, "f").func
  local d = isDef[f]
  if d == nil then d = debug.getinfo(2, "S").source:find("CalcPerform.lua", 1, true) ~= nil; isDef[f] = d end
  if d then hits[line] = true end
end
debug.sethook(hook, "l")
build.calcsTab.calcs.buildOutput(build, "MAIN")
debug.sethook()
local ex, hh = {}, {}
for l in pairs(executable) do ex[#ex + 1] = l end
for l in pairs(hits) do hh[#hh + 1] = l end
table.sort(ex); table.sort(hh)
return { executable = ex, hits = hh }
`

function ser(v: unknown): string {
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
const normAddr = (s: string): string => s.replace(/(function|table): 0x[0-9a-f]+/g, '$1: <addr>')
function norm(v: any): any { // eslint-disable-line
  if (typeof v === 'string') return normAddr(v)
  if (Array.isArray(v)) return v.map(norm)
  if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [normAddr(k), norm(x)]))
  return v
}

// --- covers: grep -n of CalcPerform.lua against the coverage cache ---
const performLines = readFileSync(PERFORM_LUA, 'latin1').split('\n')
const COVER_CACHE = resolve(SCRATCH, 'defence-synth2-coverage.json')
const GOLDENS = ['Dual Savior', 'Dual Wield Cospris CoC', 'Dual Wield Cospris CoC - Lethal Pride', 'Generals Perforate Zerker', 'Mirage Archer Toxic Rain', 'OccVortex']
const goldenXml = (g: string): Buffer => readFileSync(g === 'Dual Wield Cospris CoC - Lethal Pride' ? `${ORACLE}/test/fixtures/${g}.xml` : `${POB}/spec/TestBuilds/3.13/${g}.xml`)
const inRange = (l: number): boolean => l >= RANGE[0] && l <= RANGE[1]
const loadCache = (): Rec => { if (!existsSync(COVER_CACHE)) throw new Error('no coverage cache: run `coverage` first'); return JSON.parse(readFileSync(COVER_CACHE, 'utf8')) }
function coverInfo(sc: Scenario, xml: Buffer, cache: Rec): { covers: string[]; newLines: string; newLineCount: number } {
  const e = cache.entries[sc.name]
  if (!e || e.sha !== sha(xml)) throw new Error(`${sc.name}: coverage cache is stale: re-run \`coverage\``)
  const hit = new Set<number>(e.hits), executable = new Set<number>(cache.executable)
  const gold = new Set<number>(GOLDENS.flatMap((g) => cache.entries[g]?.hits ?? (() => { throw new Error(`no cached golden ${g}`) })()))
  const covers = sc.covers.map((needle) => {
    const all = performLines.flatMap((l, i) => (l.includes(needle) && executable.has(i + 1) ? [i + 1] : []))
    if (!all.length) throw new Error(`${sc.name}: cover needle matches no CalcPerform.lua line: ${needle}`)
    const lines = all.filter((n) => hit.has(n))
    if (!lines.length) throw new Error(`${sc.name}: cover needle ${needle} matches lines ${all.join(',')} but the scenario executes none of them`)
    return `CalcPerform.lua:${lines.join(',')} ${needle}`
  })
  const fresh = [...hit].filter((n) => executable.has(n) && !gold.has(n) && inRange(n)).sort((x, y) => x - y)
  const ranges: string[] = []
  for (let i = 0; i < fresh.length;) { let j = i; while (j + 1 < fresh.length && fresh[j + 1]! === fresh[j]! + 1) j++; ranges.push(j > i ? `${fresh[i]}-${fresh[j]}` : `${fresh[i]}`); i = j + 1 }
  return { covers, newLines: ranges.length ? `CalcPerform.lua:${ranges.join(',')}` : '', newLineCount: fresh.length }
}
async function pool<T, R>(items: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(items.length)
  let next = 0
  await Promise.all(Array.from({ length: CONCURRENCY }, async () => {
    for (;;) { const i = next++; if (i >= items.length) return; res[i] = await f(items[i]!); console.error(`  done ${i + 1}/${items.length}`) }
  }))
  return res
}
async function evalScenario(sc: Scenario): Promise<Rec> {
  const r = norm(await ljEval(loadChunk(scenarioXml(sc), BASE) + `local function probe()\n${WRAP}\n${DUMP}\nend\nreturn probe()\n`))
  // R13: the order of a ModDB bucket comes from pairs() over string keys (per LuaJIT process) -> the mod lists are multisets: sorted here
  for (const cp of Object.values(r.checkpoints as Rec)) for (const who of ['player', 'enemy']) cp[who].modDB = [...(cp[who].modDB as string[])].sort(byteOrder)
  return r
}

async function fixtures(): Promise<void> {
  mkdirSync(FIX, { recursive: true })
  const cache = loadCache()
  for (const s of SCENARIOS) coverInfo(s, scenarioXml(s), cache) // validate every needle BEFORE spending docker runs
  const image = (await run('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', IMAGE]).catch(() => ({ stdout: IMAGE }))).stdout.trim()
  const pobSha = (await run('git', ['-C', POB, 'rev-parse', '--short', 'HEAD'])).stdout.trim()
  if (new Set(SCENARIOS.map((s) => s.name)).size !== SCENARIOS.length) throw new Error('duplicate scenario names')
  const runs: string[][] = []
  for (let r = 0; r < 2; r++) {
    console.error(`run ${r + 1}/2`)
    const outs = await pool(SCENARIOS, evalScenario)
    runs.push(outs.map((o, i) => {
      const sc = SCENARIOS[i]!, xml = scenarioXml(sc), ci = coverInfo(sc, xml, cache)
      const text = `${ser({ build: BASE, scenario: sc.name, covers: ci.covers, newLines: ci.newLines, newLineCount: ci.newLineCount, gear: sc.gear ?? {}, inputs: sc.inputs ?? {}, mods: sc.mods ?? [], skills: sc.skills ?? [], xmlSha256: sha(xml),
        output: o.output, checkpoints: o.checkpoints,
        producer: { image, luajit: 'desktop LuaJIT docker, loadBuildFromXML -> mainOutput; fresh initEnv + perform with CP1..CP4 (export-skills.ts R6.1)', pobSha, generator: 'tools/lua-corpus/export-defence-synth2.ts' } })}\n`
      if (Buffer.byteLength(text) > MAX_BYTES) throw new Error(`${sc.name}: ${Buffer.byteLength(text)} bytes > 1.5 MB`)
      if (r === 0) { writeFileSync(resolve(FIX, `${sc.name}.xml`), xml); writeFileSync(resolve(FIX, `${sc.name}.luajit.json`), text) }
      return sha(text) + ' ' + sha(xml)
    }))
  }
  const same = runs[0]!.every((h, i) => h === runs[1]![i])
  console.log(`scenarios ${SCENARIOS.length}; run1 == run2 byte-identical: ${same}`)
  SCENARIOS.forEach((s, i) => console.log(`${s.name}\t${runs[0]![i]!.slice(0, 16)}\t${runs[1]![i]!.slice(0, 16)}\t${coverInfo(s, scenarioXml(s), cache).covers.join(' | ')}`))
  if (!same) process.exitCode = 1
}

async function coverage(): Promise<void> {
  mkdirSync(SCRATCH, { recursive: true })
  const jobs = [...GOLDENS.map((g) => ({ label: g, xml: goldenXml(g), name: g })), ...SCENARIOS.map((s) => ({ label: s.name, xml: scenarioXml(s), name: BASE }))]
  const cache: Rec = existsSync(COVER_CACHE) ? JSON.parse(readFileSync(COVER_CACHE, 'utf8')) : { entries: {} }
  const todo = jobs.filter((j) => cache.entries[j.label]?.sha !== sha(j.xml))
  console.error(`coverage: ${jobs.length} jobs, ${todo.length} to run (the rest cached by xml sha256)`)
  const fresh = await pool(todo, (j) => ljEval(COVER_PRE + loadChunk(j.xml, j.name) + COVER_POST))
  todo.forEach((j, i) => { cache.entries[j.label] = { sha: sha(j.xml), hits: fresh[i]!.hits }; if (!cache.executable) cache.executable = fresh[i]!.executable })
  writeFileSync(COVER_CACHE, JSON.stringify(cache) + '\n')
  const executable = new Set<number>(cache.executable as number[])
  const count = (labels: string[]): number => new Set(labels.flatMap((l) => cache.entries[l].hits as number[]).filter((l) => executable.has(l) && inRange(l))).size
  const synth = SCENARIOS.map((s) => s.name)
  console.log(`CalcPerform.lua :${RANGE[0]}-${RANGE[1]}: executable ${[...executable].filter(inRange).length}; goldens ${count(GOLDENS)}; goldens+synth ${count([...GOLDENS, ...synth])}`)
  const gold = new Set<number>(GOLDENS.flatMap((g) => cache.entries[g].hits as number[]))
  for (const [i, s] of SCENARIOS.entries()) {
    const hit = new Set<number>(cache.entries[s.name].hits)
    const miss = s.covers.filter((n) => !performLines.some((l, k) => l.includes(n) && hit.has(k + 1)))
    console.log(`${i + 1}\t${s.name}\tnew-vs-goldens ${[...hit].filter((l) => executable.has(l) && inRange(l) && !gold.has(l)).length}${miss.length ? '\tNOT HIT: ' + miss.join(' ; ') : ''}`)
  }
}
function gaps(): void {
  const cache = loadCache()
  const executable = new Set<number>(cache.executable as number[])
  const hit = new Set<number>(GOLDENS.flatMap((g) => cache.entries[g].hits as number[]))
  const synth = new Set<number>(SCENARIOS.flatMap((s) => (cache.entries[s.name]?.hits ?? []) as number[]))
  for (let l = RANGE[0]; l <= RANGE[1]; l++) if (executable.has(l) && !hit.has(l) && (process.argv[3] !== 'open' || !synth.has(l))) console.log(`${l}: ${performLines[l - 1]!.trim().slice(0, 150)}`)
}
async function parse(file?: string): Promise<void> {
  const lines = (file ? readFileSync(file, 'utf8') : [...new Set(SCENARIOS.flatMap((s) => s.mods ?? []))].join('\n')).split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'))
  const lua = `local LINES = {${lines.map((l) => JSON.stringify(l)).join(',')}}\n` + String.raw`
local out = {}
for i, line in ipairs(LINES) do
  local mods, extra = modLib.parseMod(line)
  local d = {}
  if mods then for _, m in ipairs(mods) do d[#d + 1] = modLib.formatSourceMod(m) end end
  out[i] = (mods and not extra) and ("OK " .. table.concat(d, " ; ")) or ("FAIL extra=" .. tostring(extra))
end
return out`
  const r = (await ljEval(lua)) as unknown as string[]
  lines.forEach((l, i) => console.log(`${r[i]!.startsWith('OK') ? 'OK  ' : 'FAIL'} | ${l} | ${r[i]}`))
}

async function probe(name: string): Promise<void> { // authoring aid: active skills / tinctures of a scenario as LuaJIT sees them
  const sc = SCENARIOS.find((x) => x.name === name)!
  const lua = String.raw`local env = build.calcsTab.calcs.initEnv(build, "MAIN")
local r = {}
for i, sk in ipairs(env.player.activeSkillList) do
  local sup = {} for _, e in ipairs(sk.effectList) do sup[#sup + 1] = e.grantedEffect.id end
  r[#r + 1] = i .. " " .. sk.activeEffect.grantedEffect.id .. " disable=" .. tostring(sk.skillFlags.disable) .. " why=" .. tostring(sk.disableReason) .. " mine=" .. tostring(sk.skillFlags.mine) .. " mineCount=" .. tostring(sk.activeMineCount) .. " stage=" .. tostring(sk.activeStageCount) .. " hasRes=" .. tostring(sk.skillTypes[SkillType.HasReservation]) .. " group=" .. table.concat(sup, ",")
end
local n = 0 for it in pairs(env.tinctures) do n = n + 1 r[#r + 1] = "tincture " .. it.baseName .. " buff=" .. #it.buffModList .. " mods=" .. #it.modList for _, m in ipairs(it.buffModList) do r[#r + 1] = "  B " .. modLib.formatSourceMod(m) .. " flags=" .. m.flags end for _, m in ipairs(it.modList) do r[#r + 1] = "  M " .. modLib.formatSourceMod(m) .. " flags=" .. m.flags end end
r[#r + 1] = "tinctureLimit=" .. tostring(env.modDB:Sum("BASE", nil, "TinctureLimit")) .. " combat=" .. tostring(env.mode_combat)
return r`
  console.log(((await ljEval(loadChunk(scenarioXml(sc), BASE) + lua)) as unknown as string[]).join(String.fromCharCode(10)))
}
async function det(name: string): Promise<void> { // authoring aid: two runs of one scenario, diff of the flattened JSON
  const sc = SCENARIOS.find((x) => x.name === name)!
  const [a, b] = await Promise.all([evalScenario(sc), evalScenario(sc)])
  const flat = (v: any, pre: string, out: Map<string, string>): Map<string, string> => { // eslint-disable-line
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) for (const k of Object.keys(v)) flat(v[k], pre + '/' + k, out)
    else out.set(pre, JSON.stringify(v))
    return out
  }
  const fa = flat(a, '', new Map()), fb = flat(b, '', new Map())
  let n = 0
  for (const k of new Set([...fa.keys(), ...fb.keys()])) if (fa.get(k) !== fb.get(k) && n++ < 40) console.log(k, '|', (fa.get(k) ?? '').slice(0, 300), '|', (fb.get(k) ?? '').slice(0, 300))
  console.log('differing leaves:', n)
}
async function gems(names: string[]): Promise<void> { // authoring aid: gem name -> [gemId, grantedEffectId]
  const lua = `local N = {${names.map((n) => '[' + JSON.stringify(n) + ']=true').join(',')}}\nlocal r = {}\nfor id, g in pairs(data.gems) do if N[g.name] then r[#r+1] = g.name .. "\\t" .. id .. "\\t" .. tostring(g.grantedEffectId) end end\ntable.sort(r)\nreturn r`
  console.log(((await ljEval(lua)) as unknown as string[]).join('\n'))
}
if (process.argv[2] === 'det') await det(process.argv[3]!)
else if (process.argv[2] === 'probe') await probe(process.argv[3]!)
else if (process.argv[2] === 'gems') await gems(process.argv.slice(3))
else if (process.argv[2] === 'coverage') await coverage()
else if (process.argv[2] === 'gaps') gaps()
else if (process.argv[2] === 'parse') await parse(process.argv[3])
else await fixtures()
