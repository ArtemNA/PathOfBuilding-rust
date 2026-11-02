// R-party: synthetic scenarios for the Party-tab branches no real build reaches (Warcry, otherEffects, extraAura, AuraDebuff, Vaal auras,
// minions, `MainHand.CritChance`, the `partyMembers.output` reads), Energy Blade (CalcSetup.lua:1134, :1946; CalcOffence.lua:568) and
// LinkedSupport (CalcSetup.lua:1611). Base XMLs: crates/pob-calc/tests/fixtures/offence-synth. Writes the XMLs to
// crates/pob-calc/tests/fixtures/party-synth; the LuaJIT truth: node tools/lua-corpus/export-real.ts --set party-synth (same folder);
// gate: crates/pob-calc/tests/party_synth.rs.
// Run from the pob-rust root (Node 24): node tools/lua-corpus/gen-party-synth.ts
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const BASE = resolve(ROOT, 'crates/pob-calc/tests/fixtures/offence-synth')
const OUT = resolve(ROOT, 'crates/pob-calc/tests/fixtures/party-synth')
type Rec = Record<string, any> // eslint-disable-line
const gems: Rec = JSON.parse(readFileSync(resolve(ROOT, 'data/gems.json'), 'utf8')).gems
function gem(skillId: string, level = 20): string {
  const hit = Object.entries(gems).find(([, g]) => (g as Rec).grantedEffectId === skillId && !/Alt[XY]$/.test((g as Rec).variantId ?? ''))
  if (!hit) throw new Error(`no gem with grantedEffectId ${skillId}`)
  const [id, g] = hit as [string, Rec]
  return `<Gem enableGlobal1="true" enableGlobal2="false" qualityId="Default" quality="0" enabled="true" count="1" level="${level}" gemId="${id}" variantId="${g.variantId}" skillId="${skillId}" nameSpec="${g.name}"/>`
}
const group = (ids: string[], slot?: string): string =>
  `<Skill mainActiveSkillCalcs="1" includeInFullDPS="false" label="" enabled="true" mainActiveSkill="1"${slot ? ` slot="${slot}"` : ''}>\n${ids.map((i) => gem(i)).join('\n')}\n</Skill>`
