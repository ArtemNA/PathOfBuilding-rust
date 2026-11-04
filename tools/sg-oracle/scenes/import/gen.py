# U8a: the scenes of the Import and Party tabs on the public goldens. Run from the repo root: python tools/sg-oracle/scenes/import/gen.py
import base64, json, os, re, zlib

ROOT = json.load(open('tools/upstream.json'))['pobRoot']
OUT = 'tools/sg-oracle/scenes/import'
os.makedirs(OUT, exist_ok=True)


def code_of(text):
    data = text if isinstance(text, bytes) else text.encode('utf-8')
    return base64.b64encode(zlib.compress(data)).decode().replace('+', '-').replace('/', '_')


def click(k='LEFTBUTTON'):
    return [["KeyDown", k], ["KeyUp", k]]


def write(name, steps):
    out = []
    for s in steps:
        # the runner lets a step's `release` go before its events (R53)
        if 'hold' in s and 'release' in s and 'events' in s:
            s = dict(s)
            rel = s.pop('release')
            out.append(s)
            out.append({'release': rel})
        else:
            out.append(s)
    json.dump({"steps": out}, open(f'{OUT}/{name}.json', 'w', newline='\n'), indent=0)


def imp(c, **k):
    return {"cursorImport": c, **k}


def par(c, **k):
    return {"cursorParty": c, **k}


AWAY = {"cursor": [1200, 790]}
TAB = {"cursorBuild": "modeImport", "events": click(), "dump": True}
PARTY = {"cursorBuild": "modeParty", "events": click(), "dump": True}


def typed(text):
    return [["Char", c] for c in text]


def paste(text):
    return {"clipboard": text, "hold": ["CTRL"], "events": [["KeyDown", "v"], ["KeyUp", "v"]], "release": ["CTRL"], "dump": True}


def ctrl(key, **k):
    return {"hold": ["CTRL"], "events": [["KeyDown", key], ["KeyUp", key]], "release": ["CTRL"], "dump": True, **k}


def tapi(c, **k):
    return imp(c, events=click(), dump=True, **k)


def tapp(c, **k):
    return par(c, events=click(), dump=True, **k)


def pick(tab, c, row, **k):
    return {"cursorDrop": [tab, c, row], "events": click(), "dump": True, **k}


# --- the sections, the states of their controls, hover
write('tab', [
    {"open": "OccVortex"},
    TAB | {"shot": "tab"},
    imp("enablePartyExportBuffs", dump=True, shot="tab-hover-export"),
    imp("generateCode", dump=True, shot="tab-hover-generate"),
    imp("siteRemoveAccount", dump=True, shot="tab-hover-remove"),
    imp("authenticateButton", dump=True, shot="tab-hover-auth"),
    imp("exportFrom", dump=True),
    imp("importCodeMode", dump=True),
    imp("importCodeGo", dump=True),
    tapi("siteAccountName"),
    {"events": typed("Some"), "dump": True, "shot": "tab-account-typed"},
    imp("siteAccountNameGo", dump=True, shot="tab-account-go-tip"),
    tapi("siteAccountName"),
    {"events": typed("#1234"), "dump": True},
    imp("siteAccountNameGo", dump=True, shot="tab-account-go"),
    tapi("siteAccountHistory", shot="tab-history-open"),
    AWAY | {"events": click(), "dump": True},
    AWAY | {"importMode": "SELECTCHAR", "dump": True, "shot": "tab-selectchar"},
    imp("siteCharImportTree", dump=True),
    tapi("siteCharImportTreeClearJewels"),
    tapi("siteCharImportItemsIgnoreWeaponSwap", shot="tab-selectchar-checks"),
    tapi("siteCharClose"),
    AWAY | {"token": "synthetic", "dump": True, "shot": "tab-authorized"},
    imp("logoutApiButton", dump=True),
    imp("accountRealmFetchButton", dump=True),
    imp("charImportTreeClearJewels", dump=True, shot="tab-authorized-hover"),
    tapi("accountRealm", shot="tab-realm-open"),
    pick("import", "accountRealm", 2),
    AWAY | {"dump": True, "shot": "tab-realm-xbox"},
    tapi("logoutApiButton"),
    AWAY | {"dump": True, "shot": "tab-logged-out"},
])

