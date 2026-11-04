# R-party-export: synthetic builds with `Export Support` on (every branch of buffExports the public goldens do not reach) and the scenes over them and over a public golden.
# Run from the repo root: python tools/sg-oracle/scenes/import/gen_party_export.py
#   -> crates/pob-engine/tests/fixtures/party-export/<name>.xml, tools/sg-oracle/scenes/import/{party-export,px-*}.json, index.json
# The truth: pwsh tools/sg-oracle/run.ps1 -Script import-runner.lua -ScriptArgs <scenes> -Timeout 120, then python tools/sg-oracle/scenes/import/gen_party_export.py truth
#   -> crates/pob-engine/tests/fixtures/party-export/<name>.party.xml (the <Party> element of the original's SaveDB)
import json, os, re, sys

SYNTH = 'crates/pob-calc/tests/fixtures/offence-synth'
OUT = 'crates/pob-engine/tests/fixtures/party-export'
SCENES = 'tools/sg-oracle/scenes/import'
GEMS = json.load(open('data/gems.json', encoding='utf-8'))['gems']


def gem(skill_id, extra=''):
    for gem_id, g in GEMS.items():
        if g.get('grantedEffectId') == skill_id and not re.search(r'Alt[XY]$', g.get('variantId') or ''):
            name = g['name'].replace("'", '&apos;')
            return f'<Gem enableGlobal1="true" enableGlobal2="false" qualityId="Default" quality="0" enabled="true" count="1" level="20" gemId="{gem_id}" variantId="{g["variantId"]}" skillId="{skill_id}" nameSpec="{name}"{extra}/>'
    raise SystemExit(f'no gem with grantedEffectId {skill_id}')


def group(ids):
    gems = '\n'.join(i[1] if isinstance(i, tuple) else gem(i) for i in ids)
    return f'<Skill mainActiveSkillCalcs="1" includeInFullDPS="false" label="" enabled="true" mainActiveSkill="1">\n{gems}\n</Skill>'


def build(base, groups, mods=(), inputs=(), main_last=False, spectre=None):
    xml = open(f'{SYNTH}/{base}.xml', encoding='utf-8', newline='').read().replace('\r\n', '\n')
    tag = '</SkillSet>' if '</SkillSet>' in xml else '</Skills>'
    i = xml.index(tag)
    xml = xml[:i] + '\n'.join(group(g) for g in groups) + '\n' + xml[i:]
    block = ''.join(f'<Input name="{k}" boolean="true"/>\n' for k in inputs)
    if mods:
        block += '<CustomModifierBlock title="Default" enabled="true">\n' + '\n'.join(mods) + '\n</CustomModifierBlock>\n'
    if block:
        xml = re.sub(r'(<Config[^>]*>\n)', lambda m: m.group(1) + block, xml, count=1)
    if main_last:
        # MAIN and the Calcs tab's skill: the export is the CALCS run's
        last = len(re.findall("<Skill ", xml))
        xml = re.sub(r'mainSocketGroup="\d+"', f'mainSocketGroup="{last}"', xml, count=1)
        xml, n = re.subn(r'(<Input name="skill_number" number=")\d+', lambda m: m.group(1) + str(last), xml, count=1)
        assert n == 1, base
    if spectre:
        xml, n = re.subn(r'<Spectre id="[^"]*"/>', ''.join(f'<Spectre id="{s}"/>' for s in spectre), xml, count=1)
        assert n == 1, base
    xml, n = re.subn(r'<Import[^>]*/>', '<Import exportParty="true"/>', xml, count=1)
    assert n == 1, base
    return xml


def mine(skill, main, calcs):
    return ('m', gem(skill, f' skillMineCount="{main}" skillMineCountCalcs="{calcs}"'))


