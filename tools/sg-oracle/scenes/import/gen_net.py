# U8c: the scenes of the network. Run from the repo root: python tools/sg-oracle/scenes/import/gen_net.py
# Nothing here came from a site: every reply is written by hand from what upstream's code reads (no real account, character or token). A scene's "net" is what the fake transport answers on BOTH sides:
# import-runner.lua replaces launch:DownloadPage and LaunchSubScript with it, crates/pob-ui/tests/import.rs gives pob_net::ScriptedTransport the same list.
import base64, json, os, zlib

ROOT = json.load(open('tools/upstream.json'))['pobRoot']
OUT = 'tools/sg-oracle/scenes/import'
TREE = json.load(open('data/tree/3_29.json', encoding='utf-8'))


def code_of(text):
    data = text if isinstance(text, bytes) else text.encode('utf-8')
    return base64.b64encode(zlib.compress(data)).decode().replace('+', '-').replace('/', '_')


def click(k='LEFTBUTTON'):
    return [["KeyDown", k], ["KeyUp", k]]


def write(name, steps, net=None, names=[]):
    out = []
    for s in steps:
        if 'hold' in s and 'release' in s and 'events' in s:
            s = dict(s)
            rel = s.pop('release')
            out.append(s)
            out.append({'release': rel})
        else:
            out.append(s)
    json.dump({"net": net or [], "steps": out}, open(f'{OUT}/{name}.json', 'w', newline='\n'), indent=0)
    names.append(name)
    return names


def imp(c, **k):
    return {"cursorImport": c, **k}


def tapi(c, **k):
    return imp(c, events=click(), dump=True, **k)


def par(c, **k):
    return {"cursorParty": c, **k}


def tapp(c, **k):
    return par(c, events=click(), dump=True, **k)


def pick(tab, c, row, **k):
    return {"cursorDrop": [tab, c, row], "events": click(), "dump": True, **k}


AWAY = {"cursor": [1200, 790]}
TAB = {"cursorBuild": "modeImport", "events": click(), "dump": True}
PARTY = {"cursorBuild": "modeParty", "events": click(), "dump": True}
DELIVER = {"netDeliver": True, "dump": True}


def typed(text):
    return [["Char", c] for c in text]


def paste(text):
    return {"clipboard": text, "hold": ["CTRL"], "events": [["KeyDown", "v"], ["KeyUp", "v"]], "release": ["CTRL"], "dump": True}


def select_all_delete():
    return {"hold": ["CTRL"], "events": [["KeyDown", "a"], ["KeyUp", "a"], ["KeyDown", "BACK"], ["KeyUp", "BACK"]], "release": ["CTRL"], "dump": True}


def ok(url, body, header="HTTP/1.1 200 OK\r\n\r\n"):
    return {"url": url, "code": 200, "header": header, "body": body}


def fail(url, code, body=""):
    return {"url": url, "code": code, "header": f"HTTP/1.1 {code} X\r\n\r\n", "body": body}


def err(url, msg):
    return {"url": url, "error": msg}


GOLDEN = open(f'{ROOT}/spec/TestBuilds/3.13/Generals Perforate Zerker.xml', 'rb').read()
CODE = code_of(GOLDEN)

# --- part 2: a build by link, every site's URL, the errors
SITES = [
    ("https://maxroll.gg/poe/pob/abc123", "https://maxroll.gg/poe/api/pob/abc123"),
    ("https://pob.codes/b/abc123?x=1", "https://api.pob.codes/abc123/raw"),
    ("https://pobb.in/abc123", "https://pobb.in/pob/abc123"),
    ("https://poe.ninja/poe1/pob/abc123", "https://poe.ninja/poe1/pob/raw/abc123"),
    ("https://poe.ninja/pob/def456", "https://poe.ninja/poe1/pob/raw/def456"),
    ("https://pastebin.com/abc123", "https://pastebin.com/raw/abc123"),
    ("https://pastebinp.com/abc123", "https://pastebinp.com/raw/abc123"),
    ("https://rentry.co/abc123", "https://rentry.co/paste/abc123/raw"),
    ("https://poedb.tw/pob/abc123", "https://poedb.tw/pob/abc123/raw"),
]
steps = [{"open": "OccVortex"}, TAB]
net = []
for link, raw in SITES:
    net.append(err(raw, "synthetic failure"))
    steps += [imp("importCodeIn", events=click()), select_all_delete(), paste(link), tapi("importCodeGo"), DELIVER]