function addGroups(xml: string, groups: string[]): string {
  const tag = xml.includes('</SkillSet>') ? '</SkillSet>' : '</Skills>'
  const i = xml.indexOf(tag)
  if (i < 0) throw new Error('no skills section')
  return xml.slice(0, i) + groups.join('\n') + '\n' + xml.slice(i)
}
type Buffers = Partial<Record<'PartyMemberStats' | 'Aura' | 'Curse' | 'Warcry Skills' | 'Link Skills' | 'EnemyConditions' | 'EnemyMods', string>>
function party(xml: string, b: Buffers): string {
  const body = Object.entries(b).map(([k, v]) => `<ImportedBuffs name="${k}">\n${v}\n</ImportedBuffs>`).join('\n')
  const sec = `<Party destination="All" ShowAdvanceTools="false" append="false">\n${body}\n</Party>\n`
  const stripped = xml.replace(/[ \t]*<Party[\s>/][\s\S]*?(<\/Party>|\/>)\n?/, '')
  return stripped.replace('</PathOfBuilding>', sec + '</PathOfBuilding>')
}
const mod = (v: string | number, skill: string, name: string, ty: string, tags = '-'): string => `${v}|Skill:${skill}|${name}|${ty}|-|-|${tags}`
const ge = (t: string, extra = ''): string => `type=GlobalEffect/effectType=${t}${extra}`
const AURA = [
  // stronger than the player's Anger: the player's own is dropped (CalcPerform.lua:2491), the ally's applies
  'Anger', '180', mod(40, 'Anger', 'FireMin', 'BASE', ge('Aura')), mod(70, 'Anger', 'FireMax', 'BASE', ge('Aura')), '---',
  // weaker than the player's Hatred: the player's wins (the ally's condition is already set, :3045)
  'Hatred', '50', mod(10, 'Hatred', 'PhysicalDamageGainAsCold', 'BASE', ge('Aura')), '---',
  'Vitality', '120', mod(100, 'Vitality', 'LifeRegen', 'BASE', ge('Aura')), mod(1, 'Vitality', 'Life', 'INC', ge('Aura')), '---',
  'Vaal Haste', '130', mod(20, 'VaalHaste', 'Speed', 'INC', ge('Aura')), '---',
  // the source is not a skill id: renamed after the buff (PartyTab.lua:876-885)
  'Purity of Ice', '110', mod(15, 'NoSuchSkill', 'ColdResist', 'BASE', ge('Aura')), '---',
  // an AuraDebuff stronger than the player's Pride, a new one, and a Vaal one
  'Pride_Debuff', '250', mod(10, 'Pride', 'PhysicalDamageTaken', 'INC', ge('AuraDebuff')), '---',
  'Malevolence_Debuff', '100', mod(-11, 'Malevolence', 'FireResist', 'BASE', ge('AuraDebuff')), '---',
  'Vaal Grace', '100', mod(-5, 'VaalGrace', 'ColdResist', 'BASE', ge('AuraDebuff')), '---',
  'otherEffects',
  `Dread Banner|120|${mod(100, 'DreadBanner', 'Armour', 'BASE', ge('Buff'))}`,
  `Dread Banner|120|${mod(5, 'DreadBanner', 'Evasion', 'INC', ge('Buff'))}`,
  '---',
  'extraAura', mod(7, 'NoSuchSkill', 'LightningResist', 'BASE'), '---',
  // an item source: `Party - <name>` (:871-875); a comment line (:807)
  '--- a comment ---',
  'Clarity', '100', '12|Item:7:Some Ring, Coral Ring|ManaRegen|BASE|-|-|type=GlobalEffect/effectType=Aura', '---',
].join('\n')
const WARCRY = [
  'Rallying Cry', '140', mod(12, 'RallyingCry', 'Damage', 'INC', ge('Warcry')), mod(3, 'RallyingCry', 'ManaRegen', 'BASE', ge('Warcry', '/warcryPowerBonus=1.5')), '---',
  'Enduring Cry', '100', mod(10, 'EnduringCry', 'LifeRegen', 'BASE', ge('Warcry')), '---',
].join('\n')
const LINK = [
  'Flame Link', '115', mod(5, 'FlameLink', 'FireMin', 'BASE', `${ge('Link')},type=PercentStat/actor=parent/percent=1/stat=Life`), mod(419, 'FlameLink', 'FireMax', 'BASE', ge('Link')), '---',
].join('\n')
const CURSE = [
  // the limit leaves room for all: which curses of a full list get a slot depends on `pairs` over names (CalcPerform.lua:3224, R13)
  '--- Curse Limit ---', '5',
  'Assassin\'s Mark', '100', 'true', mod(49, 'AssassinsMark', 'SelfCritMultiplier', 'BASE', ge('Curse')), '---',
  'Flammability', '130', 'false', mod(-30, 'Flammability', 'FireResist', 'BASE', ge('Curse')), '---',
  'Enfeeble', '100', 'false', mod(-20, 'Enfeeble', 'Damage', 'MORE', ge('Curse')), '---',
].join('\n')
const ENEMY_MODS = [
  'CurseEffectOnSelf', mod(-25, 'Curse', 'CurseEffectOnSelf', 'MORE', ge('Debuff')),
  'Mania', '-30|15 Mania Stacks|ActionSpeed|INC|-|-|type=GlobalEffect/effectType=Debuff,type=Condition/var=AfflictedByMania',
].join('\n')
const ENEMY_COND = 'Condition:Bleeding\nCondition:Poisoned\nCondition:RareOrUnique'
const FULL: Buffers = { PartyMemberStats: 'Life=9000\nMana=2000', Aura: AURA, Curse: CURSE, 'Warcry Skills': WARCRY, 'Link Skills': LINK, EnemyConditions: ENEMY_COND, EnemyMods: ENEMY_MODS }
const read = (n: string): string => readFileSync(resolve(BASE, `${n}.xml`), 'latin1').replace(/\r\n/g, '\n')
const main = (xml: string, n: number): string => xml.replace(/mainSocketGroup="\d+"/, `mainSocketGroup="${n}"`)
const OWN = ['Anger', 'Hatred', 'Pride', 'Enfeeble']
const scenarios: Record<string, string> = {
  // the full tab over a melee build with the same auras / curse of its own
  'party-attack-full': party(addGroups(read('fd-gen-melee'), OWN.map((s) => group([s]))), FULL),
  // the same tab on a minion build: the `env.minion` halves
  'party-minion-full': party(addGroups(main(read('fd-occ-minions'), 2), ['Anger', 'Haste', 'Enfeeble'].map((s) => group([s]))), FULL),
  // the actor reads: `partyMembers.output` / `modDB` through Link flags (Parent -> PartyMember) and PartyMemberStats
  'party-member-stats': party(read('fd-gen-melee'), {
    PartyMemberStats: [
      'Life=9000', 'FireResist=33', 'BlockChanceMax=85', 'BlockChance=61', 'MaxLifeLeechRatePercent=30', 'MovementSpeedMod|percent|max=273',
      'EnduranceChargesMax=9', 'FortificationStacks=11', 'EnergyShieldRecoveryCap=1500', 'MainHand.CritChance=7.5', 'NoNumber=abc',
      'EnemyFireResistEqualToYours', 'PartyMemberMaximumEnduranceChargesEqualToYours', 'PartyMemberFortifyEqualToYours',
    ].join('\n'),
    'Link Skills': [
      'Intuitive Link', '100',
      mod(1, 'IntuitiveLink', 'MainHandCritIsEqualToParent', 'FLAG', ge('Link')).replace('1|', 'true|'),
      mod(1, 'IntuitiveLink', 'BlockAttackChanceIsEqualToParent', 'FLAG', ge('Link')).replace('1|', 'true|'),
      mod(1, 'IntuitiveLink', 'MaximumBlockAttackChanceIsEqualToParent', 'FLAG', ge('Link')).replace('1|', 'true|'),
      mod(1, 'IntuitiveLink', 'MaximumLifeLeechIsEqualToParent', 'FLAG', ge('Link')).replace('1|', 'true|'),
      mod(1, 'IntuitiveLink', 'MovementSpeedEqualHighestLinkedPlayers', 'FLAG', ge('Link')).replace('1|', 'true|'),
      mod(25, 'IntuitiveLink', 'TakenFromParentESBeforeYou', 'BASE', ge('Link')),
      '---',
    ].join('\n'),
  }),
  // an empty tab and a tab of empty buffers (no effect on anything)
  'party-empty-buffers': party(read('fd-gen-melee'), { Aura: '', Curse: '', EnemyMods: '' }),
  // Energy Blade: the melee weapons replaced (1h here), a bow kept
  'energy-blade-melee': addGroups(read('fd-gen-melee'), [group(['EnergyBlade'])]),
  'energy-blade-bow': addGroups(read('fd-mir-bow-ballista'), [group(['EnergyBlade'])]),
  // LinkedSupport: supports socketed in the body armour also support the main hand's skills
  'linked-support': addGroups(
    read('fd-gen-melee').replace(/(<Skill [^>]*?)>/, '$1 slot="Weapon 1">').replace(/(<Item id="16"[\s\S]*?)(\s*<\/Item>)/, '$1\nSocketed Support Gems can also Support Skills from your Main Hand$2'),
    [group(['SupportCullingStrike', 'SupportBrutality'], 'Body Armour')],
  ),
}
mkdirSync(OUT, { recursive: true })
for (const [n, x] of Object.entries(scenarios)) writeFileSync(resolve(OUT, `${n}.xml`), Buffer.from(x, 'latin1'))
console.log(`${Object.keys(scenarios).length} scenarios -> ${OUT}`)
