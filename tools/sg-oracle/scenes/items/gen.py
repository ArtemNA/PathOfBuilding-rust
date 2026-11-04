import json, os
os.makedirs('tools/sg-oracle/scenes/items', exist_ok=True)
def click(k='LEFTBUTTON'): return [["KeyDown", k], ["KeyUp", k]]
def write(name, steps):
    out = []
    for s in steps:
        # the runner lets a step's `release` go before its events
        if 'hold' in s and 'release' in s and 'events' in s:
            s = dict(s)
            rel = s.pop('release')
            out.append(s)
            out.append({'release': rel})
        else:
            out.append(s)
    json.dump({"steps": out}, open(f'tools/sg-oracle/scenes/items/{name}.json', 'w'), indent=0)
RING = "Rarity: RARE\nU6 Loop\nCoral Ring\n+25 to maximum Life\n+10% to Fire Resistance\n"
tab = {"cursorBuild": ["modeItems"], "events": click(), "dump": True}
away = {"cursor": [1200, 790]}
# the tab: list, hover on items and slots, sets of one item
write('base', [
 {"open": "Dual Wield Cospris CoC"},
 tab | {"shot": "base-tab"},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": ["slot:Helmet"], "dump": True, "shot": "base-slot-tip"},
 {"cursorItemRow": ["itemList", 2], "dump": True, "shot": "base-row-tip"},
 {"cursorItemRow": ["itemList", 3], "dump": True},
 {"cursorItem": ["slot:Boots"], "dump": True},
 {"cursorItem": ["slot:Weapon 1"], "dump": True},
 {"cursorItem": ["weaponSwap2"], "events": click(), "dump": True},
 {"cursorItem": ["slot:Weapon 1 Swap"], "dump": True},
 {"cursorItem": ["weaponSwap1"], "events": click(), "dump": True},
 {"cursorItem": ["slot:Helmet"], "events": [["KeyDown", "LEFTBUTTON"], ["KeyUp", "LEFTBUTTON"]], "dump": True, "shot": "base-slot-drop"},
 {"cursor": [1200, 790], "events": [["KeyDown", "ESCAPE"]], "dump": True},
 {"cursorItemRow": ["uniqueDB", 3], "dump": True, "shot": "base-db-tip"},
 {"closeBuild": True},
])
# display item: double click, range line, add, delete, undo/redo, save
write('display', [
 {"open": "Dual Wield Cospris CoC"},
 tab,
 {"cursorItemRow": ["itemList", 4], "events": click()},
 {"events": [["KeyDown", "LEFTBUTTON", True], ["KeyUp", "LEFTBUTTON"]], "dump": True, "shot": "display-open"},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": ["removeDisplayItem"], "events": click(), "dump": True},
 {"cursor": [1200, 790], "clipboard": RING, "hold": ["CTRL"], "events": [["KeyDown", "v"], ["KeyUp", "v"]], "release": ["CTRL"], "dump": True, "shot": "display-paste"},
 {"cursorItem": ["addDisplayItem"], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"hold": ["CTRL"], "events": [["KeyDown", "z"], ["KeyUp", "z"]], "release": ["CTRL"], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"hold": ["CTRL"], "events": [["KeyDown", "y"], ["KeyUp", "y"]], "release": ["CTRL"], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"hold": ["CTRL"], "events": [["KeyDown", "d"], ["KeyUp", "d"]], "release": ["CTRL"], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "release": ["CTRL"], "dump": True, "file": "Dual Wield Cospris CoC.xml", "fileOut": "display-save.xml"},
 {"closeBuild": True},
])
# equip through the list and the database, set manage popup
write('equip', [
 {"open": "OccVortex"},
 tab,
 {"cursorItemRow": ["uniqueDB", 5], "hold": ["CTRL"], "events": click(), "release": ["CTRL"], "dump": True, "perfClick": "equipDB"},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": ["setManage"], "events": click(), "dump": True},
 {"cursorPopup": ["close"], "events": click(), "dump": True},
 {"cursorItemRow": ["itemList", 1], "hold": ["CTRL"], "events": click(), "release": ["CTRL"], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": ["uniqueDB"], "dump": True},
 {"closeBuild": True},
])
# an item with a granted skill: `calcFunc({ repSlotName, repItem = nil })` keeps its socket group upstream (CalcSetup.lua:1468 is MAIN only)
write('granted', [
 {"open": "Dual Wield Cospris CoC"},
 tab,
 {"cursorItem": ["slot:Ring 1"], "dump": True},
 {"closeBuild": True},
])
AMULET = "Rarity: RARE\nU6 Beads\nJade Amulet\nLevel: 80\n+12 to Strength\n"
HELMET = "Rarity: RARE\nU6 Crown\nIron Hat\nArmour: 10\n+20 to maximum Life\n"
UNIQUE_VARIANT = "Rarity: UNIQUE\nU6 Test\nCoral Ring\nVariant: One\nVariant: Two\nSelected Variant: 1\n{variant:1}+10 to Strength\n{variant:2}+(10-30) to Dexterity\n"
CLUSTER = "Rarity: RARE\nU6 Cluster\nLarge Cluster Jewel\nCrafted: true\nAdds 8 Passive Skills\n"
def paste(text): return {"cursor": [1200, 790], "clipboard": text, "hold": ["CTRL"], "events": [["KeyDown", "v"], ["KeyUp", "v"]], "release": ["CTRL"], "dump": True}
def tap(c): return {"cursorItem": [c], "events": click(), "dump": True}
def ptap(c, **k): return {"cursorPopup": [c], "events": click(), "dump": True, **k}
def ppick(c, i): return {"cursorPopupPick": [c, i], "events": click(), "dump": True}
write('editor', [
 {"open": "Dual Savior"},
 tab,
 paste(AMULET) | {"shot": "editor-amulet"},
 tap("displayItemAnoint") | {"shot": "editor-anoint"},
 {"cursorPopupRow": ["notableDB", 3], "dump": True, "shot": "editor-anoint-tip"},
 {"cursorPopupRow": ["notableDB", 3], "events": click(), "dump": True},
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 tap("displayItemAddCustom"),
 ptap("source"),
 ppick("source", 2),
 ptap("modSelect"),
 {"cursorPopupPick": ["modSelect", 2], "dump": True},
 {"cursorPopupPick": ["modSelect", 2], "events": click(), "dump": True},
 ptap("save"),
 {"cursor": [1200, 790], "dump": True, "shot": "editor-custom"},
 tap("displayItemCorrupt"),
 ptap("implicit1"),
 ppick("implicit1", 2),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 tap("addDisplayItem"),
 {"cursor": [1200, 790], "dump": True},
 paste(HELMET),
 tap("displayItemEnchant"),
 ptap("enchantment"),
 ppick("enchantment", 3),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True, "shot": "editor-enchant"},
 tap("editDisplayItem") | {"shot": "editor-edit"},
 ptap("cancel"),
 tap("removeDisplayItem"),
 {"cursor": [1200, 790], "dump": True},
 tap("craftDisplayItem") | {"shot": "editor-craft"},
 ptap("type"),
 ppick("type", 5),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 tap("displayItemAffix1"),
 {"cursorPick": ["displayItemAffix1", 3], "events": click(), "dump": True, "shot": "editor-affix"},
 {"cursor": [1200, 790], "dump": True},
 paste(UNIQUE_VARIANT),
 tap("displayItemVariant"),
 {"cursorPick": ["displayItemVariant", 2], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True, "shot": "editor-variant"},
 paste(CLUSTER) | {"shot": "editor-cluster"},
 {"closeBuild": True},
])
write('loadout', [
 {"open": "OccVortex"},
 tab,
 {"cursorBuild": ["buildLoadouts"], "events": click(), "dump": True},
 {"cursorBuildPick": ["buildLoadouts", 4], "events": click(), "dump": True},
 {"cursorPopup": ["edit"], "events": [["KeyDown", "LEFTBUTTON"], ["KeyUp", "LEFTBUTTON"], ["Char", "2"]], "dump": True},
 {"cursorPopup": ["save"], "events": click(), "dump": True, "shot": "loadout-new"},
 {"cursor": [1200, 790], "dump": True},
 {"cursorBuild": ["buildLoadouts"], "events": click(), "dump": True},
 {"cursorBuildPick": ["buildLoadouts", 3], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursor": [1200, 790], "dump": True, "shot": "loadout-switched"},
 {"cursorItem": ["setManage"], "events": click(), "dump": True},
 {"cursorPopup": ["close"], "events": click(), "dump": True},
 {"cursor": [1200, 790], "hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "release": ["CTRL"], "dump": True, "file": "OccVortex.xml", "fileOut": "loadout-save.xml"},
 {"closeBuild": True},
])
# a granted skill's group through taking the item off and on: the flag does not come back, the new group is a new table
write('granted-group', [
 {"open": "Dual Wield Cospris CoC"},
 {"cursorBuild": ["modeSkills"], "events": click()},
 {"cursorSkillRow": 0, "hold": ["CTRL"], "events": [["KeyDown", "RIGHTBUTTON"], ["KeyUp", "RIGHTBUTTON"]]},
 {"release": ["CTRL"]},
 {"cursor": [1200, 790]},
 tab,
 {"cursor": [1200, 790], "hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "release": ["CTRL"], "dump": True, "file": "Dual Wield Cospris CoC.xml", "fileOut": "granted-group-1.xml"},
 {"cursorItem": ["slot:Ring 1"], "events": click(), "dump": True},
 {"cursorPick": ["slot:Ring 1", 1], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"hold": ["CTRL"], "events": [["KeyDown", "z"], ["KeyUp", "z"]], "dump": True},
 {"release": ["CTRL"]},
 {"cursor": [1200, 790], "dump": True},
 {"cursor": [1200, 790], "hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "release": ["CTRL"], "dump": True, "file": "Dual Wield Cospris CoC.xml", "fileOut": "granted-group-2.xml"},
 {"closeBuild": True},
])
TINCTURE = "Rarity: MAGIC\nAshbark Tincture\n"
FLASK = "Rarity: MAGIC\nDivine Life Flask\n"
CRAFTED_CLUSTER = "Rarity: RARE\nU6 Knot\nLarge Cluster Jewel\nCrafted: true\n"
MUTATED = "Rarity: UNIQUE\nFoulborn Mageblood\nHeavy Belt\nImplicits: 1\n+(30-50) to Strength\n+(30-50) to Dexterity\n+(15-25)% to Fire Resistance\n+(15-25)% to Cold Resistance\nMagic Utility Flasks cannot be Used\nMagic Utility Flask Effects cannot be removed\n{mutated}Rightmost 4 Magic Utility Flasks constantly apply their Flask Effects to you\n"
write('coverage', [
 {"open": "Dual Wield Cospris CoC"},
 tab,
 {"cursorItem": ["slot:Flask 1"], "dump": True, "shot": "cover-flask-tip"},
 {"cursorChild": ["slot:Flask 1", "activate"], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursorChild": ["slot:Flask 1", "activate"], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 paste(FLASK) | {"shot": "cover-flask"},
 paste(TINCTURE) | {"shot": "cover-tincture"},
 paste(MUTATED) | {"shot": "cover-mutated"},
 {"cursorItem": ["removeDisplayItem"], "events": click()},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItemRow": ["uniqueDB", 11], "events": click()},
 {"events": [["KeyDown", "LEFTBUTTON", True], ["KeyUp", "LEFTBUTTON"]], "dump": True},
 {"cursor": [1200, 790], "dump": True, "shot": "cover-foulborn-check"},
 paste(CRAFTED_CLUSTER),
 {"cursorItem": ["displayItemClusterJewelSkill"], "events": click(), "dump": True},
 {"cursorPick": ["displayItemClusterJewelSkill", 2], "events": click(), "dump": True, "shot": "cover-cluster"},
 {"cursorItem": ["displayItemClusterJewelNodeCount", 40, 0], "events": click(), "dump": True},
 {"cursorItem": ["removeDisplayItem"], "events": click()},
 {"cursor": [1200, 790], "dump": True},
 {"cursorBuild": ["modeSkills"], "events": click()},
 {"cursorSkillRow": 1, "events": click(), "dump": True},
 {"cursorSkillCtl": "optimiseSockets", "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursorBuild": ["modeTree"], "events": click()},
 {"waitAssets": True},
 {"treeOrder": "coverage-order"},
 {"cursorNode": 33989, "dump": True, "treeTip": True},
 {"events": click("RIGHTBUTTON"), "dump": True},
 {"cursor": [1200, 790], "dump": True, "shot": "cover-socket-focus"},
 {"closeBuild": True},
])
_tl = json.load(open('tools/sg-oracle/scenes/tree/tab-timeless.json'))['steps'][:54]
_tl = [{k: v for k, v in x.items() if k not in ('dump', 'shot', 'tab', 'perfClick', 'file', 'fileOut')} for x in _tl if 'treeOrder' not in x]
write('timeless', _tl + [
 {"cursor": [800, 470], "dump": True},
 {"cursor": [800, 470]},
 {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON", True]]},
 {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]], "dump": True},
 {"cursor": [-100, -100], "dump": True},
 {"closeBuild": True},
])
# R-abyss-conquest: Abyss jewels through the tab on the converted golden: the comparison in the sockets (ItemsTab.lua:4216, :4984), the conquered tree, the node tooltips
TECROD = "Rarity: UNIQUE\nFestering Vengeance\nMurderous Eye Jewel\nLeague: Abyss\nLimited to: 1 Historic\nImplicits: 0\nSubjugating 4000 souls in the thrall of Tecrod\nPassives affected are Conquered by the Abyssal\nHistoric\n"
ZORATH = "Rarity: UNIQUE\nReclaimed Malevolence\nAssembled Eye Jewel\nLeague: Abyss\nLimited to: 1 Historic\nImplicits: 0\nBinding 4000 souls to phylacteries to sustain Zorath\nPassives affected are Conquered by the Abyssal\nHistoric\n"
ABYSS_SOCKET = "slot:Jewel 26725"
ABYSS_ROW_TECROD, ABYSS_ROW_ZORATH = 7, 8 # their rows in the socket's list
def press(x, y): return [{"cursor": [x, y]}, {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON"]]}, {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]]}]
def wheel(k, n): return [["KeyUp", k]] * n
write('abyss', [
 {"open": "Generals Perforate Zerker"},
 {"waitAssets": True},
 *press(971, 730), # Copy + Convert to the latest
 {"waitAssets": True},
 {"cursor": [-100, -100], "events": [["KeyDown", "RETURN"], ["KeyUp", "RETURN"]]},
 tab,
 paste(TECROD),
 {"cursor": [1200, 790], "dump": True, "shot": "abyss-display"}, # the comparison in every socket: wider than the tab, which snaps to its end (ItemsTab.lua:1502)
 {"cursor": [800, 600], "events": wheel("WHEELUP", 80), "dump": True},
 {"cursor": [800, 600]}, # a control's place follows the scroll one frame later
 tap("addDisplayItem"),
 {"cursor": [1200, 790], "dump": True},
 # no shots over the socket list: its preview of the tree covers text, so a lossy pixel of the preview (delta 3) counts as text in the pixel gate; the tooltips are in the states
 {"cursorItem": [ABYSS_SOCKET], "dump": True},
 {"cursorItem": [ABYSS_SOCKET], "events": click(), "dump": True},
 {"cursorPick": [ABYSS_SOCKET, ABYSS_ROW_TECROD], "dump": True},
 {"cursorPick": [ABYSS_SOCKET, ABYSS_ROW_TECROD], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": [ABYSS_SOCKET], "dump": True},
 {"cursorBuild": ["modeTree"], "events": click()},
 {"waitAssets": True},
 {"treeOrder": "abyss-order"},
 {"cursor": [796, 388], "events": wheel("WHEELDOWN", 5)},
 # the golden's saved view is off the tree: drag it back
 {"cursor": [800, 400]},
 {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON"]]},
 {"cursor": [602, 103]},
 {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]]},
 {"cursorNode": 33287, "events": wheel("WHEELUP", 4)},
 {"cursor": [-100, -100], "dump": True, "shot": "abyss-tree"},
 {"cursorNode": 33287, "dump": True, "treeTip": True, "shot": "abyss-tip-notable"},
 {"cursorNode": 3644, "dump": True, "treeTip": True, "shot": "abyss-tip-normal"},
 {"cursorNode": 57279, "dump": True, "treeTip": True, "shot": "abyss-tip-keystone"},
 {"cursorNode": 26725, "dump": True, "treeTip": True, "shot": "abyss-tip-socket"},
 {"cursor": [-100, -100], "dump": True},
 tab,
 paste(ZORATH),
 {"cursor": [1200, 790], "dump": True, "shot": "abyss-display-zorath"},
 {"cursor": [800, 600], "events": wheel("WHEELUP", 80), "dump": True},
 {"cursor": [800, 600]}, # a control's place follows the scroll one frame later
 tap("addDisplayItem"),
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": [ABYSS_SOCKET], "events": click(), "dump": True},
 {"cursorPick": [ABYSS_SOCKET, ABYSS_ROW_ZORATH], "dump": True},
 {"cursorPick": [ABYSS_SOCKET, ABYSS_ROW_ZORATH], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursorBuild": ["modeTree"], "events": click()},
 {"waitAssets": True},
 {"cursor": [796, 388], "events": wheel("WHEELDOWN", 4)},
 {"cursor": [-100, -100], "dump": True, "shot": "abyss-zorath-tree"},
 {"cursorNode": 9271, "events": wheel("WHEELUP", 3)},
 {"cursorNode": 9271, "dump": True, "treeTip": True, "shot": "abyss-zorath-ascend"},
 {"cursor": [-100, -100], "dump": True},
 {"closeBuild": True},
])
# U10: AddImplicit, Crucible, Corrupt (roll ranges, Scourge, Glimpse, Sort by), the restrictions of CanHaveMod, the drags of the sets and of mainSkillMinion
EXARCH_HELMET = "Rarity: RARE\nU10 Crown\nIron Hat\nSearing Exarch Item\nEater of Worlds Item\nArmour: 10\n+20 to maximum Life\n"
PLAIN_HELMET = "Rarity: RARE\nU10 Crown\nIron Hat\nArmour: 10\n+20 to maximum Life\n"
VEST_UNIQUE = "Rarity: UNIQUE\nGoldrim\nLeather Cap\nEvasion: 33\n+(30-50) to Evasion Rating\n10% increased Rarity of Items found\n+(30-40)% to all Elemental Resistances\nReflects 4 Physical Damage to Melee Attackers\n"
ROLL_UNIQUE = "Rarity: UNIQUE\nU10 Rolls\nCoral Ring\nImplicits: 0\n+(10-30) to Dexterity\n+(20-40) to maximum Life\nThis fixed line has no roll\n(10-20)% increased Fire Damage and a long description that goes on and on until the wrap of the popup has to break the line in two\n"
WEAPON = "Rarity: RARE\nU10 Blade\nCorsair Sword\nQuality: 20\n+20 to Strength\n"
HELMET_TREE = "Rarity: RARE\nU10 Tree\nIron Hat\nArmour: 10\nHas a Crucible Passive Skill Tree\n"
NO_CASTER = "Rarity: RARE\nU10 Restrict\nCoral Ring\nCrafted: true\nCannot roll Caster Modifiers\n"
NO_ATTACK = "Rarity: RARE\nU10 Restrict\nCoral Ring\nCrafted: true\nCannot roll Attack Modifiers\n"
ONLY_COLD = "Rarity: RARE\nU10 Restrict\nCoral Ring\nCrafted: true\nCannot roll Modifiers of Non-Cold Damage Types\n"
TWO_TYPES = "Rarity: RARE\nU10 Restrict\nCoral Ring\nCrafted: true\nCannot roll Modifiers of Non-Cold Damage Types\nCannot roll Modifiers of Non-Fire Damage Types\n"
def ptyped(c, text): return {"cursorPopup": [c], "events": click() + [["Char", ch] for ch in text], "dump": True}
write('u10-implicit', [
 {"open": "OccVortex"},
 tab,
 paste(EXARCH_HELMET),
 tap("displayItemAddImplicit") | {"shot": "u10-implicit-exarch"},
 ptap("source"),
 ppick("source", 2),
 ptap("modGroupSelect"),
 {"cursorPopupPick": ["modGroupSelect", 3], "dump": True},
 {"cursorPopupPick": ["modGroupSelect", 3], "events": click(), "dump": True},
 ptap("modSelect"),
 {"cursorPopupPick": ["modSelect", 2], "dump": True},
 {"cursorPopupPick": ["modSelect", 2], "events": click(), "dump": True},
 {"cursorPopup": ["save"], "dump": True, "shot": "u10-implicit-add-tip"},
 ptap("sort"),
 ppick("sort", 3),
 ptap("source"),
 ppick("source", 1),
 ptap("source"),
 ppick("source", 3),
 ptyped("custom", "+1 to Level of all Spell Skill Gems"),
 {"cursorPopup": ["save"], "dump": True},
 ptap("save"),
 {"cursor": [1200, 790], "dump": True, "shot": "u10-implicit-custom"},
 tap("displayItemAddImplicit"),
 ptap("close"),
 paste(EXARCH_HELMET),
 tap("displayItemAddImplicit"),
 {"cursorPopupPick": ["source", 1]},
 ptap("modGroupSelect"),
 ppick("modGroupSelect", 2),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 tap("displayItemAddImplicit"),
 ptap("modGroupSelect"),
 ppick("modGroupSelect", 4),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 paste(PLAIN_HELMET),
 tap("displayItemAddImplicit") | {"shot": "u10-implicit-custom-only"},
 ptap("close"),
 paste(VEST_UNIQUE),
 tap("displayItemAddImplicit") | {"shot": "u10-implicit-vestigial"},
 ptap("modGroupSelect"),
 {"cursorPopupPick": ["modGroupSelect", 2], "dump": True},
 {"cursorPopupPick": ["modGroupSelect", 2], "events": click(), "dump": True},
 ptap("sort"),
 ppick("sort", 3),
 {"cursorPopup": ["save"], "dump": True},
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 {"closeBuild": True},
])
write('u10-crucible', [
 {"open": "OccVortex"},
 tab,
 paste(WEAPON),
 tap("displayItemAddCrucible") | {"shot": "u10-crucible-popup"},
 ptap("modSelectNode1"),
 {"cursorPopupPick": ["modSelectNode1", 3], "dump": True},
 {"cursorPopupPick": ["modSelectNode1", 3], "events": click(), "dump": True},
 ptap("modSelectNode3"),
 ppick("modSelectNode3", 4),
 {"cursorPopup": ["save"], "dump": True},
 ptap("save"),
 {"cursor": [1200, 790], "dump": True, "shot": "u10-crucible-added"},
 tap("displayItemAddCrucible"),
 ptap("close"),
 paste(HELMET_TREE),
 tap("displayItemAddCrucible"),
 ptap("close"),
 {"closeBuild": True},
])
write('u10-corrupt', [
 {"open": "OccVortex"},
 tab,
 paste(ROLL_UNIQUE),
 {"affixOrder": "u10-corrupt-affixes"},
 tap("displayItemCorrupt") | {"shot": "u10-corrupt-unique"},
 ptap("rolls") | {"shot": "u10-corrupt-rolls"},
 {"cursorPopup": ["rollRangeSlider1", 30, 0], "events": click(), "dump": True},
 {"cursorPopup": ["rollRangeSlider2", -30, 0], "events": click(), "dump": True},
 {"cursorPopup": ["rollRangeSlider4", 10, 0], "events": click(), "dump": True, "shot": "u10-corrupt-rolls-moved"},
 {"cursorPopup": ["save"], "dump": True},
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 tap("displayItemCorrupt"),
 ptap("rolls"),
 ptap("implicits"),
 ptap("source"),
 ppick("source", 2),
 ptap("implicit1"),
 ppick("implicit1", 2),
 ptap("implicit3"),
 ppick("implicit3", 2),
 {"cursorPopup": ["save"], "dump": True},
 ptap("implicits"),
 ptap("rolls"),
 ptap("implicits"),
 ptap("sort"),
 ppick("sort", 3),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 paste(PLAIN_HELMET),
 tap("displayItemCorrupt") | {"shot": "u10-corrupt-helmet"},
 ptap("source"),
 ppick("source", 3),
 ptap("implicit1"),
 ppick("implicit1", 3),
 ptap("implicit2"),
 ppick("implicit2", 3),
 ptap("source"),
 ppick("source", 2),
 ptap("implicit2"),
 ppick("implicit2", 2),
 ptap("implicit3"),
 ppick("implicit3", 2),
 ptap("sort"),
 ppick("sort", 4),
 ptap("source"),
 ppick("source", 1),
 ptap("implicit1"),
 ppick("implicit1", 3),
 ptap("sort"),
 ppick("sort", 4),
 ptap("sort"),
 ppick("sort", 1),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 {"closeBuild": True},
])
write('u10-restrict', [
 {"open": "Dual Savior"},
 tab,
 paste(NO_CASTER),
 paste(NO_ATTACK),
 paste(ONLY_COLD),
 paste(TWO_TYPES),
 {"closeBuild": True},
])
write('u10-sets', [
 {"open": "OccVortex"},
 tab,
 {"cursorItem": ["setManage"], "events": click(), "dump": True, "shot": "u10-sets-manage"},
 {"cursorPopupRow": ["setList", 1]},
 {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON"]], "dump": True},
 {"cursorPopupRow": ["sharedList", 1], "dump": True, "shot": "u10-sets-drag"},
 {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]], "dump": True},
 {"cursorPopupRow": ["sharedList", 1], "events": click(), "dump": True},
 {"cursorPopupChild": ["sharedList", "rename"], "events": click(), "dump": True},
 ptyped("edit", "Y"),
 ptap("save"),
 {"sharedSlotOrder": "u10-sets-slots"},
 {"cursorPopupRow": ["sharedList", 1], "dump": True, "shot": "u10-sets-renamed"},
 {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON"]], "dump": True},
 {"cursorPopupRow": ["setList", 1], "dump": True},
 {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]], "dump": True},
 {"cursorPopupRow": ["sharedList", 1], "events": click(), "dump": True},
 {"cursorPopupChild": ["sharedList", "delete"], "events": click(), "dump": True, "shot": "u10-sets-delete"},
 {"cursor": [1200, 790], "events": [["KeyDown", "RETURN"], ["KeyUp", "RETURN"]], "dump": True},
 {"cursor": [1200, 790], "dump": True, "shot": "u10-sets-deleted"},
 ptap("close"),
 {"cursor": [1200, 790], "hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "release": ["CTRL"], "dump": True, "file": "OccVortex.xml", "fileOut": "u10-sets-save.xml"},
 {"closeBuild": True},
])
write('u10-minion', [
 {"open": "Guardian Test"},
 tab,
 {"cursor": [1200, 790], "dump": True, "shot": "u10-minion-tab"},
 {"cursorItemRow": ["itemList", 1]},
 {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON"]], "dump": True},
 {"cursorBuild": ["mainSkillMinion"], "dump": True, "shot": "u10-minion-drag"},
 {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursor": [1200, 790], "hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "release": ["CTRL"], "dump": True, "file": "Guardian Test.xml", "fileOut": "u10-minion-save.xml"},
 {"closeBuild": True},
])
# U10: `jewel_items` of the live spec: the jewel in a socket through undo, redo and an edit of the item under the same id
ABYSS_ITEM_ROW = 30
def ctrl(k): return [{"hold": ["CTRL"], "events": [["KeyDown", k], ["KeyUp", k]], "dump": True}, {"release": ["CTRL"]}]
TIPS = [{"cursorNode": n, "dump": True, "treeTip": True} for n in (33287, 3644, 26725)] + [{"cursor": [-100, -100], "dump": True}]
TO_TREE = [{"cursorBuild": ["modeTree"], "events": click()}, {"waitAssets": True}]
write('u10-abyss-jewel', [
 {"open": "Generals Perforate Zerker"},
 {"waitAssets": True},
 *press(971, 730),
 {"waitAssets": True},
 {"cursor": [-100, -100], "events": [["KeyDown", "RETURN"], ["KeyUp", "RETURN"]]},
 tab,
 paste(TECROD),
 {"cursor": [800, 600], "events": wheel("WHEELUP", 80), "dump": True},
 {"cursor": [800, 600]},
 tap("addDisplayItem"),
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": [ABYSS_SOCKET], "events": click(), "dump": True},
 {"cursorPick": [ABYSS_SOCKET, ABYSS_ROW_TECROD], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 *TO_TREE,
 {"treeOrder": "u10-abyss-order"},
 {"cursor": [796, 388], "events": wheel("WHEELDOWN", 5)},
 {"cursor": [800, 400]},
 {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON"]]},
 {"cursor": [602, 103]},
 {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]]},
 {"cursorNode": 33287, "events": wheel("WHEELUP", 4)},
 *TIPS,
 tab,
 {"cursor": [1200, 790]},
 *ctrl("z"),
 {"cursor": [1200, 790], "dump": True},
 *TO_TREE,
 *TIPS,
 tab,
 {"cursor": [1200, 790]},
 *ctrl("y"),
 {"cursor": [1200, 790], "dump": True},
 *TO_TREE,
 *TIPS,
 tab,
 # Zorath in another socket over a node of the jewel in this one: the last of `pairs(copyTable(spec.jewels))` wins it, an order of the process (R13): +12 and +21 Intelligence, 3 + 3 of 6 processes
 {"jewelOrder": "u10-abyss-jewels"},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": [ABYSS_SOCKET], "dump": True},
 {"cursor": [1200, 790], "clipboard": ZORATH},
 {"cursorItemRow": ["itemList", ABYSS_ITEM_ROW], "events": click()},
 {"events": [["KeyDown", "LEFTBUTTON", True], ["KeyUp", "LEFTBUTTON"]], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 tap("editDisplayItem"),
 {"cursorPopup": ["edit"], "events": click(), "dump": True},
 {"hold": ["CTRL"], "events": [["KeyDown", "a"], ["KeyUp", "a"], ["KeyDown", "v"], ["KeyUp", "v"]], "dump": True},
 {"release": ["CTRL"]},
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 tap("addDisplayItem"),
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": [ABYSS_SOCKET], "dump": True},
 *TO_TREE,
 *TIPS,
 tab,
 {"cursor": [1200, 790]},
 *ctrl("z"),
 {"cursor": [1200, 790], "dump": True},
 *TO_TREE,
 *TIPS,
 {"closeBuild": True},
])
# U10b: an equipped jewel edited under its id: `BuildAllDependsAndPaths` reads `itemsTab.items` (PassiveSpec.lua:1137, :1155), so the next rebuild sees the new jewel.
# Abyss: AddItem rebuilds nothing (ItemsTab.lua:1770), a node's click does. Timeless: AddItem rebuilds (:1770-1774).
LP_KAOM = "Rarity: UNIQUE\nLethal Pride\nTimeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nRadius: Large\nImplicits: 0\nCommanded leadership over 10000 warriors under Kaom\nPassives in radius are Conquered by the Karui\nHistoric\n"
LP_AKOYA = "Rarity: UNIQUE\nLethal Pride\nTimeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nRadius: Large\nImplicits: 0\nCommanded leadership over 9000 warriors under Akoya\nPassives in radius are Conquered by the Karui\nHistoric\n"
def jewel_in_socket(text, order):
    return [
     {"open": "Generals Perforate Zerker"},
     {"waitAssets": True},
     *press(971, 730),
     {"waitAssets": True},
     {"cursor": [-100, -100], "events": [["KeyDown", "RETURN"], ["KeyUp", "RETURN"]]},
     tab,
     paste(text),
     {"cursor": [800, 600], "events": wheel("WHEELUP", 80), "dump": True},
     {"cursor": [800, 600]},
     tap("addDisplayItem"),
     {"cursor": [1200, 790], "dump": True},
     {"cursorItem": [ABYSS_SOCKET], "events": click(), "dump": True},
     {"cursorPick": [ABYSS_SOCKET, ABYSS_ROW_TECROD], "events": click(), "dump": True},
     {"cursor": [1200, 790], "dump": True},
     *TO_TREE,
     {"treeOrder": order},
     {"cursor": [796, 388], "events": wheel("WHEELDOWN", 5)},
     {"cursor": [800, 400]},
     {"hold": ["LEFTBUTTON"], "events": [["KeyDown", "LEFTBUTTON"]]},
     {"cursor": [602, 103]},
     {"release": ["LEFTBUTTON"], "events": [["KeyUp", "LEFTBUTTON"]]},
     {"cursorNode": 33287, "events": wheel("WHEELUP", 4)},
     *TIPS,
    ]
def edit_item(text):
    return [
     tab,
     {"cursor": [1200, 790], "clipboard": text},
     # the list shows 19 rows: scrolled to its end, the last of them is the new item
     {"cursorItemRow": ["itemList", 19], "events": wheel("WHEELDOWN", 12), "dump": True},
     {"cursorItemRow": ["itemList", 19], "events": click()},
     {"events": [["KeyDown", "LEFTBUTTON", True], ["KeyUp", "LEFTBUTTON"]], "dump": True},
     {"cursor": [1200, 790], "dump": True},
     tap("editDisplayItem"),
     {"cursorPopup": ["edit"], "events": click(), "dump": True},
     {"hold": ["CTRL"], "events": [["KeyDown", "a"], ["KeyUp", "a"], ["KeyDown", "v"], ["KeyUp", "v"]], "dump": True},
     {"release": ["CTRL"]},
     ptap("save"),
     {"cursor": [1200, 790], "dump": True},
     tap("addDisplayItem"),
     {"cursor": [1200, 790], "dump": True},
     *TO_TREE,
    ]
# The calculation keeps the old conquest until the next rebuild as well (279,408.2, not 311,447.5).
write('u10b-jewel-edit', [
 *jewel_in_socket(TECROD, "u10b-edit-order"),
 *edit_item(TECROD.replace("4000", "3000")),
 *TIPS,
 {"cursorNode": 33287, "events": click(), "dump": True},
 *TIPS,
 tab,
 {"cursor": [1200, 790]},
 *ctrl("z"),
 {"cursor": [1200, 790], "dump": True},
 *TO_TREE,
 *TIPS,
 {"cursorNode": 33287, "events": click(), "dump": True},
 *TIPS,
 {"closeBuild": True},
])
write('u10b-timeless-edit', [
 *jewel_in_socket(LP_KAOM, "u10b-edit-order2"),
 *edit_item(LP_AKOYA),
 *TIPS,
 tab,
 {"cursor": [1200, 790]},
 *ctrl("z"),
 {"cursor": [1200, 790], "dump": True},
 *TO_TREE,
 *TIPS,
 {"closeBuild": True},
])
write('u10-version', [
 {"open": "OccVortex"},
 tab,
 {"open": "Old Version Test"},
 # the popup of a build that is not there: the tabs of the original still hold the last build, the port has shut it down
 {"cursor": [1200, 790], "dump": True, "popupOnly": True, "shot": "u10-version-popup"},
 ptap("cancel", popupOnly=True),
 {"cursor": [1200, 790]},
 {"closeBuild": True},
 {"open": "OccVortex"},
 tab,
 {"open": "Old Version Test"},
 {"cursor": [1200, 790], "dump": True, "popupOnly": True},
 ptap("convert"),
 {"cursor": [1200, 790], "dump": True},
 tab,
 {"cursor": [1200, 790], "dump": True, "shot": "u10-version-converted"},
 {"closeBuild": True},
])
write('u10-dev', [
 {"open": "OccVortex"},
 tab,
 {"devAlt": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": ["slot:Helmet"], "dump": True},
 paste("Rarity: RARE\nU10 Dev\nCoral Ring\n+20 to maximum Life\nThis line means nothing to the parser\n"),
 {"cursor": [1200, 790], "dump": True},
 {"cursorBuild": ["modeTree"], "events": click()},
 {"waitAssets": True},
 {"treeOrder": "u10-dev-order"},
 {"cursorNode": 61834, "dump": True, "treeTip": True},
 {"cursor": [-100, -100], "dump": True},
 {"devAlt": False},
 {"cursor": [-100, -100], "dump": True},
 {"closeBuild": True},
])
_rare_like = json.load(open('data/rare-like-uniques.json', encoding='utf-8'))
_uniques = json.load(open('data/uniques.json', encoding='utf-8'))['byType']
def unique_raw(title):
    for _l in _uniques.values():
        for _x in _l:
            if _x.split('\n')[0].lower() == title:
                return _x
    raise KeyError(title)
VALID_BASES = next(k for k, v in _rare_like.items() if isinstance(v, dict) and v.get('validBases'))
write('u10-crafting', [
 {"open": "OccVortex"},
 tab,
 tap("craftDisplayItem"),
 ptap("save"),
 {"cursor": [1200, 790], "dump": True},
 tap("craftingSorting"),
 {"cursorPick": ["craftingSorting", 3], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True, "shot": "u10-crafting-sorted"},
 tap("displayItemAffix1"),
 {"cursorPick": ["displayItemAffix1", 3], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 tap("displayItemCatalyst"),
 {"cursorPick": ["displayItemCatalyst", 2], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"closeBuild": True},
])
write('u10-rare-like', [
 {"open": "OccVortex"},
 tab,
 paste("Rarity: UNIQUE\n" + unique_raw(VALID_BASES)),
 {"cursor": [1200, 790], "dump": True},
 paste("Rarity: UNIQUE\n" + unique_raw("atziri's rule")),
 tap("displayItemAddCrucible"),
 ptap("close"),
 paste("Rarity: UNIQUE\n" + unique_raw("clayshaper")),
 tap("displayItemAddCrucible"),
 ptap("close"),
 {"closeBuild": True},
])
U10 = ['u10-crafting', 'u10-rare-like', 'u10-version', 'u10-dev', 'u10-abyss-jewel', 'u10b-jewel-edit', 'u10b-timeless-edit', 'u10-implicit', 'u10-crucible', 'u10-corrupt', 'u10-restrict', 'u10-sets', 'u10-minion']
write('perf', [
 {"open": "OccVortex"},
 {"cursorBuild": ["modeItems"], "events": click(), "perfClick": "openTab"},
 {"cursor": [1200, 790]},
 {"perf": 30},
 {"cursorItemRow": ["itemList", 2], "perfClick": "hoverRow"},
 {"cursorItemRow": ["uniqueDB", 5], "hold": ["CTRL"], "events": click(), "perfClick": "equipDB"},
 # once or every time: the frame after the edit (the recalculation), the hovers after it, a second edit, leaving and coming back, a second build object
 {"release": ["CTRL"], "perfClick": "recalcAfterEquip"},
 {"cursor": [1200, 790]},
 {"cursorItemRow": ["itemList", 2], "perfClick": "hoverAfterRecalc"},
 {"cursor": [1200, 790]},
 {"cursorItemRow": ["itemList", 2], "perfClick": "hoverAgainSame"},
 {"cursor": [1200, 790]},
 {"cursorItemRow": ["itemList", 3], "perfClick": "hoverOtherRow"},
 {"cursorItemRow": ["uniqueDB", 6], "hold": ["CTRL"], "events": click(), "perfClick": "equipDB2"},
 {"release": ["CTRL"], "perfClick": "recalcAfterEquip2"},
 {"cursor": [1200, 790]},
 {"cursorBuild": ["modeSkills"], "events": click(), "perfClick": "leaveTab"},
 {"cursorBuild": ["modeItems"], "events": click(), "perfClick": "openTab2"},
 {"closeBuild": True},
 {"open": "OccVortex"},
 {"cursorBuild": ["modeItems"], "events": click(), "perfClick": "openTabNewBuild"},
 {"cursor": [1200, 790]},
 {"cursorItemRow": ["itemList", 2], "perfClick": "hoverNewBuild"},
 {"closeBuild": True},
])
# personal builds (builds/local-builds.json, local only): the tab of a real character; "items_main_group" has "<build>:<row>" for the socket group a step selects in the sidebar
if os.path.exists('builds/local-builds.json'):
    _local = json.load(open('builds/local-builds.json', encoding='utf-8'))
    _group = dict(x.rsplit(':', 1) for x in _local.get('items_main_group', []))
    for _i, _b in enumerate(_local.get('items', []), 1):
        _p = f'local-{_i}'
        _main = []
        if _b in _group:
            _main = [
             {"cursorBuild": ["mainSocketGroup"], "events": click(), "dump": True},
             {"cursorBuildPick": ["mainSocketGroup", int(_group[_b])], "events": click(), "dump": True},
             {"cursor": [1200, 790], "dump": True},
            ]
        write(_p, [{"open": _b}, {"waitAssets": True}] + _main + [
         tab | {"shot": f"{_p}-tab"},
         {"cursor": [1200, 790], "dump": True},
         {"cursorItem": ["slot:Helmet"], "dump": True, "shot": f"{_p}-slot-tip"},
         {"cursorItem": ["slot:Flask 1"], "dump": True},
         {"cursorItemRow": ["itemList", 2], "dump": True, "shot": f"{_p}-row-tip"},
         {"cursorItemRow": ["uniqueDB", 3], "dump": True, "shot": f"{_p}-db-tip"},
         {"cursorItemRow": ["itemList", 1], "events": click()},
         {"events": [["KeyDown", "LEFTBUTTON", True], ["KeyUp", "LEFTBUTTON"]], "dump": True, "shot": f"{_p}-display"},
         {"cursor": [1200, 790], "dump": True},
         {"cursorItem": ["removeDisplayItem"], "events": click()},
         {"cursor": [1200, 790], "dump": True},
         {"cursorItem": ["slot:Ring 1"], "events": click(), "dump": True},
         {"cursorPick": ["slot:Ring 1", 1], "events": click(), "dump": True},
         {"cursor": [1200, 790], "dump": True, "shot": f"{_p}-ring-off"},
         {"hold": ["CTRL"], "events": [["KeyDown", "z"], ["KeyUp", "z"]], "dump": True},
         {"release": ["CTRL"]},
         {"cursor": [1200, 790], "dump": True, "shot": f"{_p}-undo"},
         {"closeBuild": True},
        ])
# a unique of the list with a Foulborn mod: its range line shows the mutated check box and the icon of ItemsTab.lua:1054
write('foulborn', [
 {"open": "Dual Wield Cospris CoC"},
 tab,
 {"cursor": [1200, 790]},
 {"cursorItemRow": ["uniqueDB", 10], "events": click()},
 {"events": [["KeyDown", "LEFTBUTTON", True], ["KeyUp", "LEFTBUTTON"]], "dump": True},
 {"cursor": [1200, 790], "dump": True},
 {"cursorItem": ["displayItemRangeLine"], "events": click(), "dump": True},
 {"cursorPick": ["displayItemRangeLine", int(os.environ.get('POB_FOULBORN_ROW', '1'))], "events": click(), "dump": True},
 {"cursor": [1200, 790], "dump": True, "shot": "foulborn-check"},
 {"closeBuild": True},
])
# TradeSearchWeights through Load and Save (the build is written by the runner)
write('trade-weights', [
 {"open": "Trade Weights Test"},
 {"cursor": [1200, 790], "hold": ["CTRL"], "events": [["KeyDown", "s"], ["KeyUp", "s"]], "release": ["CTRL"], "dump": True, "file": "Trade Weights Test.xml", "fileOut": "trade-weights-save.xml"},
 {"closeBuild": True},
])
write('real-input', [{"open": "OccVortex"}, {"waitAssets": True}, {"realInput": True}])
json.dump(['base', 'display', 'equip', 'granted', 'editor', 'loadout', 'granted-group', 'coverage', 'timeless', 'abyss', 'foulborn', 'trade-weights'] + U10, open('tools/sg-oracle/scenes/items/index.json', 'w'))