# the answers of a site, on pobb.in
ERR_404 = "https://pobb.in/pob/e404"
EMPTY = "https://pobb.in/pob/empty"
BAD = "https://pobb.in/pob/notacode"
NET = [fail(ERR_404, 404), ok(EMPTY, ""), ok(BAD, "this is not a build code")]
for link in ("https://pobb.in/e404", "https://pobb.in/empty", "https://pobb.in/notacode"):
    steps += [imp("importCodeIn", events=click()), select_all_delete(), paste(link), tapi("importCodeGo"), DELIVER | {"shot": "net-site-error" if link.endswith("e404") else None}]
for s in steps:
    if s.get("shot", 1) is None:
        del s["shot"]
# a good one into a new build: the build comes with its link
GOOD = "https://pobb.in/pob/good"
NET.append(ok(GOOD, CODE))
steps += [
    imp("importCodeIn", events=click()),
    select_all_delete(),
    paste("https://pobb.in/good"),
    tapi("importCodeMode"),
    pick("import", "importCodeMode", 2),
    tapi("importCodeGo", shot="net-site-fetching"),
    DELIVER | {"xml": True},
    {"waitAssets": True},
    AWAY | {"dump": True, "shot": "net-site-imported"},
]
write('net-sites', steps, net + NET)

# A probe outside index.json (POB_IMPORT_SCENES=net-hover-probe): the default view of this golden differs by 17 px from the original on a plain `open`, with no network and wherever the cursor is.
write('net-hover-probe', [
    {"open": "Dual Savior"},
    {"cursor": [616, 552], "dump": True},
    {"waitAssets": True},
    {"dump": True, "shot": "net-hover-probe"},
])
# the same into this build: the confirm popup, then the build
write('net-site-this', [
    {"open": "OccVortex"},
    TAB,
    imp("importCodeIn", events=click()),
    paste("https://pobb.in/good"),
    tapi("importCodeGo"),
    DELIVER | {"shot": "net-site-confirm"},
    {"cursorPopup": "confirm", "events": click(), "dump": True},
    {"dump": True, "xml": True},
], [ok(GOOD, CODE)])
# --- Share: the sites with a postUrl
steps2 = [{"open": "OccVortex"}, TAB, tapi("generateCode")]
share = [
    ("https://pobb.in/pob/", 3, "shared1", 200),
    ("https://poe.ninja/poe1/pob/api/upload", 4, "https://poe.ninja/poe1/pob/xyz", 200),
    ("https://maxroll.gg/poe/api/pob", 1, "shared2", 200),
    ("https://poedb.tw/pob/api/gen", 5, "https://poedb.tw/pob/xyz", 200),
    ("https://api.pob.codes/pob/plain", 2, "shared3", 200),
]
net = []
for url, row, body, code in share:
    net.append({"url": url, "code": code, "header": "", "body": body})
    steps2 += [
        tapi("exportFrom"),
        pick("import", "exportFrom", row),
        tapi("generateCodeByLink", shot="net-share-creating" if row == 3 else None),
        DELIVER,
        tapi("generateCode"),
    ]
for s in steps2:
    if s.get("shot", 1) is None:
        del s["shot"]
# an error of the site: the popup
steps2 += [tapi("exportFrom"), pick("import", "exportFrom", 3)]
net.append({"url": "https://pobb.in/pob/", "code": 500, "header": "", "body": "synthetic error page"})
steps2 += [tapi("generateCodeByLink"), DELIVER | {"shot": "net-share-error"}]
write('net-share', steps2, net)

# --- Party: a build by link
steps = [
    {"open": "party-attack-full"},
    PARTY,
    par("importCodeIn", events=click()),
    paste("https://pobb.in/good"),
    tapp("importCodeGo"),
    DELIVER,
    par("importCodeIn", events=click()),
    select_all_delete(),
    paste("https://pobb.in/e404"),
    tapp("importCodeGo"),
    DELIVER,
]
write('net-party', steps, [ok(GOOD, CODE), fail(ERR_404, 404)])

