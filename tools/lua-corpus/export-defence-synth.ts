// R4-synth: synthetic LuaJIT corpus for the CalcDefence.lua branches the 6 goldens never execute (IronReflexes, EnergyShieldToWard,
// Melding, lucky/unlucky, CannotEvade, ConvertArmourESToLife, recoup, PvP, degens, ChaosInoculation, MoM ...).
// Each scenario = Dual Savior.xml with its <Config> replaced (config <Input>s + a <CustomModifierBlock> of mod lines the real ModParser
// reads). Truth = desktop LuaJIT (docker): loadBuildFromXML -> build.calcsTab.mainOutput (= calcs.buildOutput(build,"MAIN")), scalars as %.17g.
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-defence-synth.ts             -> crates/pob-calc/tests/fixtures/defence-synth/<scenario>.{xml,luajit.json}; 2 runs, must be identical
//   node tools/lua-corpus/export-defence-synth.ts coverage    -> tools/.scratch/defence-synth-coverage.json + table (debug.sethook line counter)
//   node tools/lua-corpus/export-defence-synth.ts parse <f>   -> which lines of file <f> modLib.parseMod accepts (authoring aid)
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
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures/defence-synth')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const POB = 'C:/pob-work/project_15/PathOfBuilding'
const IMAGE = 'ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest'
const MAX_BYTES = 1_500_000
const BASE_XML = `${POB}/spec/TestBuilds/3.13/Dual Savior.xml`
const DEFENCE_LUA = `${POB}/src/Modules/CalcDefence.lua`
const ORACLE = 'C:/pob-work/project_15/pob-engine-wasm'
const BASE = 'Dual Savior'
const CONCURRENCY = 4
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)

