# U8d: the scenes of Trade and Buy similar. Run from the repo root: python tools/sg-oracle/scenes/trade/gen.py
# Nothing here came from a site: every reply is written by hand from what upstream's code reads (no real account, item listing or token). A scene's "net" is what the fake transport answers on BOTH sides:
# trade-runner.lua replaces launch:DownloadPage, LaunchSubScript and lcurl with it, crates/pob-ui/tests/trade.rs gives pob_net::ScriptedTransport the same list.
import json

OUT = 'tools/sg-oracle/scenes/trade'
HOST = 'https://www.pathofexile.com/'
LEAGUES_URL = f'{HOST}api/trade/data/leagues'
STATIC_URL = f'{HOST}api/trade/data/static'
SEARCH_STD = f'{HOST}api/trade/search/Standard'
FETCH = f'{HOST}api/trade/fetch/*'
# (floor(1800000000 / 3600) - 1) * 3600 (TradeQuery.lua:153-154)
CX_URL = 'https://web.poecdn.com/api/currency-exchange/1799996400'
CLOCK = {'osTime': 1800000000}
TOKENS = {'apiTokens': {'auth': 'synthetic-access', 'refresh': 'synthetic-refresh', 'expiry': 1900000000}}


def j(v):
    return json.dumps(v, ensure_ascii=False, separators=(',', ':'))


def click(k='LEFTBUTTON'):
    return [['KeyDown', k], ['KeyUp', k]]


def ok(url, body, header='HTTP/1.1 200 OK\r\n\r\n'):
    return {'url': url, 'code': 200, 'header': header, 'body': body if isinstance(body, str) else j(body)}


def fail(url, code, body='', header=None):
    return {'url': url, 'code': code, 'header': header or f'HTTP/1.1 {code} X\r\n\r\n', 'body': body}


def err(url, msg):
    return {'url': url, 'error': msg}


def pop(c, **k):
    return {'cursorPopup': c, 'events': click(), 'dump': True, **k}


def drop(c, row, **k):
    return {'cursorPopupDrop': [c, row], 'events': click(), 'dump': True, **k}


def typed(text):
    return [['Char', c] for c in text]


LEAGUES = {'result': [
    {'id': 'Standard', 'realm': 'pc', 'text': 'Standard'},
    {'id': 'Hardcore', 'realm': 'pc', 'text': 'Hardcore'},
    {'id': 'Synth League', 'realm': 'pc', 'text': 'Synth League'},
    {'id': 'Hardcore Synth League', 'realm': 'pc', 'text': 'Hardcore Synth League'},
    {'id': 'Standard', 'realm': 'xbox', 'text': 'Standard'},
    {'id': 'Hardcore', 'realm': 'xbox', 'text': 'Hardcore'},
    {'id': 'Standard', 'realm': 'sony', 'text': 'Standard'},
    {'id': 'Sony Only', 'realm': 'sony', 'text': 'Sony Only'},
]}
STATIC = {'result': [{'id': 'Currency', 'label': 'Currency', 'entries': [
    {'id': 'sep', 'text': ''}, {'id': 'chaos', 'text': 'Chaos Orb'}, {'id': 'divine', 'text': 'Divine Orb'}, {'id': 'exalted', 'text': 'Exalted Orb'}, {'id': 'unknowncoin', 'text': 'No Such Currency'},
]}]}
CHAOS = 'Metadata/Items/Currency/CurrencyRerollRare'
DIVINE = 'Metadata/Items/Currency/CurrencyModValues'
EXALTED = 'Metadata/Items/Currency/CurrencyAddModToRare'
CX = {'markets': [
    {'league': 'Standard', 'market_pair': [CHAOS, DIVINE], 'lowest_ratio': {CHAOS: 200, DIVINE: 1}, 'highest_stock': {CHAOS: 5000, DIVINE: 40}},
    {'league': 'Standard', 'market_pair': [EXALTED, CHAOS], 'lowest_ratio': {EXALTED: 1, CHAOS: 12}, 'highest_stock': {EXALTED: 100, CHAOS: 900}},
    {'league': 'Standard', 'market_pair': [EXALTED, CHAOS], 'lowest_ratio': {EXALTED: 1, CHAOS: 15}, 'highest_stock': {EXALTED: 10, CHAOS: 90}},
    {'league': 'Empty League', 'market_pair': [EXALTED, CHAOS], 'lowest_ratio': {EXALTED: 1, CHAOS: 12}, 'highest_stock': {EXALTED: 1, CHAOS: 9}},
]}
OPEN_NET = [ok(LEAGUES_URL, LEAGUES), ok(STATIC_URL, STATIC), ok(CX_URL, CX), ok(SEARCH_STD, '{"id":"first","total":1,"result":["x"]}', 'HTTP/1.1 200 OK\r\n\r\n')]
ITEMS_TAB = {'cursorBuild': 'modeItems', 'events': click(), 'dump': True}
PRICE = {'cursorItems': 'priceDisplayItem', 'events': click(), 'dump': True}
DELIVER = {'netDeliver': True, 'dump': True}
GOLDEN = 'OccVortex'