# --- the tree's links: poeurl both ways
POEURL = "http://poeurl.com/abc"
write('net-tree', [
    {"open": "OccVortex"},
    {"treePopup": "import", "dump": True},
    {"cursorPopup": "name", "events": click()},
    {"events": typed("Linked"), "dump": True},
    {"cursorPopup": "edit", "events": click()},
    {"events": typed(POEURL), "dump": True},
    {"cursorPopup": "import", "events": click(), "dump": True},
    DELIVER,
    {"cursorPopup": "import", "events": click(), "dump": True},
    DELIVER,
    {"cursorPopup": "import", "events": click(), "dump": True},
    DELIVER,
    {"cursorPopup": "cancel", "events": click(), "dump": True},
    {"treePopup": "export", "dump": True},
    {"cursorPopup": "shrink", "events": click(), "dump": True},
    DELIVER,
    {"cursorPopup": "done", "events": click(), "dump": True},
    {"treePopup": "export", "dump": True},
    {"cursorPopup": "shrink", "events": click(), "dump": True},
    DELIVER,
], [
    {"url": POEURL, "code": 200, "header": "HTTP/1.1 200 OK\r\n\r\n", "body": "x"},
    {"url": POEURL, "code": 302, "header": "HTTP/1.1 302 Found\r\nLocation: http://poeurl.com/loop\r\n\r\n", "body": ""},
    {"url": POEURL, "code": 302, "header": "HTTP/1.1 302 Found\r\nLocation: https://www.pathofexile.com/passive-skill-tree/3.29.0/AAAA\r\n\r\n", "body": ""},
    {"url": "http://poeurl.com/shrink.php?url=*", "code": 200, "header": "", "body": "short1"},
    {"url": "http://poeurl.com/shrink.php?url=*", "code": 200, "header": "", "body": "   "},
])

# --- part 3: the characters. The tree: real node ids of the latest tree (public data), the rest is made up.
nodes = {n.get('skill', k) if isinstance(n, dict) else k: n for k, n in TREE['nodes'].items() if isinstance(n, dict)}
start = next(k for k, n in nodes.items() if n.get('classStartIndex') == 3)  # Witch
# a walk from the class's start: `BuildAllDependsAndPaths` takes back what is not connected to it
parent, queue, walk = {start: None}, [start], []
while queue:
    k = queue.pop(0)
    for o in nodes[k].get('out', []) + nodes[k].get('in', []):
        o = int(o)
        n = nodes.get(o)
        if n is None or o in parent or n.get('ascendancyName') or n.get('isMastery') or n.get('classStartIndex') is not None or n.get('isProxy') or n.get('isAscendancyStart'):
            continue
        parent[o] = k
        queue.append(o)
        walk.append(o)


def path_to(n):
    out = []
    while n != start:
        out.append(n)
        n = parent[n]
    return out[::-1]


by_group = {}
for k, n in nodes.items():
    by_group.setdefault(n.get('group'), []).append(k)
# the nearest notable with a mastery in its group
notable, mastery = next((w, m) for w in walk if nodes[w].get('isNotable') for m in by_group[nodes[w]['group']] if nodes[m].get('isMastery') and nodes[m].get('masteryEffects'))
effect = nodes[mastery]['masteryEffects'][0]['effect']
hashes = list(dict.fromkeys(walk[:25] + path_to(notable)))
for w in list(hashes):
    for x in path_to(w):
        if x not in hashes:
            hashes.append(x)
tattoo_node = next(h for h in hashes if not nodes[h].get('isNotable') and not nodes[h].get('isJewelSocket') and not nodes[h].get('isKeystone'))
slots = TREE['jewelSlots']
large = min((i for i, s in enumerate(slots) if nodes[s].get('expansionJewel', {}).get('size') == 2 and s in parent), key=lambda i: len(path_to(slots[i])))
plain = next(i for i, s in enumerate(slots) if 'expansionJewel' not in nodes[s])
large_id = slots[large]
hashes += [h for h in path_to(large_id) if h not in hashes]
proxy = nodes[large_id]['expansionJewel']['proxy']


def prop(name, value):
    return {"name": name, "values": [[value, 1]], "displayMode": 0}


def level_req(v):
    return [{"name": "Level", "values": [[str(v), 0]], "displayMode": 0}]


def gem(type_line, socket, level="20", quality="+10%", **k):
    return dict({"id": f"synthgem{socket}{type_line}", "name": "", "typeLine": type_line, "ilvl": 0, "socket": socket, "frameType": 4,
                 "properties": [{"name": "Level", "values": [[level, 0]]}, {"name": "Quality", "values": [[quality, 1]]}]}, **k)