// --- scenarios --------------------------------------------------------------------------------------------------
// inputs: extra <Input> (number -> number=, boolean -> boolean=, string -> string=); mods: lines of the custom block; covers: substrings of
// CalcDefence.lua lines this scenario is meant to execute (grep -n'd in the same run; every needle must match >= 1 line).
interface Scenario { name: string; inputs?: Record<string, number | boolean | string>; mods?: string[]; gear?: Record<string, string>; covers: string[] }
const SCENARIOS: Scenario[] = [ // SCENARIOS-BEGIN
  // --- primary defences / gear slots (gear = normal-rarity bases put into empty ItemSet slots) ---
  { name: 'iron-reflexes', gear: { 'Body Armour': 'Rarity: NORMAL\nFull Wyrmscale' }, mods: ['Converts all Evasion Rating to Armour. Dexterity provides no bonus to Evasion Rating'], covers: ['Flag(nil, "IronReflexes")', 'if ironReflexes then'] },
  { name: 'es-to-ward-gear', gear: { 'Helmet': 'Rarity: NORMAL\nRunic Helm', 'Body Armour': 'Rarity: NORMAL\nVaal Regalia' }, mods: ['Increases and Reductions to Maximum Energy Shield instead apply to Ward', '+200 to Ward', '+100 to maximum Energy Shield'], covers: ['Flag(nil, "EnergyShieldToWard")'] },
  { name: 'armour-es-to-life', gear: { 'Body Armour': 'Rarity: NORMAL\nVaal Regalia' }, mods: ['Gain maximum Life instead of maximum Energy Shield from equipped Armour Items'], covers: ['energyShieldBase = not modDB:Flag(nil, "GainNoEnergyShieldFrom" .. slot)'] },
  { name: 'body-armour-to-ward', gear: { 'Body Armour': 'Rarity: NORMAL\nFull Wyrmscale' }, mods: ['Gain Ward instead of 50% of Armour and Evasion Rating from Equipped Body Armour'], covers: ['Flag(nil, "ConvertBodyArmourArmourEvasionToWard")', 'BodyArmourArmourEvasionToWardPercent'] },
  { name: 'gain-no-from-slot', gear: { 'Body Armour': 'Rarity: NORMAL\nFull Wyrmscale', 'Helmet': 'Rarity: NORMAL\nRunic Helm' }, mods: ['Converts all Evasion Rating to Armour. Dexterity provides no bonus to Evasion Rating'], covers: ['"GainNoArmourFrom" .. slot'] },
  { name: 'life-mana-conversions', mods: ['Gain 20% of Maximum Life as Extra Armour', 'Gain 15% of Maximum Life as Extra Maximum Energy Shield', '50% of maximum Mana is converted to twice that much Armour', 'Gain 10% of Maximum Mana as Extra Maximum Energy Shield'], covers: ['"LifeGainAsArmour"', '"ManaConvertToArmour"', '"ManaGainAsEnergyShield"'] },
  { name: 'ci-with-life-conversions', mods: ['Maximum Life becomes 1, Immune to Chaos Damage', 'Gain 20% of Maximum Life as Extra Armour', 'Gain 15% of Maximum Life as Extra Maximum Energy Shield', '+500 to maximum Energy Shield'], covers: ['Flag(nil, "ChaosInoculation")'] },
  { name: 'ci-keystone-eldritch-battery', mods: ['Chaos Inoculation', 'Eldritch Battery', '+800 to maximum Energy Shield', '+400 to maximum Mana'], covers: ['Flag(nil, "ChaosInoculation")'] },
  { name: 'keystone-mind-over-matter', mods: ['Mind Over Matter', '+300 to maximum Mana', '+20 to Intelligence'], covers: ['TakenFromManaBeforeLife'] },
  { name: 'keystone-vaal-pact-ghost-reaver', mods: ['Vaal Pact', 'Ghost Reaver'], covers: ['CannotRecoverLifeOutsideLeech'] },
  { name: 'keystone-zealots-oath', mods: ["Zealot's Oath", 'Regenerate 50 Life per second'], covers: ['Flag(nil, "ZealotsOath")'] },
  { name: 'keystone-wicked-ward-pain-attunement', mods: ['Wicked Ward', 'Pain Attunement', 'Unwavering Stance', 'Glancing Blows'], covers: ['WardAvoidBreakChance'] },
  { name: 'keystone-arrow-dancing-eternal-youth', mods: ['Arrow Dancing', 'Eternal Youth', 'Necromantic Aegis'], covers: ['Flag(nil, "EnergyShieldRechargeAppliesToLife")'] },
  // --- resistances ---
  { name: 'ele-res-max-highest', mods: ['Elemental Resistances are capped by your highest Maximum Elemental Resistance instead', '+3% to maximum Fire Resistance', '+2% to maximum Cold Resistance'], covers: ['Flag(nil, "ElementalResistMaxIsHighestResistMax")'] },
  { name: 'res-conversion', mods: ['Modifiers to maximum Cold resistance also apply to maximum Fire and Lightning resistances', '+2% to maximum Cold Resistance', 'Modifiers to Cold resistance also apply to Fire and Lightning resistances at 50% of their value', '+30% to Cold Resistance'], covers: ['MaxResConvertTo', '"ResConvertTo"'] },
  { name: 'res-based-es-armour-evasion', mods: ['Armour is increased by Uncapped Fire Resistance', 'Evasion Rating is increased by Uncapped Cold Resistance', 'Maximum Energy Shield is increased by Chance to Block Spell Damage', 'Maximum Energy Shield is increased by Chaos Resistance', '+500 to maximum Energy Shield', '+30% Chance to Block Spell Damage'], covers: ['"ArmourIncreasedByUncappedFireRes"', '"EnergyShieldIncreasedByChaosResistance"'] },
  // --- block / suppression / dodge / evasion ---
  { name: 'block-lucky', mods: ['+40% Chance to Block Attack Damage', '+30% Chance to Block Spell Damage', 'Chance to Block is Lucky'], covers: ['IsLucky")'] },
  { name: 'block-unlucky-worst-of-4', inputs: { EHPUnluckyWorstOf: 4 }, mods: ['+40% Chance to Block Attack Damage', '+30% Chance to Block Spell Damage', 'Chance to Block is Unlucky'], covers: ['IsUnlucky")', 'EHPUnluckyWorstOf'] },
  { name: 'block-extreme-luck', mods: ['+40% Chance to Block Attack Damage', '+30% Chance to Block Spell Damage', 'Chance to Block is Lucky', 'Your Lucky or Unlucky effects use the best or worst from three rolls instead of two'], covers: ['Flag(nil, "ExtremeLuck")'] },
  { name: 'block-unexciting', mods: ['+40% Chance to Block Attack Damage', '+30% Chance to Block Spell Damage', 'Chance to Block is Lucky', 'Your Lucky or Unlucky effects are instead Unexciting', '+40% chance to Suppress Spell Damage', 'Chance to Suppress Spell Damage is Lucky'], covers: ['Flag(nil, "Unexciting")', 'EffectiveSpellSuppressionChance'] },
  { name: 'suppress-lucky', mods: ['+40% chance to Suppress Spell Damage', 'Chance to Suppress Spell Damage is Lucky'], covers: ['Flag(nil, "SpellSuppressionChanceIsLucky")'] },
  { name: 'suppress-lucky-extreme', mods: ['+40% chance to Suppress Spell Damage', 'Chance to Suppress Spell Damage is Lucky', 'Your Lucky or Unlucky effects use the best or worst from three rolls instead of two'], covers: ['Flag(nil, "ExtremeLuck")', 'EffectiveSpellSuppressionChance'] },
  { name: 'suppress-unlucky-worst-of-2', inputs: { EHPUnluckyWorstOf: 2 }, mods: ['+40% chance to Suppress Spell Damage', 'Chance to Suppress Spell Damage is Unlucky'], covers: ['Flag(nil, "SpellSuppressionChanceIsUnlucky")'] },
  { name: 'cannot-evade', mods: ['Cannot Evade Enemy Attacks'], covers: ['Flag(nil, "CannotEvade")'] },
  { name: 'always-evade', mods: ['Attacks cannot hit you'], covers: ['Flag(nil, "AlwaysEvade")'] },
  { name: 'evade-based-on-ward', mods: ['+300 to Ward', 'Chance to Evade Hits is based off of 50% of your Ward instead of your Evasion Rating'], covers: ['Flag(nil, "EvadeChanceBasedOnWard")'] },
  { name: 'dodge-from-suppression', mods: ['+40% chance to Suppress Spell Damage', 'Modifiers to Chance to Suppress Spell Damage instead apply to Chance to Dodge Spell Hits at 50% of their value', '+25% chance to Dodge Attack Hits', '+25% chance to Dodge Spell Hits'], covers: ['Flag(nil, "ConvertSpellSuppressionToSpellDodge")'] },
  { name: 'cannot-block', mods: ['Cannot Block', '+10% to maximum Fire Resistance'], covers: ['Flag(nil, "CannotBlockAttacks")'] },
  { name: 'max-block-if-not-blocked-recently', mods: ['You are at Maximum Chance to Block Attack Damage if you have not Blocked Recently'], covers: ['Flag(nil, "MaxBlockIfNotBlockedRecently")'] },
  // --- pools: MoM, ES, ward, prevented loss, allies ---
  { name: 'mind-over-matter', mods: ['30% of Damage is taken from Mana before Life', '30% of Physical Damage is taken from Mana before Life'], covers: ['TakenFromManaBeforeLife'] },
  { name: 'es-protects-mana', gear: { 'Body Armour': 'Rarity: NORMAL\nVaal Regalia' }, mods: ['Energy Shield protects Mana instead of Life', '30% of Damage is taken from Mana before Life'], covers: ['Flag(nil, "EnergyShieldProtectsMana")'] },
  { name: 'life-loss-prevented', inputs: { conditionUsingFlask: true }, mods: ['When hit during effect, 20% of Life loss from Damage taken occurs over 4 seconds instead'], covers: ['"LifeLossPrevented"'] },
  { name: 'ward-bypass', mods: ['+400 to Ward', '10% of Damage taken bypasses Ward', 'Damage taken bypasses unbroken Ward if the hit deals less damage than 5% of Ward', 'Ward has a 50% chance to not break'], covers: ['WardBypass', 'WardAvoidBreakChance'] },
  { name: 'ward-physical-reduction', mods: ['+400 to Ward', 'Physical Damage Reduction from Hits is based off of 50% of your Ward instead of your Armour'], covers: ['Flag(nil, "PhysicalReductionBasedOnWard")'] },
  { name: 'ally-spectres-totems', inputs: { TotalSpectreLife: 8000, conditionHaveTotem: true, TotalTotemLife: 5000 }, mods: ["30% of damage from hits is taken from your Spectres' life before you", "25% of damage from hits is taken from your nearest Totem's life before you"], covers: ['redirect'] },
  { name: 'es-bypass-flags', mods: ['Damage taken from Unblocked hits always bypasses Energy Shield', 'Damage taken from Blocked hits cannot bypass Energy Shield', 'Chaos Damage taken does not bypass Energy Shield', '+40% Chance to Block Attack Damage', '+500 to maximum Energy Shield'], covers: ['Flag(nil, "UnblockedDamageDoesBypassES")', 'Flag(nil, "BlockedDamageDoesntBypassES")'] },
  { name: 'damage-reserves-life', mods: ['Life that would be lost by taking damage is instead Reserved', 'Reserves 30% of Life'], covers: ['Flag(nil, "DamageInsteadReservesLife")'] },
  { name: 'stun-threshold-es', mods: ['Stun Threshold is based on 50% of your Energy Shield instead of Life', '+500 to maximum Energy Shield'], covers: ['StunThresholdBasedOnEnergyShieldInsteadOfLife'] },
  { name: 'stun-threshold-mana-and-add-es', mods: ['Stun Threshold is based on 30% of your Mana instead of Life', '10% of your Energy Shield is added to your Stun Threshold', '+500 to maximum Energy Shield'], covers: ['StunThresholdBasedOnManaInsteadOfLife', 'AddESToStunThreshold'] },
  // --- recovery: recoup, regen, degen ---
  { name: 'recoup-life', mods: ['20% of Damage taken Recouped as Life', '10% of Physical Damage taken Recouped as Life'], covers: ['RecoupRecoveryAmount'] },
  { name: 'recoup-es-instead-of-life', mods: ['20% of Damage taken Recouped as Life', 'Recoup Energy Shield instead of Life'], covers: ['Flag(nil, "EnergyShieldRecoupInsteadOfLife")'] },
  { name: 'degens', mods: ['Lose 3% of Life per second', 'Lose 1% of Mana per second', 'Lose 2% of Energy Shield per second', 'Regenerate 2% of Life per second', 'Regenerate 100 Energy Shield per second', '+500 to maximum Energy Shield'], covers: ['DegenPercent', 'RegenRecovery'] },
  { name: 'build-degen-chaos', mods: ['5% of maximum Life taken as Chaos Damage per second'], covers: ['"BuildDegen"'] },
  { name: 'build-degen-with-regen', mods: ['5% of maximum Life taken as Chaos Damage per second', 'Regenerate 100 Energy Shield per second', 'Regenerate 5% of Mana per second', '+500 to maximum Energy Shield'], covers: ['output.NetLifeRegen = output.NetLifeRegen - totalLifeDegen'] },
  { name: 'enemy-degen-dot', inputs: { enemyDamageType: 'DamageOverTime', enemyPhysicalDamage: 800, enemyFireDamage: 600, enemyChaosDamage: 400 }, covers: ['"EnemyDegen"', 'damageCategoryConfig == "DamageOverTime"'] },
  { name: 'zealots-oath', mods: ['Life Regeneration is applied to Energy Shield instead', 'Regenerate 50 Life per second'], covers: ['Flag(nil, "ZealotsOath")'] },
  { name: 'no-life-regen', mods: ['Life Regeneration has no effect', 'Cannot Gain Mana'], covers: ['Flag(nil, "No"..resource.."Regen")'] },
  { name: 'unaffected-by-regen', mods: ['Life Recovery from Regeneration is not applied', 'Mana Recovery from Regeneration is not applied', 'Regenerate 5% of Mana per second'], covers: ['"UnaffectedBy"..resource.."Regen"'] },
  { name: 'es-recharge-to-life', mods: ['Energy Shield Recharge instead applies to Life', '+500 to maximum Energy Shield'], covers: ['Flag(nil, "EnergyShieldRechargeAppliesToLife")'] },
  { name: 'es-recovery-caps', mods: ['Cannot Recover Energy Shield to above Armour', 'Cannot Recover Energy Shield to above Evasion Rating', '+500 to maximum Energy Shield'], inputs: { conditionLowEnergyShield: true }, covers: ['ArmourESRecoveryCap'] },
  // --- damage taken as / smoothing / armour application ---
  { name: 'taken-as-conversion', mods: ['50% of Physical Damage from Hits taken as Fire Damage', '30% of Fire and Lightning damage from Hits taken as Cold damage', '30% of Physical Damage taken as Chaos Damage'], covers: ['useConversionSmoothing', 'passIncomingDamage'] },
  { name: 'armour-applies-to-elemental', mods: ['+5000 to Armour', 'Armour applies to Fire, Cold and Lightning damage taken from hits instead of Physical Damage', '50% of Armour also applies to Chaos Damage taken from Hits'], covers: ['ArmourAppliesTo'] },
  // --- ailments / misc flags ---
  { name: 'ailment-immunities', mods: ['Cannot be Stunned', 'Cannot be Shocked', 'Cannot be Frozen', 'Cannot be Ignited', 'Cannot be Poisoned', 'You are immune to Curses', 'Cannot be Blinded', 'You cannot be Maimed', 'Cannot be Knocked Back'], covers: ['Immune")'] },
  { name: 'shock-avoid-applies-to-ailments', mods: ['Chance to avoid being Shocked applies to all Elemental Ailments', '+30% chance to avoid being Shocked'], covers: ['"ShockAvoidAppliesToElementalAilments"'] },
  // --- config: enemy / PvP / low pools ---
  { name: 'pvp-scaling', inputs: { PvpScaling: true, enemyMultiplierPvpDamage: 120 }, covers: ['env.configInput.PvpScaling'] },
  { name: 'enemy-boss-level', inputs: { enemyIsBoss: 'Uber', enemyLevel: 100, enemyPhysicalDamage: 5000, enemyPhysicalOverwhelm: 20, enemyFireDamage: 3000, enemyFirePen: 15, enemyCritChance: 30, enemyCritDamage: 80, enemySpeed: 1100 }, covers: ['env.configInput["enemy"..damageType.."Damage"]', 'enemyCritDamage'] },
  { name: 'enemy-damage-type-spell', inputs: { enemyDamageType: 'Spell' }, covers: ['damageCategoryConfig'] },
  { name: 'enemy-damage-type-projectile', inputs: { enemyDamageType: 'Projectile' }, mods: ['+25% chance to avoid Projectiles'], covers: ['AvoidProjectilesChance'] },
  { name: 'ward-not-break-flask', inputs: { conditionUsingFlask: true }, mods: ['+400 to Ward', 'Ward does not break during Flask effect'], covers: ['Flag(nil, "Condition:WardNotBreak")'] },
  { name: 'ally-radiance-sentinel', inputs: { TotalRadianceSentinelLife: 4000 }, mods: ["40% of damage from hits is taken from your Sentinel of Radiance's life before you"], covers: ['redirect'] },
  { name: 'low-life-config', inputs: { conditionLowLife: true }, covers: ['env.configInput["conditionLowLife"]'] },
  // --- batch 3 ---
  { name: 'evade-melee-projectile-split', mods: ['+3000 to Evasion Rating', '10% increased chance to evade melee attacks', '20% increased chance to evade projectile attacks'], covers: ['output.splitEvade'] },
  { name: 'pseudo-recoup-life', mods: ['5% of Physical Damage prevented from Hits in the past 10 seconds is regenerated as Life per second', '+1000 to Armour'], covers: ['PseudoRecoup'] },
  { name: 'pseudo-recoup-es', mods: ['3% of Physical Damage prevented from Hits recently is regenerated as Energy Shield per second', '+500 to maximum Energy Shield', '+1000 to Armour'], covers: ['PseudoRecoup'] },
  { name: 'recoup-add-flags', mods: ['20% of Damage taken Recouped as Life', '10% of Physical Damage taken Recouped as Life', 'Life Recoup also recovers Mana', 'Damage taken Recouped as Life is also Recouped as Energy Shield', '+500 to maximum Energy Shield'], covers: ['addFlag'] },
  { name: 'recoup-over-3-seconds', mods: ['20% of Damage taken Recouped as Life', 'Recoup effects instead occur over 3 seconds'], covers: ['"3SecondRecoup"'] },
  { name: 'suppress-applies-to-ailments-armour', mods: ['+40% chance to Suppress Spell Damage', 'Modifiers to Chance to Suppress Spell Damage also apply to Chance to avoid Elemental Ailments at 50% of their value', 'Modifiers to Chance to Suppress Spell Damage also apply to Chance to Defend with 50% of Armour at 100% of their value', '+1000 to Armour'], covers: ['SpellSuppressionAppliesToAilmentAvoidancePercent', 'SpellSuppressionAppliesToChanceToDefendWithArmourPercent'] },
  { name: 'regen-recovers-es-consecrated', inputs: { conditionOnConsecratedGround: true }, mods: ['Consecrated Ground you create causes Life Regeneration to also recover Energy Shield for you and allies', 'Regenerate 50 Life per second', '+500 to maximum Energy Shield'], covers: ['RegenerationRecovers'] },
  { name: 'life-regen-applies-to-es', mods: ['+50% Life Regeneration also applies to Energy Shield', 'Regenerate 50 Life per second', '+500 to maximum Energy Shield'], covers: ['LifeRegenAppliesToEnergyShield'] },
  { name: 'self-ignite-duration', mods: ['20% increased Ignite Duration on you', 'Modifiers to Ignite Duration on you apply to all Elemental Ailments'], covers: ['SelfIgniteDuration'] },
  { name: 'enemy-damage-type-melee', inputs: { enemyDamageType: 'Melee' }, mods: ['10% increased chance to evade melee attacks', '+25% chance to Dodge Attack Hits'], covers: ['damageCategoryConfig == "Melee"'] },
  { name: 'enemy-damage-type-spell-projectile', inputs: { enemyDamageType: 'SpellProjectile' }, mods: ['+25% chance to avoid Projectiles', '+25% chance to Dodge Spell Hits'], covers: ['damageCategoryConfig == "SpellProjectile"'] },
  { name: 'enemy-damage-type-untyped', inputs: { enemyDamageType: 'Untyped' }, covers: ['damageCategoryConfig == "Untyped"'] },
  { name: 'dodge-unlucky-worst-of-2', inputs: { EHPUnluckyWorstOf: 2 }, mods: ['+25% chance to Dodge Attack Hits', '+25% chance to Dodge Spell Hits', '+40% chance to Suppress Spell Damage'], covers: ['EHPUnluckyWorstOf'] },
] // SCENARIOS-END

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
// --- docker LuaJIT (twin of pob-ts luajitEval, no timeless LUT needed for Dual Savior: the LUT dir is mounted only if present) ---
async function ljEval(code: string): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r4s-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const luts = resolve(SCRATCH, 'luts')
    mkdirSync(luts, { recursive: true })
    const args = ['run', '--rm', '-v', `${POB}:/workdir:ro`, '-v', `${POBTS}/tools/oracle/lua:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${luts}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = (await run('docker', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 })).stdout } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
const POBTS = 'C:/pob-work/pob-ts'

function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
const loadChunk = (xml: Buffer, name: string): string =>
  `loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(name)})\nif __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n`

// --- XML of a scenario: Dual Savior with <Config> replaced ---
const baseXml = readFileSync(BASE_XML) // bytes, kept as latin1 text for the splice
const BASE_INPUT = { conditionLeeching: true } as Record<string, number | boolean | string> // Dual Savior.xml:707-709, kept in every scenario
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
  // gear: raw item texts (normal rarity bases) put into empty ItemSet slots (ids 100+), Items-level <Item> elements inserted before the first top-level <Slot>
  const slots = Object.keys(sc.gear ?? {})
  if (slots.length) {
    const items = slots.map((slot, i) => `		<Item id="${100 + i}">${nl}			${xmlEsc(sc.gear![slot]!.split('\n').join(nl))}${nl}		</Item>${nl}`).join('')
    const anchor = out.indexOf('		<Slot name="Weapon 1" itemId="1"/>')
    if (anchor < 0) throw new Error('no top-level <Slot name="Weapon 1"> anchor')
    out = out.slice(0, anchor) + items + out.slice(anchor)
    slots.forEach((slot, i) => {
      const empty = `<Slot name="${slot}" itemId="0"/>`
      if (!out.includes(empty)) throw new Error(`no empty ItemSet slot ${slot}`)
      out = out.replace(empty, `<Slot name="${slot}" itemId="${100 + i}"/>`)
    })
  }
  return Buffer.from(out, 'latin1')
}

// --- Lua probes ---
const RAWF = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
`
const DUMP = String.raw`
local out = build.calcsTab.mainOutput
local nums, strs, bools = {}, {}, {}
for k, v in pairs(out) do
  local t = type(v)
  if t == "number" then nums[k] = RAWF("%.17g", v) elseif t == "string" then strs[k] = v elseif t == "boolean" then bools[k] = v end
end
return { nums = nums, strs = strs, bools = bools }
`
// executable lines of CalcDefence.lua from the bytecode of every nested prototype (jit.util), + the lines hit while `body` runs
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
walk(assert(loadfile("Modules/CalcDefence.lua")))
`
// the hook is armed only around ONE calcs.buildOutput(build, "MAIN") after the build is loaded (= the pass whose output the fixture holds;
// the CALCS pass of loadBuildFromXML builds breakdown tables and would count `if breakdown then` lines the MAIN output never runs)
const COVER_POST = String.raw`
local hits, isDef = {}, {}
local function hook(_, line)
  local f = debug.getinfo(2, "f").func
  local d = isDef[f]
  if d == nil then d = debug.getinfo(2, "S").source:find("CalcDefence.lua", 1, true) ~= nil; isDef[f] = d end
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

