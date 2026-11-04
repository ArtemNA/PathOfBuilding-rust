# The scenes of the Compare gate. From the repository root: python tools/sg-oracle/scenes/compare/gen.py
import json, os
D = 'tools/sg-oracle/scenes/compare'
os.makedirs(D, exist_ok=True)
names = []
def click(k='LEFTBUTTON'): return [["KeyDown", k], ["KeyUp", k]]
def wheel(k): return [["KeyDown", k], ["KeyUp", k]]
def write(name, steps):
    names.append(name)
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
    json.dump({"steps": out}, open(f'{D}/{name}.json', 'w'), indent=0)
tab = {"cursorBuild": ["modeCompare"], "events": click(), "dump": True}
AWAY = [1200, 790]
def sub(n, **kw): return {"cursorCompare": [f"subTab{n}"], "events": click(), "dump": True, **kw}
def pick(name, row):
    return [{"cursorCompare": [name], "events": click()}, {"cursorCompareRow": [name, row], "events": click()}, {"cursor": AWAY, "dump": True}]

# selecting a build, the top bar, the summary with the numbers of both builds
write('summary', [
 {"open": "Dual Savior"},
 tab | {"shot": "summary-empty"},
 {"compareImport": ["OccVortex", "OccVortex"]},
 {"cursor": AWAY, "dump": True, "shot": "summary-one"},
 {"cursor": [700, 500], "events": wheel("WHEELDOWN"), "dump": True, "shot": "summary-scrolled"},
 {"events": wheel("WHEELUP"), "dump": True},
 {"compareImport": ["Generals Perforate Zerker", "Other - Zerker"]},
 {"cursor": AWAY, "dump": True, "shot": "summary-two"},
 *pick("compareBuildSelect", 1),
 {"cursorCompare": ["removeBtn"], "events": click(), "dump": True},
 {"cursorCompare": ["removeBtn"], "events": click(), "dump": True, "shot": "summary-removed"},
 {"closeBuild": True},
])
# the socket group of the entry, its rebuild; the current build under the comparison; a build with minions
write('select', [
 {"open": "Mirage Archer Toxic Rain"},
 tab,
 {"compareImport": ["Dual Wield Cospris CoC", "Cospris"]},
 {"cursor": AWAY, "dump": True, "shot": "select-one"},
 *pick("cmpSocketGroup", 2),
 *pick("cmpSocketGroup", 3),
 {"compareImport": ["Spectre Test", "Spectres"]},
 {"cursor": AWAY, "dump": True},
 *pick("cmpSocketGroup", 1),
 {"cursor": AWAY, "dump": True, "shot": "select-minion"},
 *pick("cmpSocketGroup", 2),
 {"cursor": AWAY, "dump": True, "shot": "select-mine"},
 *pick("cmpSocketGroup", 3),
 {"cursorCompare": ["cmpMainSkill"], "dump": True},
 {"closeBuild": True},
])
# the sets of the entry; the primary changes under the comparison; nothing of the comparison is saved
write('sets', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["Mirage Archer Toxic Rain", "Mirage"]},
 {"cursor": AWAY, "dump": True},
 *pick("compareSpecSelect", 1),
 *pick("compareSpecSelect", 2),
 {"compareImport": ["Sets Test", "Sets"]},
 {"cursor": AWAY, "dump": True},
 *pick("compareItemSetSelect", 2),
 {"cursor": AWAY, "dump": True, "shot": "sets-empty-items"},
 *pick("compareItemSetSelect", 1),
 {"cursorBuild": ["mainSocketGroup"], "events": click()},
 {"cursorBuild": ["mainSocketGroup", 0, 36], "events": click()},
 {"cursor": AWAY, "dump": True},
 {"save": True},
 {"cursor": AWAY, "dump": True, "saved": "Dual Savior"},
 {"closeBuild": True},
])
# the import popup: a bad code, an empty field, cancel, the keys of the popup
write('popup', [
 {"open": "OccVortex"},
 tab,
 {"cursorCompare": ["importBtn"], "dump": True},
 {"events": click(), "dump": True, "shot": "popup-open"},
 {"cursorPopup": ["go"], "events": click(), "dump": True},
 {"cursorPopup": ["input"], "events": click()},
 {"events": [["Char", "x"], ["Char", "y"]], "dump": True},
 {"cursorPopup": ["go"], "events": click(), "dump": True, "shot": "popup-bad-code"},
 {"cursorPopup": ["cancel"], "events": click(), "dump": True},
 {"closeBuild": True},
])
# the power report: the metric, the categories, the sort by a column, a row under the cursor
def row(n, dx=0): return {"cursorCompare": ["comparePowerReportList", dx - 300, -125 + 18 + 2 + 18 * n + 9]}
def scroll_to_report(): return {"cursor": [700, 500], "events": wheel("WHEELDOWN") * 40}
write('power', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["OccVortex", "OccVortex"]},
 scroll_to_report() | {"dump": True},
 {"compareSel": ["comparePowerStatSelect", 2]},
 {"cursor": AWAY, "dump": True, "shot": "power-full-dps"},
 {"cursorCompare": ["comparePowerReportList", -200, -116], "events": click(), "dump": True},
 {"cursorCompare": ["comparePowerReportList", 300, -116], "events": click(), "dump": True},
 {"cursorCompare": ["comparePowerReportList", 80, -116], "events": click(), "dump": True},
 {"cursorCompare": ["comparePowerTreeCheck"], "events": click()},
 {"cursor": AWAY, "dump": True, "shot": "power-no-tree"},
 row(0) | {"dump": True, "shot": "power-item-tip"},
 row(1) | {"dump": True},
 {"compareSel": ["comparePowerStatSelect", 5]},
 {"cursor": AWAY, "dump": True},
 {"compareSel": ["comparePowerStatSelect", 1]},
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
write('power2', [
 {"open": "OccVortex"},
 tab,
 # the labels of the groups read nameSpec, which differs between the processes on Cospris (MINEFIELD R48)
 {"compareImport": ["Generals Perforate Zerker", "Zerker"]},
 scroll_to_report() | {"dump": True},
 {"compareSel": ["comparePowerStatSelect", 4]},
 {"cursor": AWAY, "dump": True, "shot": "power2"},
 {"cursorCompare": ["comparePowerTreeCheck"], "events": click()},
 {"cursor": AWAY, "dump": True},
 {"cursorCompare": ["comparePowerItemsCheck"], "events": click()},
 {"cursor": AWAY, "dump": True},
 {"cursorCompare": ["comparePowerGemsCheck"], "events": click()},
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
# the groups of both builds, the aligned gems, the highlight of a hovered gem, the wheel
write('skills', [
 {"open": "Generals Perforate Zerker"},
 tab,
 {"compareImport": ["Mirage Archer Toxic Rain", "Mirage"]},
 sub('Skills'),
 {"cursor": AWAY, "dump": True, "shot": "skills-tab"},
 {"cursor": [340, 240], "dump": True, "shot": "skills-hover"},
 {"cursor": [340, 276], "dump": True},
 {"cursor": [340, 312], "dump": True},
 {"cursor": [700, 500], "events": wheel("WHEELDOWN"), "dump": True, "shot": "skills-scrolled"},
 {"compareImport": ["Dual Savior", "Other - Savior"]},
 {"cursor": AWAY, "dump": True},
 *pick("compareBuildSelect", 1),
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
# the slots of both builds, hover on an item and the Equip button, copy, equip, expanded mode, the sets, a socket
write('items', [
 {"open": "Dual Wield Cospris CoC"},
 tab,
 {"compareImport": ["OccVortex", "Occ"]},
 sub('Items'),
 {"cursor": AWAY, "dump": True, "shot": "items-compact"},
 {"cursor": [500, 300], "dump": True, "shot": "items-hover-primary"},
 {"cursor": [1000, 300], "dump": True, "shot": "items-hover-compare"},
 {"cursor": [340, 270], "dump": True},
 {"cursor": AWAY, "dump": True},
 {"cursorCompare": ["itemsExpandedCheck"], "events": click(), "dump": True, "shot": "items-expanded"},
 {"cursor": [700, 500], "events": wheel("WHEELDOWN"), "dump": True},
 {"cursor": [700, 500], "events": wheel("WHEELDOWN"), "dump": True},
 {"events": wheel("WHEELUP")},
 {"events": wheel("WHEELUP")},
 {"cursorCompare": ["itemsExpandedCheck"], "events": click(), "dump": True},
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
write('items-copy', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["Mirage Archer Toxic Rain", "Mirage"]},
 sub('Items'),
 {"compareScrollX": 300},
 {"compareItemButton": [4, "equip"], "dump": True, "shot": "items-equip-tip"},
 {"compareItemButton": [4, "equip"], "events": click(), "dump": True},
 {"cursor": AWAY, "dump": True},
 {"compareItemButton": [6, "copy"], "events": click(), "dump": True},
 {"compareItemButton": [6, "buy"], "dump": True},
 {"cursor": AWAY, "dump": True, "shot": "items-after-copy"},
 {"closeBuild": True},
])
# the Calcs view: only differences, a row and an info line under the cursor, both skill selections, the mode
write('calcs', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["Spectre Test", "Spectres"]},
 sub('Calcs'),
 {"cursor": AWAY, "dump": True, "shot": "calcs-tab"},
 {"cursor": [500, 480], "dump": True, "shot": "calcs-row-tip"},
 {"cursor": [500, 560], "dump": True},
 {"cursor": [900, 620], "dump": True},
 {"cursor": [400, 330], "dump": True},
 {"cursor": [900, 330], "dump": True},
 {"cursor": [700, 500], "events": wheel("WHEELDOWN"), "dump": True},
 {"cursorCompare": ["calcsShowOnlyDifferencesCheck"], "events": click()},
 {"cursor": AWAY, "dump": True, "shot": "calcs-all-rows"},
 *pick("cmpCalcsSocketGroup", 1),
 {"cursor": AWAY, "dump": True, "shot": "calcs-minion"},
 {"cursorCompare": ["cmpCalcsShowMinion"], "events": click()},
 {"cursor": AWAY, "dump": True},
 *pick("cmpCalcsSocketGroup", 2),
 *pick("cmpCalcsMode", 1),
 *pick("primCalcsMode", 2),
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
# the Config view: the differences, the toggle, the search, the sets, writes on both sides, the copy
write('config', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["OccVortex", "Occ"]},
 sub('Config'),
 {"cursor": AWAY, "dump": True, "shot": "config-tab"},
 {"cursorCompare": ["configToggleBtn"], "events": click()},
 {"cursor": AWAY, "dump": True, "shot": "config-all"},
 {"cursor": [700, 500], "events": wheel("WHEELDOWN"), "dump": True},
 {"events": wheel("WHEELUP")},
 {"cursorCompare": ["configSearchEdit"], "events": click()},
 {"events": [["Char", "c"], ["Char", "h"], ["Char", "a"], ["Char", "r"]], "dump": True, "shot": "config-search"},
 {"cursorCompare": ["cfg_c_useFrenzyCharges"], "events": click()},
 {"cursor": AWAY, "dump": True},
 {"cursorCompare": ["cfg_p_useFrenzyCharges"], "events": click()},
 {"cursor": AWAY, "dump": True, "shot": "config-written"},
 {"cursorCompare": ["copyConfigBtn"], "events": click(), "dump": True},
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
# U8b2: Copy and use of a tree; the set selectors of the Items view; the socket under a label; the tooltips of the tree; the version of an entry
write('tree-copy', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["Generals Perforate Zerker", "Zerker"]},
 sub('Tree'),
 {"waitAssets": True},
 {"cursorCompare": ["copySpecUseBtn"], "events": click(), "dump": True, "setsState": True},
 {"waitAssets": True},
 {"treeOrder": "tree-copy-order"},
 {"cursor": AWAY, "dump": True, "setsState": True, "shot": "tree-copy-use"},
 {"cursorCompare": ["copySpecBtn"], "events": click(), "dump": True, "setsState": True},
 {"cursor": AWAY, "dump": True, "setsState": True},
 {"closeBuild": True},
])
def pick_sets(name, row):
    return [{"cursorCompare": [name], "events": click()}, {"cursorCompareRow": [name, row], "events": click()}, {"cursor": AWAY, "dump": True, "setsState": True}]
write('items-sets', [
 {"open": "Sets Test"},
 tab,
 {"compareImport": ["Mirage Archer Toxic Rain", "Mirage"]},
 sub('Items'),
 {"cursor": AWAY, "dump": True, "setsState": True, "shot": "items-sets-start"},
 *pick_sets("primaryItemSetSelect", 2),
 {"cursor": AWAY, "dump": True, "setsState": True, "shot": "items-sets-primary"},
 *pick_sets("primaryItemSetSelect", 1),
 *pick_sets("compareTreeSetSelect", 1),
 {"cursor": AWAY, "dump": True, "setsState": True, "shot": "items-sets-tree"},
 *pick_sets("compareTreeSetSelect", 3),
 {"closeBuild": True},
])
write('items-sets2', [
 {"open": "Mirage Archer Toxic Rain"},
 tab,
 {"compareImport": ["Sets Test", "Sets"]},
 sub('Items'),
 {"cursor": AWAY, "dump": True, "setsState": True},
 *pick_sets("compareItemSetSelect2", 2),
 {"cursor": AWAY, "dump": True, "setsState": True, "shot": "items-sets-compare"},
 *pick_sets("primaryTreeSetSelect", 1),
 {"cursor": AWAY, "dump": True, "setsState": True, "shot": "items-sets-primary-tree"},
 {"closeBuild": True},
])
write('items-viewer', [
 {"open": "Mirage Archer Toxic Rain"},
 {"waitAssets": True},
 tab,
 {"compareImport": ["Dual Wield Cospris CoC", "Cospris"]},
 sub('Items'),
 {"waitAssets": True},
 {"cursor": AWAY, "dump": True},
 {"cursor": [335, 676], "dump": True, "shot": "items-viewer-1"},
 {"cursor": [335, 696], "dump": True, "shot": "items-viewer-2"},
 {"cursor": [330, 240 + 20 * 2 + 10], "dump": True},
 {"closeBuild": True},
])
# the node tooltip of the primary build: on its own, and after an import (the cache the entries leave, MINEFIELD R59)
write('node-tips', [
 {"open": "Dual Savior"},
 {"waitAssets": True},
 {"treeOrder": "node-tips-order"},
 {"cursorNodeAuto": True, "dump": True},
 {"cursorNodeAuto": True, "dump": True, "nodeTip": True, "shot": "node-tip-alone"},
 tab,
 {"compareImport": ["OccVortex", "Occ"]},
 {"cursorBuild": ["modeTree"], "events": click()},
 {"waitAssets": True},
 {"cursorNodeAuto": True, "dump": True},
 {"cursorNodeAuto": True, "dump": True, "nodeTip": True, "shot": "node-tip-imported"},
 {"closeBuild": True},
])
# addCompareJewelSection (PassiveTreeView.lua:1482): a socket with another jewel, a socket of the compared build only, a socket empty there
write('jewel-tips', [
 {"open": "Dual Wield Cospris CoC"},
 tab,
 {"compareImport": ["OccVortex", "Occ"]},
 sub('Tree'),
 {"waitAssets": True},
 {"treeOrder": "jewel-tips-order"},
 {"treeCenter": True},
 {"cursor": [800, 500], "events": wheel("WHEELDOWN") * 3},
 {"cursor": AWAY},
 {"cursorCompareNode": 61419, "dump": True},
 {"cursorCompareNode": 61419, "dump": True, "nodeTip": True, "shot": "jewel-tip-both"},
 {"cursor": AWAY},
 {"cursorCompareNode": 36634, "dump": True},
 {"cursorCompareNode": 36634, "dump": True, "nodeTip": True, "shot": "jewel-tip-compare-only"},
 {"cursor": AWAY},
 {"cursorCompareNode": 64583, "dump": True},
 {"cursorCompareNode": 64583, "dump": True, "nodeTip": True, "shot": "jewel-tip-primary-only"},
 {"closeBuild": True},
])
# :889 the version of an entry; data.jewelRadius is one for all the builds (MINEFIELD R42)
write('tree-version', [
 {"open": "Generals Perforate Zerker"},
 tab,
 {"compareImport": ["Dual Savior", "Savior"]},
 sub('Tree'),
 {"waitAssets": True},
 {"treeOrder": "tree-version-order"},
 {"cursor": AWAY, "dump": True, "radius": True, "shot": "tree-version-start"},
 {"cursorCompare": ["rightVersionSelect"], "events": click(), "dump": True},
 {"cursorCompareRow": ["rightVersionSelect", 3], "events": click(), "dump": True, "shot": "tree-version-popup"},
 {"cursorPopup": ["cancel"], "events": click(), "dump": True},
 {"compareSel": ["rightVersionSelect", 25]},
 {"cursor": AWAY, "dump": True},
 {"cursorPopup": ["convert"], "events": click(), "dump": True, "radius": True},
 {"waitAssets": True},
 {"treeOrder": "tree-version-order2"},
 {"cursor": AWAY, "dump": True, "radius": True, "shot": "tree-version-overlay"},
 # the rings and the numbers of the primary build under the table the entry left
 # no hover over a jewel of the primary here: PassiveTreeView.lua:1005 of the original fails on the table of another tree version (a Lua error; the port draws)
 {"cursorBuild": ["mainSocketGroup"], "events": click()},
 {"cursorBuild": ["mainSocketGroup", 0, 36], "events": click()},
 {"cursor": AWAY, "dump": True, "radius": True},
 {"closeBuild": True},
])
# Import from Folder: the list, the search, the sort, folders and the path, the errors, a build by its row, the wheel, the keys
SELECT_ALL_DELETE = {"hold": ["CTRL"], "events": [["KeyDown", "a"], ["KeyUp", "a"], ["KeyDown", "BACK"], ["KeyUp", "BACK"]], "release": ["CTRL"], "dump": True}
FOLDER = [{"cursorCompare": ["importBtn"], "events": click()}, {"cursorPopup": ["importFolder"], "events": click(), "dump": True}]
write('folder', [
 {"open": "OccVortex"},
 tab,
 {"cursorCompare": ["importBtn"], "events": click()},
 {"cursorPopup": ["importFolder"], "events": click(), "dump": True, "shot": "folder-open"},
 {"events": [["Char", "D"], ["Char", "u"], ["Char", "a"], ["Char", "l"]], "dump": True, "shot": "folder-search"},
 SELECT_ALL_DELETE,
 {"cursorPopup": ["sort"], "events": click(), "dump": True},
 {"cursorPopupDrop": ["sort", 2], "events": click(), "dump": True},
 {"cursorPopup": ["sort"], "events": click()},
 {"cursorPopupDrop": ["sort", 1], "events": click(), "dump": True},
 # into the folder by the Open button, back by the path
 {"cursorFolderRow": 1, "events": click(), "dump": True},
 {"cursorPopup": ["open"], "dump": True},
 {"cursorPopup": ["open"], "events": click(), "dump": True, "shot": "folder-inside"},
 {"cursorFolderPath": 1, "dump": True},
 {"cursorFolderPath": 1, "events": click(), "dump": True},
 # a broken file: the message popup over the list
 {"cursorFolderRow": 1, "events": click()},
 {"cursorPopup": ["open"], "events": click()},
 {"cursorFolderRow": 3, "events": click(), "dump": True},
 {"cursorPopup": ["open"], "events": click(), "dump": True, "shot": "folder-broken"},
 {"cursorPopup": ["close"], "events": click(), "dump": True},
 # the folder of many: the wheel over a list without the focus, the list with it
 {"cursorFolderRow": 1, "events": click()},
 {"cursorPopup": ["open"], "events": click(), "dump": True},
 {"cursorFolderRow": 3, "events": wheel("WHEELDOWN"), "dump": True, "shot": "folder-scrolled"},
 {"events": wheel("WHEELDOWN"), "dump": True},
 {"cursorFolderRow": 3, "events": click(), "dump": True},
 {"events": wheel("WHEELUP"), "dump": True},
 # back, a build by Return on its row: the entry, the popup closed
 {"cursorFolderPath": 2, "events": click(), "dump": True},
 {"cursorFolderRow": 2, "events": click(), "dump": True},
 {"events": [["KeyDown", "RETURN"], ["KeyUp", "RETURN"]], "dump": True, "shot": "folder-imported"},
 # again: the sort stays, the Close button
 {"cursorCompare": ["importBtn"], "events": click()},
 {"cursorPopup": ["importFolder"], "events": click(), "dump": True},
 {"cursorPopup": ["close"], "events": click(), "dump": True},
 *FOLDER,
 {"events": [["KeyDown", "ESCAPE"], ["KeyUp", "ESCAPE"]], "dump": True},
 {"events": [["KeyDown", "ESCAPE"], ["KeyUp", "ESCAPE"]], "dump": True},
 # a build at the root by the Open button
 *FOLDER,
 {"cursorFolderRow": 2, "events": click(), "dump": True},
 {"cursorPopup": ["open"], "events": click(), "dump": True},
 {"cursor": AWAY, "dump": True, "shot": "folder-root-build"},
 # an empty file met by the scan: the cloud error under the list
 {"writeFile": ["Folder Test/Empty File.xml", ""]},
 *FOLDER,
 {"cursor": AWAY, "dump": True, "shot": "folder-empty"},
 {"cursorPopup": ["close"], "events": click(), "dump": True},
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
# the cache shared with the entries (oracle only: numbers for the report; the Rust test skips cacheProbe)
# the Tree view: overlay and side by side, a spec of the entry, the search, the copy of a tree, the row of the report on a tree node
write('tree', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["Generals Perforate Zerker", "Zerker"]},
 sub('Tree'),
 {"waitAssets": True},
 {"treeOrder": "tree-order"},
 {"cursor": AWAY, "dump": True, "shot": "tree-overlay"},
 {"cursorCompare": ["treeOverlayCheck"], "events": click(), "dump": True},
 {"waitAssets": True},
 {"cursor": AWAY, "dump": True, "shot": "tree-side"},
 {"cursorCompare": ["leftTreeSearch"], "events": click()},
 {"events": [["Char", "l"], ["Char", "i"], ["Char", "f"], ["Char", "e"]], "dump": True, "shot": "tree-search"},
 {"cursorCompare": ["treeOverlayCheck"], "events": click(), "dump": True},
 {"cursorCompare": ["copySpecBtn"], "events": click(), "dump": True},
 {"cursor": AWAY, "dump": True},
 {"closeBuild": True},
])
write('power-tree', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["OccVortex", "OccVortex"]},
 {"cursor": [700, 500], "events": wheel("WHEELDOWN") * 40},
 {"cursorCompare": ["comparePowerItemsCheck"], "events": click()},
 {"cursorCompare": ["comparePowerGemsCheck"], "events": click()},
 {"cursorCompare": ["comparePowerConfigCheck"], "events": click()},
 {"compareSel": ["comparePowerStatSelect", 2]},
 {"cursor": AWAY, "dump": True},
 row(0) | {"dump": True, "shot": "power-tree-tip"},
 {"closeBuild": True},
])
# Red on purpose until a decision: the comparison lines of the config tooltip run over the `GlobalCache` the builds share (the report of U8b).
write('config-tips', [
 {"open": "Dual Savior"},
 tab,
 {"compareImport": ["OccVortex", "Occ"]},
 sub('Config'),
 {"cursorCompare": ["configToggleBtn"], "events": click()},
 {"cursorCompare": ["configSearchEdit"], "events": click()},
 {"events": [["Char", "c"], ["Char", "h"], ["Char", "a"], ["Char", "r"]]},
 {"cursorCompare": ["cfg_c_useFrenzyCharges"], "events": click(), "dump": True},
 {"cursor": AWAY},
 {"cursorCompare": ["cfg_p_useFrenzyCharges"], "dump": True},
 {"cursorCompare": ["cfg_p_useFrenzyCharges"], "events": click(), "dump": True},
 {"closeBuild": True},
])
# the node tooltip of the primary build before and after an import (oracle only: numbers for the report)
write('probe-tree', [
 {"open": "Dual Savior"},
 {"waitAssets": True},
 {"cursorNodeAuto": True, "dump": True},
 {"cursorNodeAuto": True, "dump": True, "nodeTip": True},
 tab,
 {"compareImport": ["OccVortex", "Occ"]},
 {"cursorBuild": ["modeTree"], "events": click()},
 {"cursorNodeAuto": True, "dump": True},
 {"cursorNodeAuto": True, "dump": True, "nodeTip": True},
 {"closeBuild": True},
])
write('probe-cache', [
 {"open": "Dual Savior"},
 {"cacheProbe": ["conditionEnemyBleeding", True, "TotalDPS"]},
 tab,
 {"compareImport": ["OccVortex", "OccVortex"]},
 {"cacheProbe": ["conditionEnemyBleeding", True, "TotalDPS"]},
 {"cacheProbe": ["conditionEnemyBleeding", True, "FullDPS"]},
 {"closeBuild": True},
])
json.dump(names, open(f'{D}/index.json', 'w'))
# numbers for the report: the same steps on both sides
perf = [{"open": "Generals Perforate Zerker"}, tab | {"perfClick": "open tab"}, {"compareImport": ["Dual Wield Cospris CoC", "Cospris"]}, {"cursor": AWAY}, {"perf": 30}]
for n in ['Tree', 'Skills', 'Items', 'Calcs', 'Config', 'Summary']:
    perf += [{"cursorCompare": [f"subTab{n}"], "events": click(), "perfClick": f"switch {n}"}, {"cursor": AWAY}, {"perf": 30}]