# --- a code both ways, a code over this build and into a new one
GOLDEN = open(f'{ROOT}/spec/TestBuilds/3.13/OccVortex.xml', 'rb').read()
OTHER = open(f'{ROOT}/spec/TestBuilds/3.13/Dual Savior.xml', 'rb').read()
CODE = code_of(GOLDEN)
CODE_OTHER = code_of(OTHER)
FIXED = "U3ludGhldGljIGNvZGUgZm9yIHRoZSBzY3JlZW5zaG90"
write('code', [
    {"open": "OccVortex"},
    TAB,
    tapi("generateCode", perfClick="generateClick"),
    AWAY | {"dump": True},
    imp("generateCodeCopy", dump=True),
    tapi("generateCodeCopy"),
    AWAY | {"dump": True, "shot": "code-copied"},
    tapi("generateCode"),
    AWAY | {"setGenerated": FIXED, "dump": True, "shot": "code-generated"},
    imp("generateCodeByLink", dump=True),
    imp("exportFrom", dump=True),
    tapi("exportFrom", shot="code-export-open"),
    pick("import", "exportFrom", 3),
    AWAY | {"dump": True, "shot": "code-export-sel"},
    # a site's link in the out field
    AWAY | {"setGenerated": "https://pobb.in/abc", "dump": True, "shot": "code-out-link"},
    tapi("generateCodeOut"),
    {"events": [["KeyDown", "HOME"], ["KeyUp", "HOME"]], "dump": True},
    ctrl("a"),
    {"events": [["KeyDown", "BACK"], ["KeyUp", "BACK"]], "dump": True},
    AWAY | {"dump": True},
    # Ctrl+I focuses the field
    AWAY | ctrl("i"),
    paste(CODE_OTHER) | {"shot": "code-pasted"},
    imp("importCodeMode", dump=True, shot="code-mode-hover"),
    tapi("importCodeMode", shot="code-mode-open"),
    pick("import", "importCodeMode", 1),
    tapi("importCodeGo", shot="code-confirm"),
    {"cursorPopup": "1", "events": click(), "dump": True},
    tapi("importCodeGo"),
    {"cursorPopup": "confirm", "events": click(), "dump": True, "perfClick": "importThis"},
    AWAY | {"dump": True},
    AWAY | {"dump": True},
    TAB | {"shot": "code-imported"},
    # the import is unsaved; the file takes it
    AWAY | ctrl("s", file="OccVortex.xml", fileOut="code-saved.xml"),
    AWAY | {"dump": True},
    ctrl("i"),
    paste(CODE),
    imp("importCodeMode", events=click(), dump=True),
    pick("import", "importCodeMode", 2),
    tapi("importCodeGo", perfClick="importNew"),
    AWAY | {"dump": True},
    TAB | {"shot": "code-imported-new"},
    # an unsaved build forces the mode (ImportTab.lua:586-588)
    ctrl("i"),
    paste(CODE_OTHER),
    AWAY | {"dump": True, "shot": "code-new-mode"},
])

# --- what the field answers to
NOT_XML = code_of("this is no XML")
BAD_ROOT = code_of('<?xml version="1.0"?>\n<Foo/>')
BAD_CLOSE = code_of('<PathOfBuilding><a></b></PathOfBuilding>')
BAD_OPEN = code_of('<PathOfBuilding><a>')
write('errors', [
    {"open": "OccVortex"},
    TAB,
    AWAY | ctrl("i"),
    paste("abc") | {"shot": "errors-invalid"},
    ctrl("a"),
    paste(CODE[:200]) | {"shot": "errors-truncated"},
    ctrl("a"),
    paste(CODE + "AAAA"),
    ctrl("a"),
    paste("!!!!"),
    ctrl("a"),
    paste('{"character":{}}'),
    ctrl("a"),
    paste("https://pobb.in/abc") | {"shot": "errors-url"},
    ctrl("a"),
    paste("  https://pob.codes/b/abc  "),
    ctrl("a"),
    paste("https://maxroll.gg/poe/pob/abc"),
    ctrl("a"),
    paste("https://poe.ninja/poe1/pob/abc"),
    ctrl("a"),
    paste("https://pastebin.com/abc"),
    ctrl("a"),
    paste("https://pastebinp.com/abc"),
    ctrl("a"),
    paste("https://rentry.co/abc"),
    ctrl("a"),
    paste("https://poedb.tw/pob/abc"),
    ctrl("a"),
    paste("https://www.youtube.com/redirect?event=x&q=https%3A%2F%2Fpobb.in%2Fabc&v=y"),
    ctrl("a"),
    paste("https://www.google.com/url?q=https%3A%2F%2Fpastebin.com%2Fabc&sa=D"),
    ctrl("a"),
    {"events": [["KeyDown", "DELETE"], ["KeyUp", "DELETE"]], "dump": True},
    paste(NOT_XML),
    imp("importCodeGo", events=click(), dump=True),
    {"cursorPopup": "confirm", "events": click(), "dump": True, "shot": "errors-not-xml"},
])
write('errors2', [
    {"open": "OccVortex"},
    TAB,
    AWAY | ctrl("i"),
    paste(BAD_ROOT),
    imp("importCodeGo", events=click(), dump=True),
    {"cursorPopup": "confirm", "events": click(), "dump": True},
])
write('errors3', [
    {"open": "OccVortex"},
    TAB,
    AWAY | ctrl("i"),
    paste(BAD_CLOSE),
    imp("importCodeGo", events=click(), dump=True),
    {"cursorPopup": "confirm", "events": click(), "dump": True},
])
write('errors4', [
    {"open": "OccVortex"},
    TAB,
    AWAY | ctrl("i"),
    paste(BAD_OPEN),
    imp("importCodeGo", events=click(), dump=True),
    {"cursorPopup": "confirm", "events": click(), "dump": True},
])