def limits(policy='trade-search-request-limit', state='1:10:0,1:60:0,1:300:0', account='1:5:0'):
    return ('HTTP/1.1 200 OK\r\n'
            f'X-Rate-Limit-Policy: {policy}\r\n'
            'X-Rate-Limit-Rules: Ip,Account\r\n'
            'X-Rate-Limit-Ip: 8:10:60,15:60:120,60:300:1800\r\n'
            f'X-Rate-Limit-Ip-State: {state}\r\n'
            'X-Rate-Limit-Account: 30:5:60\r\n'
            f'X-Rate-Limit-Account-State: {account}\r\n\r\n')


FETCH_LIMITS = limits('trade-fetch-request-limit')


def listing(amount, currency, ptype='~price', whisper='@Synth hello', account='SynthSeller'):
    return {'price': {'amount': amount, 'currency': currency, 'type': ptype}, 'whisper': whisper, 'account': {'name': account}}


def ring(i, life, res, amount, currency, **k):
    item = {'rarity': 'Rare', 'name': f'Synth Band {i}', 'typeLine': 'Coral Ring', 'ilvl': 80,
            'implicitMods': [{'description': '+25 to maximum Life'}],
            'explicitMods': [{'description': f'+{life} to maximum Life'}, {'description': f'+{res}% to Fire Resistance', 'flags': {'crafted': True}}],
            'pseudoMods': [{'description': f'Sum: {100 + i}.5'}]}
    lst = k.pop('listing', None) or listing(amount, currency)
    item.update(k)
    return {'id': f'h{i}', 'item': item, 'listing': lst}


RINGS = [
    ring(1, 40, 20, 3, 'chaos'),
    ring(2, 90, 40, 2, 'exalted', listing=listing(2, 'exalted', '~b/o', whisper=None)),
    ring(3, 70, 30, 1, 'divine', influences={'crusader': True}),
    ring(4, 60, 10, 10, 'chaos', duplicated=True),
    ring(5, 10, 5, 1, 'chaos', corrupted=True, sockets=[{'sColour': 'W'}]),
    {'id': 'h6', 'item': {'rarity': 'Rare', 'name': 'Not A Ring', 'typeLine': 'Glorious Plate', 'ilvl': 80, 'explicitMods': [{'description': '+10 to maximum Life'}]}, 'listing': listing(1, 'chaos')},
    {'id': 'h7', 'item': {'rarity': 'Rare', 'name': 'No Base', 'typeLine': 'A Base That Does Not Exist', 'ilvl': 80}, 'listing': listing(1, 'chaos')},
]
HASHES = [f'h{i + 1}' for i in range(25)]


def write(name, steps, net=None, names=[]):
    json.dump({'net': net or [], 'steps': steps}, open(f'{OUT}/{name}.json', 'w', newline='\n'), indent=0)
    names.append(name)
    return names


def opened(build=GOLDEN, auth=False, league='Synth League'):
    s = [{'open': build}, CLOCK]
    if auth:
        s.append(TOKENS)
    s += [ITEMS_TAB, PRICE, DELIVER, {'dump': True}, DELIVER, DELIVER, {'popupDropSel': ['league', league], 'dump': True}]
    return s