BODY = {
    "id": "synth-body", "inventoryId": "BodyArmour", "frameType": 2, "name": "Synth Shell", "typeLine": "Vaal Regalia", "ilvl": 84,
    "influences": {"shaper": True, "elder": True}, "corrupted": True,
    "properties": [prop("Quality", "+20%"), prop("Energy Shield", "400")],
    "sockets": [{"group": 0, "sColour": "B"}, {"group": 0, "sColour": "B"}, {"group": 0, "sColour": "B"}, {"group": 1, "sColour": "R"}, {"group": 1, "sColour": "G"}, {"group": 2, "sColour": "W"}],
    "socketedItems": [gem("Arc", 0), gem("Spell Echo Support", 1, "19", "+0%", support=True), gem("Controlled Destruction Support", 2, support=True),
                      gem("Clarity", 3, "5", "+0%"), gem("Enlighten Support", 4, "3", "+0%", support=True), gem("Nonsense Gem", 5)],
    "enchantMods": ["+24 to maximum Energy Shield"],
    "implicitMods": [{"description": "+30 to maximum Life", "flags": {"vestigial": True}}, "+10% to Fire Resistance"],
    "fracturedMods": ["+20 to Strength"],
    "explicitMods": ["120% increased Energy Shield", {"description": "+50 to maximum Mana", "flags": {"crafted": True}}, {"description": "+15% to all Elemental Resistances\n+5 to Intelligence", "flags": {"mutated": True}}, {"description": "+10 to Dexterity", "flags": {"fractured": True}}],
    "craftedMods": ["+12% to Cold Resistance"],
    "mutatedMods": ["+5 to Dexterity"],
}
WEAPON = {
    "id": "synth-wand", "inventoryId": "Weapon", "frameType": 3, "name": "Abberath's Horn", "typeLine": "Goat's Horn", "ilvl": 70,
    "properties": [prop("Quality", "+12%")], "requirements": level_req(30),
    "sockets": [{"group": 0, "sColour": "R"}, {"group": 0, "sColour": "R"}],
    "socketedItems": [gem("Cleave of Rage", 0, hybrid={"baseTypeName": "Cleave of Rage", "isVaalGem": False}), gem("Melee Physical Damage Support", 1, support=True, builtInSupport="Supported by Level 1 Added Fire Damage")],
    "explicitMods": ["Adds 2 to 4 Physical Damage to Attacks"],
}
OFFHAND = {
    "id": "synth-shield", "inventoryId": "Offhand", "frameType": 2, "name": "Synth Guard", "typeLine": "Synthesised Titanium Spirit Shield", "ilvl": 80, "synthesised": True,
    "properties": [prop("Energy Shield", "100")], "explicitMods": ["+20 to maximum Life"],
}
SWAP = {
    "id": "synth-swap", "inventoryId": "Weapon2", "frameType": 1, "name": "", "typeLine": "Heavy Crude Bow of the Bear", "ilvl": 3,
    "properties": [prop("Quality", "+3%")], "sockets": [{"group": 0, "sColour": "G"}], "socketedItems": [gem("Puncture", 0)],
    "explicitMods": ["+5 to Strength"],
}
SWAP2 = {"id": "synth-swap2", "inventoryId": "Offhand2", "frameType": 0, "name": "", "typeLine": "Rawhide Tower Shield", "ilvl": 2, "properties": [prop("Armour", "50")]}
BOOTS = {
    "id": "synth-boots", "inventoryId": "Boots", "frameType": 2, "name": "Synth Tread", "typeLine": "Two-Toned Boots", "ilvl": 84,
    "properties": [prop("Armour", "100"), prop("Evasion Rating", "100")], "explicitMods": ["30% increased Movement Speed"],
}
BOOTS2 = dict(BOOTS, id="synth-boots2", properties=[prop("Evasion Rating", "100"), prop("Energy Shield", "30")])
HELM = {
    "id": "synth-helm", "inventoryId": "Helm", "frameType": 2, "name": "Synth &lt;Crown&gt;", "typeLine": "<<set:MS>><<set:M>><<set:S>>Hubris Circlet", "ilvl": 86, "searing": True, "tangled": True,
    "properties": [prop("Quality (Attack Modifiers)", "+20%"), prop("Energy Shield", "150"), prop("Ward", "10")],
    "scourgeMods": ["+5 to Strength"], "crucibleMods": ["+5 to Dexterity"],
    "explicitMods": ["+100 to maximum Energy Shield"],
    "flavourText": ["<default>{Allocates Sentinel if you have the matching modifier on Forbidden Flame}", "just words"],
}
GLOVES = {
    "id": "synth-gloves", "inventoryId": "Gloves", "frameType": 3, "name": "Synth Mitts", "typeLine": "Vestigial Sorcerer Gloves", "ilvl": 70, "vestigial": True, "split": True, "duplicated": True,
    "properties": [prop("Energy Shield", "60")], "requirements": [{"name": "Class:", "values": [["Witch", 0]]}, {"name": "Level", "values": [["60", 0]]}],
    "explicitMods": ["+10 to Intelligence"], "foilVariation": 3,
}
AMULET = {"id": "synth-amulet", "inventoryId": "Amulet", "frameType": 9, "name": "Synth Relic", "typeLine": "Onyx Amulet", "ilvl": 80, "isRelic": True, "implicitMods": ["+16 to all Attributes"], "explicitMods": ["+1 to Level of all Lightning Skill Gems"]}
RING = {"id": "synth-ring", "inventoryId": "Ring", "frameType": 2, "name": "Synth Loop", "typeLine": "Coral Ring", "ilvl": 80, "implicitMods": ["+25 to maximum Life"], "explicitMods": ["+30 to maximum Mana"]}
RING2 = {"id": None, "inventoryId": "Ring2", "frameType": 1, "name": "", "typeLine": "Sapphire Ring of the Fox", "ilvl": 40, "implicitMods": ["+25% to Cold Resistance"], "explicitMods": ["+5 to Dexterity"]}
BELT = {
    "id": "synth-belt", "inventoryId": "Belt", "frameType": 2, "name": "Synth Cord", "typeLine": "Stygian Vise", "ilvl": 80,
    "sockets": [{"group": 0, "sColour": "A"}],
    "socketedItems": [{"id": "synth-eye", "abyssJewel": True, "frameType": 2, "name": "Synth Eye", "typeLine": "Hypnotic Eye Jewel", "ilvl": 80, "socket": 0, "explicitMods": ["+20 to maximum Mana"]}],
    "implicitMods": ["Has 1 Abyssal Socket"],
}
FLASK = {"id": "synth-flask1", "inventoryId": "Flask", "x": 0, "frameType": 1, "name": "", "typeLine": "Seething Divine Life Flask of Staunching", "ilvl": 60, "properties": [prop("Quality", "+20%")], "explicitMods": ["Removes Bleeding when you use a Flask"]}
FLASK2 = {"id": "synth-flask2", "inventoryId": "Flask", "x": 1, "frameType": 0, "name": "", "typeLine": "Quicksilver Flask", "ilvl": 30, "properties": [prop("Quality", "+5%")]}
IGNORED = {"id": "synth-ignored", "inventoryId": "MainInventory", "frameType": 0, "name": "", "typeLine": "Scroll of Wisdom", "ilvl": 1}
NOBASE = {"id": "synth-nobase", "inventoryId": "Ring3", "frameType": 2, "name": "Synth", "typeLine": "A Base That Does Not Exist", "ilvl": 1}
BLADE = {"id": "synth-blade", "inventoryId": "Weapon", "frameType": 0, "name": "", "typeLine": "Energy Blade", "ilvl": 1, "properties": [{"name": "One Handed Sword", "values": []}], "sockets": [{"group": 0, "sColour": "A"}, {"group": 0, "sColour": "A"}], "explicitMods": ["ignored"], "implicitMods": ["ignored"]}
EQUIPMENT = [BODY, WEAPON, OFFHAND, SWAP, SWAP2, BOOTS, HELM, GLOVES, AMULET, RING, RING2, BELT, FLASK, FLASK2, IGNORED, NOBASE]
GUARDIAN = [
    {"id": "synth-guard-body", "inventoryId": "BodyArmour", "frameType": 2, "name": "Guard Shell", "typeLine": "Astral Plate", "ilvl": 80, "properties": [prop("Armour", "800")], "explicitMods": ["+50 to maximum Life"]},
    {"id": "synth-guard-helm", "inventoryId": "Helm", "frameType": 2, "name": "Guard Helm", "typeLine": "Eternal Burgonet", "ilvl": 80, "properties": [prop("Armour", "300")], "explicitMods": ["+20 to maximum Life"]},
]
JEWELS = [
    {"id": "synth-jewel", "inventoryId": "PassiveJewels", "x": plain, "frameType": 2, "name": "Synth Spark", "typeLine": "Cobalt Jewel", "ilvl": 80, "explicitMods": ["10% increased maximum Energy Shield"]},
    {"id": "synth-unique-jewel", "inventoryId": "PassiveJewels", "x": next(i for i, s in enumerate(slots) if 'expansionJewel' not in nodes[s] and i != plain), "frameType": 3, "name": "Thread of Hope", "typeLine": "Crimson Jewel", "ilvl": 80,
     "properties": [prop("Radius", "Variable"), prop("Limited to", "1")], "explicitMods": ["Only affects Passives in Medium Ring", "Passives in Radius can be Allocated without being connected to your tree", "-20% to all Elemental Resistances"]},
    {"id": "synth-cluster", "inventoryId": "PassiveJewels", "x": large, "frameType": 2, "name": "Synth Cluster", "typeLine": "Large Cluster Jewel", "ilvl": 84,
     "enchantMods": ["Adds 8 Passive Skills", "2 Added Passive Skills are Jewel Sockets", "Added Small Passive Skills grant: 12% increased Fire Damage"], "explicitMods": ["1 Added Passive Skill is Prismatic Heart", "1 Added Passive Skill is Widespread Destruction"]},
]
GROUP = f"expansion_{large_id}"
# BuildSubgraph turns a template's index into the tree's orbit space from the proxy's own index: these are the ones a graph on this socket has
PROXY_OIDX = nodes[int(proxy)]['orbitIndex']
JEWEL_DATA = {str(large): {"type": "JewelPassiveTreeExpansionLarge", "subgraph": {
    "groups": {GROUP: {"proxy": proxy, "nodes": ["70001", "70002", "70003"]}},
    "nodes": {"70001": {"group": GROUP, "orbitIndex": PROXY_OIDX, "skill": "70001"}, "70002": {"group": GROUP, "orbitIndex": (PROXY_OIDX + 2) % 12, "skill": "70002"}, "70003": {"group": GROUP, "orbitIndex": (PROXY_OIDX + 1) % 12, "isMastery": True, "skill": "70003"}},
}}}