// --- deterministic JSON ---
function ser(v: unknown): string {
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')

// --- covers: grep -n of CalcDefence.lua ---
const defenceLines = readFileSync(DEFENCE_LUA, 'latin1').split('\n')
const COVER_CACHE = resolve(SCRATCH, 'defence-synth-coverage-main2.json')
// `covers` = the lines each needle matches (grep -n) that the scenario really executed (debug.sethook cache of the `coverage` mode, keyed by
// the XML sha256: run `coverage` first); `newLines` = lines the scenario executed that none of the 6 goldens does.
function coverInfo(sc: Scenario, xml: Buffer): { covers: string[]; newLines: string; newLineCount: number } {
  if (!existsSync(COVER_CACHE)) throw new Error('no coverage cache: run `export-defence-synth.ts coverage` first')
  const cache: Rec = JSON.parse(readFileSync(COVER_CACHE, 'utf8'))
  const e = cache.entries[sc.name]
  if (!e || e.sha !== sha(xml)) throw new Error(`${sc.name}: coverage cache is stale: re-run \`coverage\``)
  const hit = new Set<number>(e.hits)
  const executable = new Set<number>(cache.executable)
  const gold = new Set<number>(goldens.flatMap((g) => cache.entries[g]?.hits ?? (() => { throw new Error(`no cached golden ${g}`) })()))
  const covers = sc.covers.map((needle) => {
    const all = defenceLines.flatMap((l, i) => (l.includes(needle) && executable.has(i + 1) ? [i + 1] : []))
    if (!all.length) throw new Error(`${sc.name}: cover needle matches no CalcDefence.lua line: ${needle}`)
    const lines = all.filter((n) => hit.has(n))
    if (!lines.length) throw new Error(`${sc.name}: cover needle ${needle} matches lines ${all.join(',')} but the scenario executes none of them`)
    return `CalcDefence.lua:${lines.join(',')} ${needle}`
  })
  const fresh = [...hit].filter((n) => executable.has(n) && !gold.has(n)).sort((x, y) => x - y)
  const ranges: string[] = []
  for (let i = 0; i < fresh.length;) { let j = i; while (j + 1 < fresh.length && fresh[j + 1]! === fresh[j]! + 1) j++; ranges.push(j > i ? `${fresh[i]}-${fresh[j]}` : `${fresh[i]}`); i = j + 1 }
  return { covers, newLines: ranges.length ? `CalcDefence.lua:${ranges.join(',')}` : '', newLineCount: fresh.length }
}

async function pool<T, R>(items: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(items.length)
  let next = 0
  await Promise.all(Array.from({ length: CONCURRENCY }, async () => {
    for (;;) { const i = next++; if (i >= items.length) return; res[i] = await f(items[i]!); console.error(`  done ${i + 1}/${items.length}`) }
  }))
  return res
}

async function evalOutput(sc: Scenario): Promise<Rec> {
  const xml = scenarioXml(sc)
  return ljEval(RAWF + loadChunk(xml, BASE) + DUMP)
}

async function fixtures(): Promise<void> {
  mkdirSync(FIX, { recursive: true })
  for (const s of SCENARIOS) coverInfo(s, scenarioXml(s)) // validate every cover needle against the coverage cache BEFORE spending docker runs
  const image = (await run('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', IMAGE]).catch(() => ({ stdout: IMAGE }))).stdout.trim()
  const pobSha = (await run('git', ['-C', POB, 'rev-parse', '--short', 'HEAD'])).stdout.trim()
  const runs: string[][] = []
  const names = SCENARIOS.map((s) => s.name)
  if (new Set(names).size !== names.length) throw new Error('duplicate scenario names')
  for (let r = 0; r < 2; r++) {
    console.error(`run ${r + 1}/2`)
    const outs = await pool(SCENARIOS, evalOutput)
    runs.push(outs.map((o, i) => {
      const sc = SCENARIOS[i]!
      const xml = scenarioXml(sc)
      const ci = coverInfo(sc, xml)
      const text = `${ser({ build: BASE, scenario: sc.name, covers: ci.covers, newLines: ci.newLines, newLineCount: ci.newLineCount, gear: sc.gear ?? {}, inputs: sc.inputs ?? {}, mods: sc.mods ?? [], xmlSha256: sha(xml), output: o,
        producer: { image, luajit: 'desktop LuaJIT docker, loadBuildFromXML -> build.calcsTab.mainOutput', pobSha, generator: 'tools/lua-corpus/export-defence-synth.ts' } })}\n`
      if (Buffer.byteLength(text) > MAX_BYTES) throw new Error(`${sc.name}: ${Buffer.byteLength(text)} bytes > 1.5 MB`)
      if (r === 0) { writeFileSync(resolve(FIX, `${sc.name}.xml`), xml); writeFileSync(resolve(FIX, `${sc.name}.luajit.json`), text) }
      return sha(text) + ' ' + sha(xml)
    }))
  }
  const same = runs[0]!.every((h, i) => h === runs[1]![i])
  console.log(`scenarios ${SCENARIOS.length}; run1 == run2 byte-identical: ${same}`)
  SCENARIOS.forEach((s, i) => console.log(`${s.name}\t${runs[0]![i]!.slice(0, 16)}\t${runs[1]![i]!.slice(0, 16)}\t${coverInfo(s, scenarioXml(s)).covers.join(' | ')}`))
  if (!same) process.exitCode = 1
}