# --- the popup, its realm and league, the first requests
write('open', [
    *opened(),
    {'cursor': [0, 0], 'dump': True},
    {'dump': True, 'shot': 'open-popup'},
    pop('realm'),
    drop('realm', 2),
    DELIVER,
    DELIVER,
    DELIVER,
    pop('league'),
    drop('league', 1),
    pop('close'),
    PRICE,
    pop('close'),
], OPEN_NET + [ok(STATIC_URL, STATIC), ok(CX_URL.replace('exchange/', 'exchange/sony/'), CX)])
# the failures of those requests
write('open-errors', [
    {'open': GOLDEN},
    CLOCK,
    ITEMS_TAB,
    PRICE,
    DELIVER,
    {'dump': True},
    DELIVER,
    DELIVER,
    {'dump': True},
], [err(LEAGUES_URL, 'synthetic failure'), fail(STATIC_URL, 500), fail(SEARCH_STD, 403)])
write('open-bad-leagues', [
    {'open': GOLDEN},
    CLOCK,
    ITEMS_TAB,
    PRICE,
    DELIVER,
    {'dump': True},
    DELIVER,
    DELIVER,
], [ok(LEAGUES_URL, {'error': {'code': 3, 'message': 'synthetic'}}), ok(LEAGUES_URL, 'not json'), ok(LEAGUES_URL, {'error': 'text'}), ok(STATIC_URL, 'not json')])
# R58: a build with no <Items>: the list is made by the popup, the saved build has it from then on
write('new-build', [
    {'openNew': True},
    CLOCK,
    {'dump': True, 'xml': True},
    ITEMS_TAB,
    PRICE,
    {'dump': True, 'xml': True},
    DELIVER,
    pop('close', xml=True),
], OPEN_NET)

# --- the weights
write('weights', [
    *opened(),
    pop('StatWeightMultipliersButton', shot='weights-popup'),
    {'cursorPopupAt': ['ListControl', 40, 28], 'events': click(), 'dump': True},
    pop('Slider'),
    {'cursorPopupAt': ['ListControl', 40, 60], 'events': click(), 'dump': True},
    {'cursorPopupAt': ['Slider', 20, 8], 'events': click(), 'dump': True},
    {'dump': True, 'shot': 'weights-changed'},
    pop('finalise', xml=True),
    pop('StatWeightMultipliersButton'),
    pop('reset'),
    pop('cancel', xml=True),
    pop('StatWeightMultipliersButton'),
    pop('reset'),
    pop('reset'),
    pop('finalise', xml=True),
    pop('close'),
], OPEN_NET)


# --- the query: a row, its options, its weights. Unauthenticated: the URL lands in the row's field.
def query_scene(name, row, options=(), build=GOLDEN, shot=None, net=OPEN_NET, execute=True, league='Synth League'):
    s = opened(build, league=league)
    s += [pop(f'bestButton{row}', **({'shot': shot} if shot else {}))]
    s += list(options)
    if execute:
        s += [pop('generateQuery', perfClick='query'), {'dump': True}]
    write(name, s, net)


query_scene('query-ring', 8, shot='query-options')
query_scene('query-body', 4, options=[pop('includeEldritch'), drop('includeEldritch', 2)])
query_scene('query-amulet', 7, options=[pop('includeTalisman'), pop('includeEldritch'), drop('includeEldritch', 4), pop('copyEnchantMode'), drop('copyEnchantMode', 2),
                                        pop('includeCorrupted'), pop('includeMirrored')])
query_scene('query-weapon', 1, options=[{'cursorPopup': 'maxPrice', 'events': click()}, {'events': typed('12'), 'dump': True}, pop('maxPriceType'), drop('maxPriceType', 3),
                                        {'cursorPopup': 'maxLevel', 'events': click()}, {'events': typed('70'), 'dump': True},
                                        {'cursorPopup': 'sockets', 'events': click()}, {'events': typed('6'), 'dump': True},
                                        {'cursorPopup': 'links', 'events': click()}, {'events': typed('5'), 'dump': True}])
query_scene('query-standard', 10, options=[pop('includeScourge')], league='Standard')
query_scene('query-selectors', 9, options=[pop('modSelector1'), drop('modSelector1', 3), {'cursorPopup': 'modSelectorMin1', 'events': click()}, {'events': typed('10'), 'dump': True},
                                           pop('modSelector2'), drop('modSelector2', 4),
                                           pop('modSelectorClear1'),
                                           pop('modNotSelector1'), drop('modNotSelector1', 5)])
query_scene('query-cancel', 8, execute=False, options=[pop('cancel')])
query_scene('query-flask', 11)
# the rows after the sockets: named by the first run of the popup (`open`)
UNIQUE = 19
query_scene('query-megalomaniac', UNIQUE)
query_scene('query-watchers', UNIQUE + 1, options=[pop('includeAllWEMods')])
query_scene('query-pearl', UNIQUE + 2)
query_scene('query-socket', 16, options=[pop('jewelType'), drop('jewelType', 2)])

# --- the search: logged in. The league: the first of the sorted list, the realm's own request came last
SEARCH_LG = f'{HOST}api/trade/search/Synth%20League'
# a block of a fetch is complete when as many items came as were asked (TradeQueryRequests.lua:289): the hashes here are as many as the items of the reply
SEARCH_OK = ok(SEARCH_LG, {'id': 'synthq1', 'total': 25, 'result': HASHES[:7]}, limits())
ITEMS_BODY = {'result': RINGS}
FETCH_OK = ok(FETCH, ITEMS_BODY, FETCH_LIMITS)
FETCH_EMPTY = ok(FETCH, {'result': []}, FETCH_LIMITS)
AUTH_NET = OPEN_NET + [SEARCH_OK, FETCH_OK, FETCH_EMPTY]
FRAME = {'dump': True}