def passives(**k):
    d = {
        "hashes": hashes + [mastery], "hashes_ex": [70001, 70002], "mastery_effects": {str(mastery): effect},
        "skill_overrides": {str(tattoo_node): {"name": "Tattoo of the Tawhoa Shaman", "icon": "Art/2DArt/SkillIcons/passives/LifeRecoupNode.png",
                                               "activeEffectImage": "Art/2DArt/UIImages/InGame/AncestralTrial/PassiveTreeTattoos/TawhoaTribePassiveBG.png", "isTattoo": True, "stats": ["5% chance to Poison on Hit"]}},
        "bandit_choice": "Alira", "pantheon_major": "Lunaris", "pantheon_minor": "Shakari", "jewel_data": JEWEL_DATA, "alternate_ascendancy": "Farrul",
    }
    d.update(k)
    return d


def character(**k):
    d = {"id": "synthchar", "name": "Synth Char", "realm": "pc", "class": "Necromancer", "league": "Standard", "level": 85, "experience": 1, "equipment": EQUIPMENT, "jewels": JEWELS, "passives": passives(), "guardian": GUARDIAN}
    d.update(k)
    return d


def j(v):
    return json.dumps(v, ensure_ascii=False, separators=(',', ':'))


# the JSON of a character (dev mode, ImportTab.lua:702-705), over an empty build and over a golden
def dev_scene(name, open_step, char, shot=None):
    steps = [open_step, {"devMode": True}, TAB, imp("importCodeIn", events=click()), paste(j({"character": char})), tapi("importCodeGo", perfClick="charImport"), {"dump": True, "xml": True}]
    if shot:
        steps[-1]["shot"] = shot
    return write(name, steps)