const goldens = ['Dual Savior', 'Dual Wield Cospris CoC', 'Dual Wield Cospris CoC - Lethal Pride', 'Generals Perforate Zerker', 'Mirage Archer Toxic Rain', 'OccVortex']
async function coverage(): Promise<void> {
  mkdirSync(SCRATCH, { recursive: true })
  type Job = { label: string; kind: 'golden' | 'synth'; xml: Buffer; name: string }
  const jobs: Job[] = [
    ...goldens.map((g) => ({ label: g, kind: 'golden' as const, xml: readFileSync(g === 'Dual Wield Cospris CoC - Lethal Pride' ? `${ORACLE}/test/fixtures/${g}.xml` : `${POB}/spec/TestBuilds/3.13/${g}.xml`), name: g })),
    ...SCENARIOS.map((s) => ({ label: s.name, kind: 'synth' as const, xml: scenarioXml(s), name: BASE })),
  ]
  const cachePath = resolve(SCRATCH, 'defence-synth-coverage-main2.json')
  const cache: Rec = existsSync(cachePath) ? JSON.parse(readFileSync(cachePath, 'utf8')) : { entries: {} }
  const keyOf = (j: Job): string => sha(j.xml)
  const todo = jobs.filter((j) => cache.entries[j.label]?.sha !== keyOf(j))
  console.error(`coverage: ${jobs.length} jobs, ${todo.length} to run (the rest cached by xml sha256)`)
  const fresh = await pool(todo, (j) => ljEval(COVER_PRE + loadChunk(j.xml, j.name) + COVER_POST))
  todo.forEach((j, i) => { cache.entries[j.label] = { sha: keyOf(j), hits: fresh[i]!.hits }; if (!cache.executable) cache.executable = fresh[i]!.executable })
  writeFileSync(cachePath, JSON.stringify(cache) + '\n')
  const res = jobs.map((j) => ({ hits: cache.entries[j.label].hits as number[], executable: cache.executable as number[] }))
  const executable = new Set<number>(cache.executable as number[])
  const inRange = (l: number, a: number, b: number): boolean => l >= a && l <= b
  const ranges: [string, number, number][] = [['helpers 45-648', 45, 648], ['defence 651-1671', 651, 1671], ['estimations 1674-3872', 1674, 3872], ['all 45-3872', 45, 3872]]
  const cnt = (set: Set<number>, a: number, b: number): number => [...set].filter((l) => executable.has(l) && inRange(l, a, b)).length
  const gold = new Set<number>(); const synth = new Set<number>()
  const per: Rec = {}
  jobs.forEach((j, i) => { const h = res[i]!.hits as number[]; h.forEach((l) => (j.kind === 'golden' ? gold : synth).add(l)); per[j.label] = h })
  const union = new Set<number>([...gold, ...synth])
  console.log('range\texecutable\tgoldens\tgoldens+synth')
  for (const [n, a, b] of ranges) console.log(`${n}\t${cnt(executable, a, b)}\t${cnt(gold, a, b)}\t${cnt(union, a, b)}`)
  // per scenario: which of its covers lines were hit, and which were NOT hit by any golden
  const detail: Rec = {}
  for (const [i, s] of SCENARIOS.entries()) {
    const hit = new Set<number>(per[s.name])
    const lines = s.covers.flatMap((n) => defenceLines.flatMap((l, k) => (l.includes(n) ? [k + 1] : [])))
    const miss = lines.filter((l) => executable.has(l) && !hit.has(l))
    const newOnes = [...hit].filter((l) => executable.has(l) && !gold.has(l))
    detail[s.name] = { coversLines: lines.length, coversHit: lines.length - miss.length, coversNotHit: miss, linesNotInAnyGolden: newOnes.length }
    console.log(`${i + 1}\t${s.name}\tcovers ${lines.length - miss.length}/${lines.length}${miss.length ? ' NOT HIT ' + miss.join(',') : ''}\tnew-vs-goldens ${newOnes.length}`)
  }
  writeFileSync(resolve(SCRATCH, 'defence-synth-coverage-report.json'), ser({ executable: [...executable], gold: [...gold], synth: [...synth], detail }) + '\n')
}