def searching(row=8):
    return [pop(f'bestButton{row}'), pop('generateQuery'), {'dump': True}, DELIVER, DELIVER, DELIVER, DELIVER, DELIVER, DELIVER]


write('search', [
    *opened(auth=True),
    DELIVER,
    FRAME,
    *searching(),
    {'dump': True, 'shot': 'search-results'},
    pop('resultDropdown8'),
    drop('resultDropdown8', 2),
    pop('itemSortSelection'),
    drop('itemSortSelection', 2),
    pop('itemSortSelection'),
    drop('itemSortSelection', 3),
    pop('itemSortSelection'),
    drop('itemSortSelection', 4),
    pop('itemSortSelection'),
    drop('itemSortSelection', 1),
    pop('whisperButton8', clipboard='fixed'),
    pop('resultDropdown8'),
    drop('resultDropdown8', 1),
    pop('whisperButton8', clipboard='fixed'),
    pop('importButton8', xml=True),
    pop('StatWeightMultipliersButton'),
    pop('cancel'),
    pop('changeButton8'),
    pop('close'),
], AUTH_NET)
# the limits: 429 with and without Retry-After, a limit reached, the countdown
LIM = 'HTTP/1.1 429 Too Many Requests\r\nRetry-After: 30\r\nX-Rate-Limit-Policy: trade-search-request-limit\r\nX-Rate-Limit-Rules: Ip\r\nX-Rate-Limit-Ip: 8:10:60\r\nX-Rate-Limit-Ip-State: 8:10:30\r\n\r\n'
LIM0 = 'HTTP/1.1 429 Too Many Requests\r\nX-Rate-Limit-Policy: trade-fetch-request-limit\r\nX-Rate-Limit-Rules: Ip\r\nX-Rate-Limit-Ip: 8:10:60\r\nX-Rate-Limit-Ip-State: 9:10:0\r\n\r\n'
write('limits', [
    *opened(auth=True),
    DELIVER,
    FRAME,
    pop('bestButton8'),
    pop('generateQuery'),
    FRAME,
    DELIVER | {'shot': 'limits-countdown'},
    FRAME,
    {'osTime': 1800000010, 'dump': True},
    {'osTime': 1800000031, 'dump': True},
    {'osTime': 1800000040, 'dump': True},
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    {'osTime': 1800000050, 'dump': True},
    FRAME,
    DELIVER,
    FRAME,
    {'osTime': 1800000200, 'dump': True},
    DELIVER,
    FRAME,
    DELIVER,
], OPEN_NET + [
    fail(SEARCH_LG, 429, '{}', LIM),
    SEARCH_OK,
    fail(FETCH, 429, '{}', LIM0),
    FETCH_OK,
    FETCH_EMPTY,
])
# the answers of the search: nothing found, an API error, 401 invalid_token, not JSON, an expired login, the fetch's failures
write('search-errors', [
    *opened(auth=True),
    DELIVER,
    FRAME,
    pop('bestButton8'),
    pop('generateQuery'),
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    {'dump': True, 'shot': 'search-no-results'},
    pop('bestButton8'),
    pop('generateQuery'),
    FRAME,
    DELIVER,
    FRAME,
    pop('bestButton8'),
    pop('generateQuery'),
    FRAME,
    DELIVER,
    FRAME,
    pop('bestButton8'),
    pop('generateQuery'),
    FRAME,
    DELIVER,
    FRAME,
    pop('bestButton8'),
    pop('generateQuery'),
    FRAME,
    DELIVER,
    FRAME,
], OPEN_NET + [
    ok(SEARCH_LG, {'id': 'none', 'total': 0, 'result': []}, limits()),
    ok(SEARCH_LG, {'id': 'none', 'total': 0, 'result': []}, limits()),
    ok(SEARCH_LG, {'id': 'none', 'total': 0, 'result': []}, limits()),
    ok(SEARCH_LG, {'id': 'none', 'total': 0, 'result': []}, limits()),
    ok(SEARCH_LG, {'id': 'none', 'total': 0, 'result': []}, limits()),
    fail(SEARCH_LG, 400, j({'error': {'code': 2, 'message': 'Invalid query'}}), limits().replace('200 OK', '400 Bad')),
    fail(SEARCH_LG, 400, j({'error': {'code': 3, 'message': 'Logging in will increase this limit'}}), limits().replace('200 OK', '400 Bad')),
    ok(SEARCH_LG, 'not json', limits()),
    fail(SEARCH_LG, 401, '{"error":"invalid_token"}', limits().replace('200 OK', '401 No')),
])
# the search that grows: clipped at 10000 and halved, then filled from the first batch
write('search-adjust', [
    *opened(auth=True),
    DELIVER,
    FRAME,
    pop('bestButton8'),
    pop('generateQuery'),
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
], OPEN_NET + [
    ok(SEARCH_LG, {'id': 'clip', 'total': 10000, 'result': HASHES[:12]}, limits()),
    ok(SEARCH_LG, {'id': 'few', 'total': 3, 'result': HASHES[1:4]}, limits()),
    ok(SEARCH_LG, {'id': 'few2', 'total': 3, 'result': HASHES[1:4]}, limits()),
    ok(SEARCH_LG, {'id': 'few3', 'total': 3, 'result': HASHES[1:4]}, limits()),
    ok(SEARCH_LG, {'id': 'few4', 'total': 3, 'result': HASHES[1:4]}, limits()),
    ok(FETCH, {'result': RINGS[:5] + [ring(i, 20 + i, 10, i, 'chaos') for i in range(8, 13)]}, FETCH_LIMITS),
    ok(FETCH, {'result': RINGS[1:4]}, FETCH_LIMITS),
    ok(FETCH, {'result': [ring(13, 33, 11, 4, 'chaos'), ring(14, 44, 12, 5, 'chaos')]}, FETCH_LIMITS),
])
# a pasted URL (the query of `Sum` weight, and one without): the code is base64 of a gzip stream, made here
import base64, gzip
def code(q):
    return base64.b64encode(gzip.compress(j(q).encode())).decode().replace('+', '-').replace('/', '_')
