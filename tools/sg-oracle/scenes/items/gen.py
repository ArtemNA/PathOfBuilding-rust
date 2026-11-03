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
json.dump(['base', 'display', 'equip', 'granted', 'editor', 'loadout', 'granted-group', 'coverage', 'timeless', 'foulborn', 'trade-weights'], open('tools/sg-oracle/scenes/items/index.json', 'w'))