dev_scene('char-new', {"openNew": True}, character(), shot="char-new-tree")
dev_scene('char-golden', {"open": "OccVortex"}, character())
dev_scene('char-level-low', {"openNew": True}, character(level=12, passives=passives(hashes=hashes[:6], hashes_ex=[], jewel_data={}, skill_overrides={}, mastery_effects={}, alternate_ascendancy=None, bandit_choice="Eramir"), jewels=[], equipment=[RING], guardian=[]))
dev_scene('char-ruthless', {"openNew": True}, character(league="Ruthless Standard", jewels=[], passives=passives(hashes=hashes[:8], hashes_ex=[], jewel_data={}, skill_overrides={}, mastery_effects={}, alternate_ascendancy=2), equipment=[BOOTS2, BLADE], guardian=[]))
dev_scene('char-class-only', {"openNew": True}, character(**{"class": "Marauder"}, jewels=[], passives=passives(hashes=[], hashes_ex=[], jewel_data={}, skill_overrides={}, mastery_effects={}, alternate_ascendancy=None, bandit_choice=None, pantheon_major=None, pantheon_minor=None), equipment=[], guardian=[]))
# twice in a row: the second one replaces by `id` and restores what a re-import keeps. No abyss jewel here: `clearItems` walks `pairs(self.slots)` (R13), and whether the belt or its socket comes first decides whether
# the jewel survives (`DeleteItem`'s `PopulateSlots` empties the socket): 4 processes of the original, 2 orders of `itemOrderList`.
steps = [{"openNew": True}, {"devMode": True}, TAB, imp("importCodeIn", events=click()), paste(j({"character": character(equipment=[e for e in EQUIPMENT if e is not BELT])})), tapi("importCodeGo"), {"dump": True, "xml": True}, tapi("importCodeGo"), {"dump": True, "xml": True}]
write('char-twice', steps)