Q1 = {'status': {'option': 'online'}, 'stats': [{'type': 'and', 'filters': []}, {'type': 'weight', 'value': {'min': 5}, 'filters': [{'id': 'explicit.stat_3299347043', 'value': {'weight': 1.5}}]}]}
Q2 = {'status': {'option': 'online'}, 'stats': [{'type': 'and', 'filters': []}]}
SEARCH_XBOX = f'{HOST}api/trade/search/xbox/Hardcore%20Synth'
def paste(url):
    return [{'cursorPopup': 'uri8', 'events': click()}, {'clipboard': url, 'hold': ['CTRL'], 'events': [['KeyDown', 'v'], ['KeyUp', 'v']]}, {'release': ['CTRL'], 'dump': True}]
write('price-url', [
    *opened(auth=True),
    DELIVER,
    FRAME,
    *paste(f'{HOST}trade/search/Standard/{code(Q1)}'),
    pop('priceButton8'),
    FRAME,
    DELIVER,
    FRAME,
    DELIVER,
    FRAME,
    pop('changeButton8'),
    {'cursorPopup': 'uri8', 'events': click()},
    {'hold': ['CTRL'], 'events': [['KeyDown', 'a'], ['KeyUp', 'a'], ['KeyDown', 'BACK'], ['KeyUp', 'BACK']]},
    {'release': ['CTRL'], 'dump': True},
    *paste(f'https://pathofexile.com/trade/search/xbox/Hardcore%20Synth/{code(Q2)}'),
    pop('priceButton8'),
    FRAME,
    DELIVER,
    FRAME,
    {'cursorPopup': 'uri8', 'events': click()},
    {'hold': ['CTRL'], 'events': [['KeyDown', 'a'], ['KeyUp', 'a'], ['KeyDown', 'BACK'], ['KeyUp', 'BACK']]},
    {'release': ['CTRL'], 'dump': True},
    *paste(f'{HOST}trade/search/Standard/notacode'),
    pop('priceButton8'),
    FRAME,
], OPEN_NET + [
    ok(SEARCH_STD, {'id': 'pasted', 'total': 7, 'result': HASHES[:7]}, limits()),
    FETCH_OK,
    ok(SEARCH_XBOX, {'id': 'pasted2', 'total': 0, 'result': []}, limits()),
])