HIM = ('s', gem('SupportHighImpactMine'))
BUILDS = {
    # auras, curses, an aura curse, a mark, otherEffects (`buffAllies`), extraAura, the resist / charge / fortify lines of PlayerMods
    'px-auras': build('fd-gen-melee', [
        ['Anger'], ['Hatred'], ['Determination'], ['Clarity'], ['Pride'], ['Malevolence'], ['Haste'], ['VaalHaste'], ['Vulnerability'], ['Enfeeble'], ['SnipersMark'],
        ['Flammability', 'SupportBlasphemy'], ['HeraldOfAsh'], ['FlameWall'], ['SigilOfPower'],
    ], [
        'nearby allies count as having fortification equal to yours',
        'nearby enemies have fire resistance equal to yours',
        'nearby enemies have cold resistance equal to yours',
        'your nearby party members maximum endurance charges is equal to yours',
    ]),
    # AurasAffectEnemies (:2529-2542), banners with an extra effect and debuff (:2474, :2575-2610)
    'px-auras-enemies': build('fd-gen-melee', [['Anger'], ['Pride'], ['WarBanner'], ['DreadBanner'], ['DefianceBanner'], ['Vulnerability']], [
        'auras from your skills which affect allies also affect enemies',
        'placed banners also grant 12% increased attack damage to you and allies',
        'banners also cause enemies to take 10% increased damage',
    ], ['bannerPlanted']),
    'px-warcry': build('fd-gen-melee', [['IntimidatingCry'], ['RallyingCry'], ['EnduringCry'], ['SeismicCry'], ['AncestralCry'], ['GeneralsCry']]),
    # Rallying Cry with a minion and a weapon: `extraWarcryModList` (:2441-2465)
    'px-warcry-minion': build('fd-gen-melee', [['RallyingCry'], ['IntimidatingCry'], ['Anger'], ['Enfeeble'], ['RaiseZombie']], main_last=True),
    # every Link, the extra link mods, the flask line
    'px-links': build('fd-gen-melee', [['FlameLink'], ['ProtectiveLink'], ['SoulLink'], ['VampiricLink'], ['DestructiveLink'], ['IntuitiveLink']], [
        'non-unique utility flasks you use apply to linked targets',
        'curses are inflicted on you instead of linked targets',
        'elemental ailments are inflicted on you instead of linked targets',
    ]),
    # linked to the minion: the first Link closes the branch (:2737, `linkSkills[1]`), the flasks reach it
    'px-links-minion': build('fd-gen-melee', [['SoulLink'], ['FlameLink'], ['RaiseZombie']], [
        'non-unique utility flasks you use apply to linked targets',
        'link skills can target damageable minions',
    ], ['linkedToMinion'], main_last=True),
    # mine auras: the stack (:3893-3931), the count of the CALCS run, a support's aura (a second aura on one skill: ` Limit`)
    'px-mines': build('t-mine', [[mine('IcicleMine', 5, 3), HIM], [mine('StormblastMine', 2, 4)], [mine('PyroclastMine', 0, 0), HIM], ['Anger']]),
    # the spectre's aura and curse (:2915-2925), golems (`buffAllies`), a golem's aura
    'px-spectre': build('mn-spectre-hydra-mid', [['Anger'], ['SummonStoneGolem'], ['SummonFlameGolem'], ['SummonLightningGolem'], [('m', gem('RaiseSpectre', ' skillMinion="Metadata/Monsters/LeagueAzmeri/SpecialCorpses/DemonBossHigh"'))]], inputs=['summonLightningGolemEnableWrath'],
                        spectre=['Metadata/Monsters/LeagueAzmeri/SpecialCorpses/Hydra/HydraHigh_', 'Metadata/Monsters/LeagueAzmeri/SpecialCorpses/DemonBossHigh']),
    # a minion skill's buff for allies (:2838)
    'px-reaper': build('mn-reaper', [['Anger']], inputs=['summonReaperConsumeRecently']),
    # no skills but the base ones: `Curse` holds its limit alone, an empty `extraAura`
    'px-bare': build('mn-absolution', []),
}