# --- by account name: every stage, every error
HOST = "https://www.pathofexile.com/"
ACCOUNT = "Synth#1234"
LIST = f"{HOST}character-window/get-characters?accountName=Synth%231234&realm=pc"
PROFILE = f"{HOST}account/view-profile/Synth%231234"
PAGE = '<a href="/account/view-profile/Synth-1234/characters">x</a>'
CHARS = [
    {"name": "Synth Char", "realm": "pc", "class": "Necromancer", "league": "Standard", "level": 85},
    {"name": "Another", "realm": "pc", "class": "Juggernaut", "league": "Hardcore", "level": 40},
    {"name": "bystander", "realm": "pc", "class": "Witch", "league": "Standard", "level": 3},
    {"name": "Elsewhere", "realm": "xbox", "class": "Witch", "league": "Standard", "level": 3},
    {"name": "Odd One", "realm": "pc", "class": "Nonsense", "league": "Standard", "level": 3},
]
TREE_URL = f"{HOST}character-window/get-passive-skills?accountName=Synth%231234&character=bystander&realm=pc"
ITEMS_URL = f"{HOST}character-window/get-items?accountName=Synth%231234&character=bystander&realm=pc"
TREE_URL2 = TREE_URL.replace("bystander", "Another")
ITEMS_URL2 = ITEMS_URL.replace("bystander", "Another")
SITE_PASSIVES = passives(items=JEWELS, alternate_ascendancy=2, bandit_choice=None, pantheon_major=None, pantheon_minor=None)
del SITE_PASSIVES["items"]
SITE_PASSIVES["items"] = JEWELS
START = [imp("siteAccountName", events=click()), {"events": typed(ACCOUNT), "dump": True}, tapi("siteAccountNameGo")]
write('net-account', [
    {"openNew": True},
    TAB,
    *START,
    DELIVER,
    DELIVER | {"shot": "net-account-chars"},
    tapi("siteCharSelectLeague"),
    pick("import", "siteCharSelectLeague", 3),
    tapi("siteCharSelectLeague"),
    pick("import", "siteCharSelectLeague", 2),
    AWAY | {"events": click(), "dump": True},
    tapi("siteCharImportItems"),
    DELIVER | {"xml": True, "shot": "net-account-items"},
    tapi("siteCharImportTree"),
    DELIVER | {"xml": True},
    tapi("siteCharClose"),
], [
    ok(LIST, j(CHARS)),
    ok(PROFILE, PAGE),
    ok(ITEMS_URL, j({"items": EQUIPMENT, "guardian": GUARDIAN})),
    ok(TREE_URL, j(SITE_PASSIVES)),
])
# the tree over a tree: the confirm popup; the answers that stop it
write('net-account-confirm', [
    {"open": "OccVortex"},
    TAB,
    *START,
    DELIVER,
    DELIVER,
    tapi("siteCharImportTree"),
    {"cursorPopup": "confirm", "events": click(), "dump": True},
    DELIVER,
    tapi("siteCharImportItems"),
    DELIVER,
    tapi("siteCharImportTree"),
    {"cursorPopup": "confirm", "events": click(), "dump": True},
    DELIVER | {"xml": True},
], [
    ok(LIST, j(CHARS)),
    ok(PROFILE, PAGE),
    err(TREE_URL2, "synthetic\nfailure"),
    ok(ITEMS_URL2, "false"),
    ok(TREE_URL2, j(SITE_PASSIVES)),
])
steps = [{"openNew": True}, TAB]
net = []
for reply in (fail(LIST, 401), fail(LIST, 403), fail(LIST, 404), fail(LIST, 500), err(LIST, "synthetic\nerror"), ok(LIST, "not json"), ok(LIST, "[]"), ok(LIST, j(CHARS))):
    net.append(reply)
    steps += [*(START if len(steps) == 2 else [tapi("siteAccountNameGo")]), DELIVER]
net += [fail(PROFILE, 500)]
steps += [DELIVER]
write('net-account-errors', steps, net)