# --- Buy similar, from the Items tab
RARE = """Rarity: RARE
Synth Grip
Sorcerer Gloves
Quality: 20
Energy Shield: 80
Sockets: B-B-B-B
Item Level: 84
Implicits: 1
+25 to maximum Life
+40 to maximum Energy Shield
{crafted}+30% to Fire Resistance
+30% to Cold Resistance
10% increased Attack Speed
Adds 5 to 10 Physical Damage to Attacks
This Line Means Nothing To The Trade Site"""
UNIQUE_ITEM = """Rarity: UNIQUE
Foulborn Tabula Rasa
Simple Robe
Sockets: W-W-W-W-W-W
Item has no level requirement and Energy Shield (Hidden)
Implicits: 0
Item has no Level Requirement"""
LEAGUE_ROWS = [ok(LEAGUES_URL, LEAGUES)]
BUY_ROWS = [1, 2, 3, 4, 5, 6, 7, 8]
write('buy-rare', [
    {'open': GOLDEN},
    CLOCK,
    ITEMS_TAB,
    {'displayItem': RARE, 'dump': True},
    {'cursorItems': 'displayItemBuySimilar', 'events': click(), 'dump': True},
    {'dump': True, 'shot': 'buy-rare-loading'},
    DELIVER | {'shot': 'buy-rare'},
    pop('baseTypeCheck'),
    pop('mod1Check'),
    pop('mod2Check'),
    pop('mod3Check'),
    {'cursorPopup': 'ilvlMin', 'events': click()},
    {'events': typed('80'), 'dump': True},
    pop('def1Check'),
    {'cursorPopup': 'mod2Min', 'events': click()},
    {'events': typed('5'), 'dump': True},
    pop('listedDrop'),
    drop('listedDrop', 3),
    pop('leagueDrop'),
    drop('leagueDrop', 2),
    pop('realmDrop'),
    drop('realmDrop', 2),
    DELIVER,
    pop('search'),
    pop('close'),
    {'cursorItems': 'displayItemBuySimilar', 'events': click(), 'dump': True},
    DELIVER,
    pop('close'),
], LEAGUE_ROWS)
write('buy-unique', [
    {'open': GOLDEN},
    CLOCK,
    ITEMS_TAB,
    {'displayItem': UNIQUE_ITEM, 'dump': True},
    {'cursorItems': 'displayItemBuySimilar', 'events': click(), 'dump': True},
    DELIVER | {'shot': 'buy-unique'},
    pop('search'),
    pop('close'),
], [err(LEAGUES_URL, 'synthetic failure')])

# --- from the Compare tab: the Buy button of a row of the Items view
steps = [{'open': 'Dual Savior'}, CLOCK, {'cursorBuild': 'modeCompare', 'events': click(), 'dump': True}, {'compareImport': ['Mirage Archer Toxic Rain', 'Mirage']},
         {'cursorCompare': 'subTabItems', 'events': click(), 'dump': True}, {'compareScrollX': 300}]
for i, row in enumerate(BUY_ROWS):
    # a row without a compared item has no popup: ESCAPE closes what there is
    steps += [{'compareItemButton': [row, 'buy'], 'events': click(), 'dump': True, **({'shot': 'buy-compare'} if i == 0 else {})}, DELIVER]
    if i == 0:
        steps += [pop('search')]
    steps += [{'events': [['KeyDown', 'ESCAPE'], ['KeyUp', 'ESCAPE']], 'dump': True}]
write('buy-compare', steps, LEAGUE_ROWS)

# --- a build with more rows than the popup has room for: the scroll bar (TradeQuery.lua:576-592, :656-660)
WHEEL = [['KeyDown', 'WHEELDOWN'], ['KeyUp', 'WHEELDOWN']]
UP = [['KeyDown', 'WHEELUP'], ['KeyUp', 'WHEELUP']]
write('scroll', [
    *opened('Generals Perforate Zerker'),
    {'cursor': [640, 400], 'dump': True, 'shot': 'scroll-top'},
    {'events': WHEEL, 'dump': True},
    {'events': WHEEL, 'dump': True},
    {'events': WHEEL + WHEEL + WHEEL, 'dump': True, 'shot': 'scroll-middle'},
    {'events': WHEEL + WHEEL + WHEEL + WHEEL + WHEEL + WHEEL + WHEEL + WHEEL, 'dump': True, 'shot': 'scroll-end'},
    {'events': UP, 'dump': True},
    pop('close'),
], OPEN_NET)