async function parse(file?: string): Promise<void> {
  const lines = (file ? readFileSync(file, 'utf8') : [...new Set(SCENARIOS.flatMap((s) => s.mods ?? []))].join('\n')).split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'))
  const code = String.raw`
local out = {}
for i, line in ipairs(LINES) do
  local mods, extra = modLib.parseMod(line)
  local n = mods and #mods or -1
  local d = {}
  if mods then for _, m in ipairs(mods) do d[#d + 1] = modLib.formatSourceMod and modLib.formatSourceMod(m) or m.name end end
  out[i] = (mods and not extra) and ("OK " .. table.concat(d, " ; ")) or ("FAIL extra=" .. tostring(extra) .. " n=" .. n)
end
return out
`
  const lua = `local LINES = {${lines.map((l) => JSON.stringify(l)).join(',')}}\n${code}`
  const r = (await ljEval(lua)) as unknown as string[]
  lines.forEach((l, i) => console.log(`${r[i]!.startsWith('OK') ? 'OK  ' : 'FAIL'} | ${l} | ${r[i]}`))
}

async function effect(): Promise<void> { // how many LuaJIT output keys each scenario changes against the plain Dual Savior (a scenario that changes nothing proves nothing)
  const base = await evalOutput({ name: 'baseline', covers: [] })
  let weak = 0
  for (const sc of SCENARIOS) {
    const fx = JSON.parse(readFileSync(resolve(FIX, `${sc.name}.luajit.json`), 'utf8')).output as Rec
    let n = 0
    for (const t of ['nums', 'strs', 'bools']) for (const k of new Set([...Object.keys(base[t] ?? {}), ...Object.keys(fx[t] ?? {})])) if (JSON.stringify((base[t] ?? {})[k]) !== JSON.stringify((fx[t] ?? {})[k])) n++
    if (n === 0) weak++
    console.log(`${sc.name}	${n}`)
  }
  console.log(`scenarios changing no output key: ${weak}`)
}

if (process.argv[2] === 'effect') await effect()
else if (process.argv[2] === 'show') { // authoring aid: show <scenario> <key>... -> LuaJIT values
  const sc = SCENARIOS.find((x) => x.name === process.argv[3])!
  const o = await evalOutput(sc)
  for (const k of process.argv.slice(4)) console.log(k, o.nums[k] ?? o.strs[k] ?? o.bools[k])
  const probe = String.raw`local e = build.calcsTab.calcs.buildOutput(build, "MAIN")
local a = e.player.itemList["Body Armour"]
return { life = RAWF("%.17g", e.player.output.Life), slot = a and a.name or "nil", ad = a and a.armourData and "armourData" or "none", es = a and a.armourData and tostring(a.armourData.EnergyShield) or "-" }`
  const r = await ljEval(RAWF + loadChunk(scenarioXml(sc), BASE) + probe)
  console.log('second buildOutput:', JSON.stringify(r))
} else if (process.argv[2] === 'coverage') await coverage()
else if (process.argv[2] === 'parse') await parse(process.argv[3])
else await fixtures()