perf += [{"cursorCompare": ["cmpSocketGroup"], "events": click()}, {"cursorCompareRow": ["cmpSocketGroup", 2], "events": click()}, {"cursor": AWAY, "perfClick": "recalc entry"}, {"closeBuild": True}]
write('probe-perf', perf)
json.dump(names, open(f'{D}/index.json', 'w'))
# Personal builds: the local list (not in git), the scenes stay ignored; a minion group is chosen through the UI when the list names it.
local = 'builds/local-builds.json'
if os.path.exists(local):
    lb = json.load(open(local))
    pair = lb.get('compare', [])
    main_group = {k.split(':')[0]: int(k.split(':')[1]) for k in lb.get('items_main_group', [])}
    for i, (a, b) in enumerate([(pair[0], pair[1]), (pair[1], pair[0])] if len(pair) > 1 else []):
        steps = [{"open": a}, tab, {"compareImport": [b, "Other"]}, {"cursor": AWAY, "dump": True, "shot": f"local-{i+1}-summary"}]
        if b in main_group:
            steps += [{"compareSel": ["cmpSocketGroup", main_group[b]]}, {"cursor": AWAY, "dump": True, "shot": f"local-{i+1}-minion"}]
        for n in ['Skills', 'Items', 'Calcs', 'Config']:
            steps += [sub(n), {"cursor": AWAY, "dump": True, "shot": f"local-{i+1}-{n.lower()}"}]
        steps.append({"closeBuild": True})
        json.dump({"steps": steps}, open(f'{D}/local-{i+1}.json', 'w'), indent=0)