# --- what a hover shows
write('hover', [
    *opened(auth=True),
    DELIVER,
    FRAME,
    {'cursorPopup': 'tradeAuthButton', 'dump': True, 'shot': 'hover-auth'},
    {'cursorPopup': 'fetchCountEdit', 'dump': True, 'shot': 'hover-fetch'},
    {'cursorPopup': 'StatWeightMultipliersButton', 'dump': True, 'shot': 'hover-weights'},
    {'cursorPopup': 'itemSortSelection', 'dump': True, 'shot': 'hover-sort'},
    {'cursorPopup': 'bestButton8', 'dump': True, 'shot': 'hover-best'},
    {'cursorPopup': 'bestButton16', 'dump': True},
    {'waitAssets': True},
    {'dump': True, 'shot': 'hover-best-socket'},
    {'cursorPopup': 'uri8', 'dump': True, 'shot': 'hover-uri'},
    {'cursorPopup': 'priceButton8', 'dump': True, 'shot': 'hover-price'},
    pop('realm', shot='hover-realm-open'),
    {'cursor': [0, 0], 'events': click(), 'dump': True},
    *searching(),
    {'cursorPopup': 'priceButton8', 'dump': True},
    {'cursorPopup': 'resultDropdown8', 'dump': True, 'shot': 'hover-result'},
    pop('resultDropdown8', shot='hover-result-open'),
    {'cursorPopupDrop': ['resultDropdown8', 2], 'dump': True, 'shot': 'hover-result-row'},
    {'cursor': [0, 0], 'events': click(), 'dump': True},
    {'cursorPopup': 'importButton8', 'dump': True, 'shot': 'hover-import'},
    {'cursorPopup': 'whisperButton8', 'dump': True, 'shot': 'hover-whisper'},
    {'cursorPopup': 'bestButton8', 'dump': True},
], AUTH_NET)

# --- the rows that are not a slot, logged in: the evaluation of each kind
SUFFIX_NOTABLES = ['Aerodynamics', 'Blanketed Snow', 'Blowback']
MEGA = [{'id': f'm{i}', 'item': {'rarity': 'Unique', 'name': 'Megalomaniac', 'typeLine': 'Medium Cluster Jewel', 'ilvl': 80,
                                 'implicitMods': [], 'enchantMods': [{'description': 'Adds 4 Passive Skills'}, {'description': 'Added Small Passive Skills grant: Nothing'}],
                                 'explicitMods': [{'description': f'1 Added Passive Skill is {n}'} for n in names],
                                 'pseudoMods': [{'description': f'Sum: {10 + i}'}]}, 'listing': listing(i + 1, 'chaos')}
        for i, names in enumerate([SUFFIX_NOTABLES, SUFFIX_NOTABLES[1:] + SUFFIX_NOTABLES[:1]])]
WATCHERS = [{'id': 'w0', 'item': {'rarity': 'Unique', 'name': "Watcher's Eye", 'typeLine': 'Prismatic Jewel', 'ilvl': 80,
                                  'explicitMods': [{'description': '6% increased maximum Energy Shield'}, {'description': '5% increased maximum Life'}, {'description': '5% increased maximum Mana'},
                                                   {'description': '50% increased Fire Damage while affected by Anger'}],
                                  'pseudoMods': [{'description': 'Sum: 77'}]}, 'listing': listing(4, 'exalted')}]
PEARLS = [{'id': 'p0', 'item': {'rarity': 'Unique', 'name': 'Pearl of Tsoatha', 'typeLine': 'Prismatic Ring', 'ilvl': 80,
                                'implicitMods': [{'description': '+10% to all Elemental Resistances'}], 'explicitMods': [{'description': '+30 to maximum Life'}],
                                'pseudoMods': [{'description': 'Sum: 5'}]}, 'listing': listing(1, 'divine', '~b/o', whisper=None)}]
JEWELS = [{'id': 'j0', 'item': {'rarity': 'Rare', 'name': 'Synth Spark', 'typeLine': 'Cobalt Jewel', 'ilvl': 80, 'explicitMods': [{'description': '10% increased maximum Life'}, {'description': '20% increased Spell Damage'}],
                                'pseudoMods': [{'description': 'Sum: 9'}]}, 'listing': listing(2, 'chaos')}]


def row_search(name, row, reply, options=(), extra=(), build=GOLDEN, pre=()):
    n = len(reply['result'])
    net = OPEN_NET + [ok(SEARCH_LG, {'id': 'r1', 'total': 25, 'result': HASHES[:n]}, limits()), ok(FETCH, reply, FETCH_LIMITS)]
    write(name, [
        *opened(build, auth=True),
        DELIVER,
        FRAME,
        *pre,
        pop(f'bestButton{row}', dump=True),
        *options,
        pop('generateQuery'),
        FRAME,
        DELIVER,
        FRAME,
        DELIVER,
        FRAME,
        DELIVER,
        FRAME,
        {'cursorPopup': f'importButton{row}', 'dump': True},
        pop(f'importButton{row}', xml=True),
        *extra,
        {'events': [['KeyDown', 'ESCAPE'], ['KeyUp', 'ESCAPE']], 'dump': True},
    ], net)