# --- the JSON of a character: dev mode only (ImportTab.lua:611); its Import is not ported, the field's answers are
write('json-dev', [
    {"open": "OccVortex"},
    TAB,
    AWAY | ctrl("i"),
    paste('{"character":{"equipment":[],"passives":{}}}'),
    {"devMode": True} | ctrl("a"),
    paste('{"character":{"equipment":[],"passives":{}}}') | {"shot": "json-valid"},
    ctrl("a"),
    paste('{bad json}'),
    ctrl("a"),
    paste('{"other":1}'),
    ctrl("a"),
    paste('{"character":{"equipment":[]}}'),
    ctrl("a"),
    paste('{"character":{"passives":{}}}'),
    {"devMode": False} | ctrl("a"),
    {"events": [["KeyDown", "DELETE"], ["KeyUp", "DELETE"]], "dump": True},
])

# --- the Party tab
FIX = open('crates/pob-calc/tests/fixtures/party-synth/party-attack-full.xml', encoding='utf-8', newline='').read()
BUFFERS = re.findall(r'<ImportedBuffs name="([^"]+)">(.*?)</ImportedBuffs>', FIX, re.S)


def support(names, extra=''):
    body = ''.join(f'<ExportedBuffs name="{n}">{t}</ExportedBuffs>\n' for n, t in BUFFERS if n in names)
    return code_of(f'<?xml version="1.0" encoding="UTF-8"?>\n<PathOfBuilding>\n<Party>\n{body}{extra}</Party>\n</PathOfBuilding>')