# --- OAuth: the login, the lists, the characters, the failures
TOKEN = f"{HOST}oauth/token"
API = "https://api.pathofexile.com/character"
TOKENS = j({"access_token": "synthetic-access", "refresh_token": "synthetic-refresh", "expires_in": 3600})
LIMITS = "HTTP/1.1 200 OK\r\nX-Rate-Limit-Policy: character-list-request-limit\r\nX-Rate-Limit-Rules: Account\r\nX-Rate-Limit-Account: 5:10:60\r\nX-Rate-Limit-Account-State: 1:10:0\r\n\r\n"
OC = [dict(c, id=f"id{i}") for i, c in enumerate(CHARS)]
CLOCK = {"osTime": 1800000000}
write('net-oauth', [
    {"openNew": True},
    CLOCK,
    TAB,
    tapi("authenticateButton", clipboard="fixed", shot="net-oauth-logging-in"),
    {"netDeliver": True, "clipboard": "fixed", "dump": True},
    DELIVER,
    DELIVER | {"shot": "net-oauth-fetched"},
    tapi("charSelectLeague"),
    pick("import", "charSelectLeague", 3),
    tapi("charImportItems"),
    DELIVER | {"xml": True},
    tapi("charImportTree"),
    DELIVER | {"xml": True, "shot": "net-oauth-imported"},
    tapi("charImportTree"),
    {"cursorPopup": "confirm", "events": click(), "dump": True},
    DELIVER | {"xml": True},
    tapi("accountRealm"),
    pick("import", "accountRealm", 2),
    tapi("accountRealmFetchButton"),
    DELIVER,
    tapi("logoutApiButton"),
], [
    {"url": "oauth", "oauth": {"code": "synthetic-code", "port": 49082}},
    ok(TOKEN, TOKENS),
    ok(API, j({"characters": OC}), LIMITS),
    ok(f"{API}/Another", j({"character": character()})),
    ok(f"{API}/xbox", j({"characters": [dict(CHARS[3], id="x")]}), LIMITS.replace("character-list", "character-list")),
])
# the failures: a state that does not match, a server that says no, a 401 with a retry, a dead refresh, the rate limit
write('net-oauth-fail', [
    {"openNew": True},
    CLOCK,
    TAB,
    tapi("authenticateButton", clipboard="fixed"),
    {"netDeliver": True, "clipboard": "fixed", "dump": True},
    tapi("authenticateButton", clipboard="fixed"),
    {"netDeliver": True, "clipboard": "fixed", "dump": True},
    DELIVER,
    AWAY | {"osTime": 1800000100, "dump": True},
], [
    {"url": "oauth", "oauth": {"code": "synthetic-code", "state": "wrong", "port": 49082}},
    {"url": "oauth", "oauth": {"code": "synthetic-code", "port": 49082}},
    err(TOKEN, "synthetic refused"),
])
HAVE_TOKEN = {"apiTokens": {"auth": "synthetic-access", "refresh": "synthetic-refresh", "expiry": 1900000000}}
LIMITED = "HTTP/1.1 429 Too Many\r\nRetry-After: 30\r\nX-Rate-Limit-Policy: character-list-request-limit\r\n\r\n"
write('net-oauth-api', [
    {"openNew": True},
    CLOCK,
    HAVE_TOKEN,
    TAB,
    tapi("accountRealmFetchButton"),
    DELIVER,
    DELIVER,
    AWAY | {"osTime": 1800000031, "dump": True},
    tapi("accountRealmFetchButton"),
    DELIVER,
    DELIVER,
    DELIVER,
    tapi("charSelect"),
    AWAY | {"events": click(), "dump": True},
    tapi("charImportItems"),
    DELIVER,
    DELIVER,
    AWAY | {"dump": True},
], [
    fail(API, 401),
    {"url": API, "code": 200, "header": LIMITED, "body": "{}"},
    fail(API, 500),
    ok(API, j({"characters": OC}), LIMITS),
    fail(f"{API}/Another", 500),
])
write('net-oauth-expired', [
    {"openNew": True},
    CLOCK,
    {"apiTokens": {"auth": "synthetic-access", "refresh": "synthetic-refresh", "expiry": 1}},
    TAB,
    tapi("accountRealmFetchButton"),
    DELIVER,
    DELIVER,
    {"apiTokens": {"auth": "synthetic-access", "refresh": "synthetic-refresh", "expiry": 1}},
    AWAY | {"dump": True},
    tapi("accountRealmFetchButton"),
    DELIVER,
    AWAY | {"dump": True},
], [
    ok(TOKEN, TOKENS),
    ok(API, j({"characters": OC}), LIMITS),
    err(TOKEN, "synthetic refused"),
])

idx = json.load(open(f'{OUT}/index.json'))
names = write.__defaults__[1]
for n in names:
    if n not in idx and n != 'net-hover-probe':
        idx.append(n)
json.dump(idx, open(f'{OUT}/index.json', 'w', newline='\n'))
print(names)