row_search('row-megalomaniac', 19, {'result': MEGA})
row_search('row-megalomaniac2', 28, {'result': MEGA}, build='Generals Perforate Zerker', pre=[{'cursor': [640, 400], 'events': WHEEL + WHEEL + WHEEL + WHEEL + WHEEL + WHEEL, 'dump': True}])
row_search('row-watchers', 20, {'result': WATCHERS}, options=[pop('jewelSlot'), drop('jewelSlot', 2)])
row_search('row-pearl', 21, {'result': PEARLS}, options=[pop('ringSlot'), drop('ringSlot', 2)])
row_search('row-socket', 16, {'result': JEWELS})
row_search('row-amulet-copy', 7, {'result': [{'id': 'a0', 'item': {'rarity': 'Rare', 'name': 'Synth Choker', 'typeLine': 'Onyx Amulet', 'ilvl': 80,
                                                                   'enchantMods': [{'description': 'Allocates Acrimony'}],
                                                                   'implicitMods': [{'description': '+16 to all Attributes'}], 'explicitMods': [{'description': '+40 to maximum Life'}]}, 'listing': listing(9, 'chaos')}]},
           options=[pop('copyEnchantMode'), drop('copyEnchantMode', 2), pop('includeEldritch'), drop('includeEldritch', 1)])
row_search('row-helmet-remove', 3, {'result': [{'id': 'h0', 'item': {'rarity': 'Rare', 'name': 'Synth Crown', 'typeLine': 'Hubris Circlet', 'ilvl': 80, 'searing': True,
                                                                     'implicitMods': [{'description': '+20 to maximum Life'}], 'explicitMods': [{'description': '+80 to maximum Energy Shield'}]}, 'listing': listing(9, 'chaos')}]},
           options=[pop('includeEldritch'), drop('includeEldritch', 4)])

# --- the login from the popup, the logout, a token that expired
TOKEN_URL = f'{HOST}oauth/token'
TOKENS_REPLY = j({'access_token': 'synthetic-access', 'refresh_token': 'synthetic-refresh', 'expires_in': 3600})
write('login', [
    *opened(),
    {'cursorPopup': 'tradeAuthButton', 'events': click(), 'dump': True, 'shot': 'login-waiting'},
    {'osTime': 1800000030, 'dump': True},
    {'netDeliver': True, 'dump': True},
    DELIVER,
    DELIVER,
    FRAME,
    pop('tradeAuthButton', shot='login-out'),
    pop('close'),
], OPEN_NET + [{'url': 'oauth', 'oauth': {'code': 'synthetic-code', 'port': 49082}}, ok(TOKEN_URL, TOKENS_REPLY)])
write('login-timeout', [
    *opened(),
    {'cursorPopup': 'tradeAuthButton', 'events': click(), 'dump': True},
    {'osTime': 1800000061, 'dump': True},
    {'dump': True},
    {'netDeliver': True, 'dump': True},
    pop('close'),
], OPEN_NET + [{'url': 'oauth', 'oauth': {'err': 'synthetic', 'port': 0}}])
write('expired', [
    {'open': GOLDEN},
    CLOCK,
    {'apiTokens': {'auth': 'synthetic-access', 'refresh': 'synthetic-refresh', 'expiry': 1}},
    ITEMS_TAB,
    PRICE,
    DELIVER,
    DELIVER,
    DELIVER,
    pop('close'),
], OPEN_NET + [err(TOKEN_URL, 'synthetic refused')])

# a probe outside index.json (POB_TRADE_SCENES=probe-rows): how many rows has each golden
PROBE = [CLOCK]
for b in ['Generals Perforate Zerker', 'Dual Savior', 'Dual Wield Cospris CoC', 'Mirage Archer Toxic Rain']:
    PROBE += [{'open': b}, ITEMS_TAB, PRICE, pop('close')]
json.dump({'net': OPEN_NET, 'steps': PROBE}, open(f'{OUT}/probe-rows.json', 'w', newline='\n'), indent=0)

# the real window, outside index.json: inject-trade-real.ps1 and trade-rust-input.ps1
json.dump({'net': OPEN_NET, 'steps': [{'open': GOLDEN}, {'realInput': True}]}, open(f'{OUT}/real-input.json', 'w', newline='\n'), indent=0)

names = write.__defaults__[1]
json.dump(names, open(f'{OUT}/index.json', 'w', newline='\n'))
print(names)