ALL = support([n for n, _ in BUFFERS])
AURA = support(['Aura'])
CURSE = support(['Curse'])
ENEMY = support(['EnemyConditions', 'EnemyMods'])
STATS = support(['PartyMemberStats'])
NO_PARTY = code_of('<?xml version="1.0"?>\n<PathOfBuilding>\n<Notes/>\n</PathOfBuilding>')
write('party', [
    {"open": "OccVortex"},
    PARTY | {"shot": "party-tab"},
    par("importCodeDestination", dump=True, shot="party-hover-dest"),
    par("appendNotReplace", dump=True, shot="party-hover-append"),
    par("clear", dump=True, shot="party-hover-clear"),
    par("ShowAdvanceTools", dump=True, shot="party-hover-advanced"),
    par("removeEffects", dump=True, shot="party-hover-remove"),
    par("rebuild", dump=True, shot="party-hover-rebuild"),
    tapp("importCodeIn"),
    paste("abc") | {"shot": "party-invalid"},
    ctrl("a"),
    paste("https://pobb.in/abc"),
    ctrl("a"),
    paste(ALL) | {"shot": "party-code-valid"},
    tapp("importCodeGo", perfClick="partyImport"),
    AWAY | {"dump": True, "shot": "party-imported"},
    # the advanced view and its buffers
    tapp("ShowAdvanceTools", shot="party-advanced"),
    tapp("editWarcries", shot="party-focus-warcries"),
    {"events": typed("x"), "dump": True},
    ctrl("z"),
    tapp("editLinks", shot="party-focus-links"),
    tapp("editPartyMemberStats", shot="party-focus-stats"),
    tapp("enemyCond", shot="party-focus-cond"),
    tapp("enemyMods", shot="party-focus-mods"),
    tapp("editCurses", shot="party-focus-curses"),
    tapp("editAuras"),
    # an edit changes nothing in the calc until "Rebuild All"
    {"events": typed("Y") + [["KeyDown", "RETURN"], ["KeyUp", "RETURN"]], "dump": True},
    ctrl("z"),
    ctrl("y"),
    ctrl("a"),
    tapp("rebuild"),
    AWAY | {"dump": True},
    tapp("removeEffects"),
    AWAY | {"dump": True, "shot": "party-removed"},
    tapp("rebuild"),
    AWAY | {"dump": True, "shot": "party-rebuilt"},
    tapp("ShowAdvanceTools"),
    # the destination and Clear
    tapp("importCodeDestination", shot="party-dest-open"),
    pick("party", "importCodeDestination", 3),
    tapp("clear"),
    AWAY | {"dump": True, "shot": "party-clear-aura"},
    tapp("importCodeDestination"),
    pick("party", "importCodeDestination", 1),
    tapp("clear"),
    AWAY | {"dump": True, "shot": "party-clear-all"},
    # a saved file, with the check boxes
    tapp("appendNotReplace"),
    AWAY | ctrl("s", file="OccVortex.xml", fileOut="party-saved-empty.xml"),
    AWAY | {"dump": True},
])
write('party-import', [
    {"open": "OccVortex"},
    PARTY,
    tapp("importCodeIn"),
    paste(AURA),
    tapp("importCodeGo"),
    AWAY | {"dump": True},
    tapp("appendNotReplace"),
    # append: a second Aura, a Curse over nothing, the same again: Curse replaces
    tapp("importCodeIn"),
    ctrl("a"),
    paste(AURA),
    tapp("importCodeGo"),
    AWAY | {"dump": True, "shot": "party-append-aura"},
    tapp("importCodeIn"),
    ctrl("a"),
    paste(CURSE),
    tapp("importCodeGo"),
    AWAY | {"dump": True},
    tapp("importCodeIn"),
    ctrl("a"),
    paste(CURSE),
    tapp("importCodeGo"),
    AWAY | {"dump": True, "shot": "party-append-curse"},
    tapp("importCodeIn"),
    ctrl("a"),
    paste(ENEMY),
    tapp("importCodeGo"),
    AWAY | {"dump": True, "shot": "party-enemy"},
    tapp("importCodeIn"),
    ctrl("a"),
    paste(STATS),
    tapp("importCodeGo"),
    tapp("importCodeIn"),
    ctrl("a"),
    paste(STATS),
    tapp("importCodeGo"),
    AWAY | {"dump": True},
    # no <Party>: the replacing import clears and stops
    tapp("appendNotReplace"),
    tapp("importCodeIn"),
    ctrl("a"),
    paste(NO_PARTY),
    tapp("importCodeGo"),
    AWAY | {"dump": True},
    tapp("importCodeIn"),
    ctrl("a"),
    paste(CURSE),
    tapp("importCodeGo"),
    AWAY | ctrl("s", file="OccVortex.xml", fileOut="party-saved-curse.xml"),
    AWAY | {"dump": True},
])
write('party-load', [
    {"open": "party-attack-full"},
    PARTY | {"shot": "party-load"},
    tapp("ShowAdvanceTools", shot="party-load-advanced"),
    tapp("importCodeDestination"),
    pick("party", "importCodeDestination", 5),
    AWAY | ctrl("s", file="party-attack-full.xml", fileOut="party-load-saved.xml"),
    AWAY | {"dump": True},
    # the saved state back, through a list of builds
    {"open": "party-attack-full"},
    PARTY | {"shot": "party-reload"},
    tapp("ShowAdvanceTools"),
    tapp("removeEffects"),
    AWAY | {"dump": True},
    tapp("rebuild"),
    AWAY | {"dump": True},
    AWAY | ctrl("s", file="party-attack-full.xml", fileOut="party-reload-saved.xml"),
    AWAY | {"dump": True},
])
# a new build: no <Party> section, `lastContent.PartyMemberStats` stays nil and the tab counts as changed
write('party-new', [
    {"openNew": True},
    AWAY | {"dump": True},
    PARTY | {"shot": "party-new"},
    TAB | {"shot": "tab-new"},
    tapi("generateCode"),
    AWAY | {"dump": True},
])
# rough numbers for the report
write('perf', [
    {"open": "OccVortex"},
    TAB,
    AWAY | {"dump": False},
    {"perf": 200},
    {"perfGenerate": 20},
    AWAY | ctrl("i"),
    paste(CODE),
    {"perfImport": True},
    {"perfImport": True},
    {"perfImport": True},
    {"perf": 200},
    PARTY,
    {"perf": 200},
])
write('real-input', [{"open": "OccVortex"}, {"waitAssets": True}, {"realInput": True}])
json.dump(['tab', 'code', 'errors', 'errors2', 'errors3', 'errors4', 'party', 'party-import', 'party-load', 'party-new', 'json-dev'], open(f'{OUT}/index.json', 'w', newline='\n'))

# personal builds: ignored scenes from the ignored list, the names stay out of git
LOCAL = 'builds/local-builds.json'
if os.path.exists(LOCAL):
    for i, b in enumerate(json.load(open(LOCAL, encoding='utf-8')).get('import', [])):
        write(f'local-{i + 1}', [
            {"open": b},
            TAB | {"shot": f"local-{i + 1}-import"},
            tapi("generateCode"),
            AWAY | {"dump": True},
            PARTY | {"shot": f"local-{i + 1}-party"},
            tapp("ShowAdvanceTools"),
            AWAY | ctrl("s", file=f"{b}.xml", fileOut=f"local-{i + 1}-saved.xml"),
            AWAY | {"dump": True},
        ])