def click():
    return [["KeyDown", "LEFTBUTTON"], ["KeyUp", "LEFTBUTTON"]]


AWAY = {"cursor": [1200, 790]}
TAB = {"cursorBuild": "modeImport", "events": click(), "dump": True}
PARTY = {"cursorBuild": "modeParty", "events": click(), "dump": True}


def tapi(c, **k):
    return {"cursorImport": c, "events": click(), "dump": True, **k}


def write_scene(name, steps):
    json.dump({"steps": steps}, open(f'{SCENES}/{name}.json', 'w', newline='\n'), indent=0)


def scenes():
    # a public golden, by hand: off, on, the code, the Party tab, saved and opened again, off again
    write_scene('party-export', [
        {"open": "OccVortex"},
        TAB | {"xml": True, "shot": "party-export-off"},
        {"cursorImport": "enablePartyExportBuffs", "dump": True, "shot": "party-export-hover"},
        tapi("enablePartyExportBuffs", xml=True),
        AWAY | {"dump": True, "xml": True, "shot": "party-export-on"},
        tapi("generateCode", xml=True),
        AWAY | {"dump": True},
        # the zlib streams differ: the shot of the field holds a fixed text
        AWAY | {"setGenerated": "U3ludGhldGljIGNvZGUgZm9yIHRoZSBzY3JlZW5zaG90", "dump": True, "shot": "party-export-code"},
        PARTY | {"shot": "party-export-party"},
        AWAY | {"dump": True},
        TAB,
        AWAY | {"hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "dump": True, "file": "OccVortex.xml", "fileOut": "party-export-saved.xml"},
        {"release": ["CTRL"]},
        {"open": "OccVortex"},
        TAB | {"xml": True, "shot": "party-export-reopened"},
        PARTY,
        TAB,
        tapi("enablePartyExportBuffs", xml=True),
        AWAY | {"dump": True, "xml": True, "shot": "party-export-off-again"},
        tapi("generateCode"),
        AWAY | {"dump": True},
        PARTY,
    ])
    # the other public goldens: the XML alone, on after a click
    golden = []
    # (not Dual Wield Cospris CoC: its `VaalRighteousFire` variant is of the process, R13/R48)
    for b in ("Dual Savior", "Mirage Archer Toxic Rain", "Generals Perforate Zerker"):
        golden += [{"open": b}, TAB, tapi("enablePartyExportBuffs"), AWAY | {"dump": True, "xml": True}]
    write_scene('party-export-goldens', golden)
    for name in BUILDS:
        write_scene(name, [
            {"open": name},
            TAB | {"xml": True, "shot": f"{name}-import"},
            tapi("generateCode", xml=True),
            AWAY | {"dump": True},
            PARTY,
        ])
    index = json.load(open(f'{SCENES}/index.json'))
    for n in ['party-export', 'party-export-goldens', *BUILDS]:
        if n not in index:
            index.append(n)
    json.dump(index, open(f'{SCENES}/index.json', 'w', newline='\n'))


def truth():
    for name in BUILDS:
        rec = json.load(open(f'tools/sg-oracle/fixtures/import/{name}.json', encoding='utf-8'))
        xml = next(d['state']['xml'] for d in rec['dumps'] if isinstance(d['state'], dict) and d['state'].get('xml'))
        m = re.search(r'\t<Party[ >].*?</Party>\n', xml, re.S)
        assert m, name
        open(f'{OUT}/{name}.party.xml', 'w', encoding='utf-8', newline='\n').write(m.group(0))
        print(name, len(m.group(0)))


if __name__ == '__main__':
    if sys.argv[1:] == ['truth']:
        truth()
    else:
        os.makedirs(OUT, exist_ok=True)
        for name, xml in BUILDS.items():
            open(f'{OUT}/{name}.xml', 'w', encoding='utf-8', newline='\n').write(xml)
        scenes()
        print(len(BUILDS), 'builds')
