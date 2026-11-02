// R3.8a: LuaJIT-truth corpora for R3.8b (fix pob-build's XML/ConfigTab to match Lua, MINEFIELD-RUST R6).
//  (1) crates/pob-build/tests/fixtures/xml-lua.json   : synthetic XML -> runtime/lua/xml.lua ParseXML tree | ["ERR"|"THROW", msg];
//      engines: desktop LuaJIT (tools/luajit-win, PoB runtime/lua51.dll, plain dofile of xml.lua) AND docker LuaJIT (cross-check; must agree).
//  (2) crates/pob-build/tests/fixtures/config-load.json: synthetic full builds (Dual Savior.xml with varied <Build> attrs / <Config>)
//      -> docker PoB boot + loadBuildFromXML + dump of build.configTab / bandit / pantheon (one docker run per case, O5/W12).
// Strings are BYTE strings: bytes >= 0x7f or < 0x20 (besides \n \r \t) are written as \u00XX (one escape per byte), UTF-8 is NOT decoded.
// Numbers in config answers are {"num":"<%.17g>"}. Attribs are [key,value] pairs sorted by key (Lua order is undefined).
// Usage (PowerShell, Node 24, from pob-rust root): node tools/lua-corpus/export-xml-config.ts [--no-selfcheck]
import { POB as UPSTREAM_POB } from '../upstream.ts'
import { execFile, execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
const run = promisify(execFile)
type Rec = Record<string, any> // eslint-disable-line
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const POB = UPSTREAM_POB
const POBTS = 'C:/pob-work/pob-ts'
const FIX = join(ROOT, 'crates/pob-build/tests/fixtures')
const IMAGE = 'ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest'
const MAX = 1_500_000
const u8 = (s: string): string => Buffer.from(s, 'utf8').toString('latin1') // JS text -> byte string
const BOM = '\xef\xbb\xbf'

// ---------------------------------------------------------------- deterministic JSON of byte strings
const jstr = (s: string): string => '"' + s.replace(/[\x00-\x1f"\\\x7f-\xff]/g, (c) => (c === '"' ? '\\"' : c === '\\' ? '\\\\' : c === '\n' ? '\\n' : c === '\r' ? '\\r' : c === '\t' ? '\\t' : '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'))) + '"'
const ser = (v: unknown): string => typeof v === 'string' ? jstr(v) : v === null || typeof v !== 'object' ? JSON.stringify(v) : Array.isArray(v) ? `[${v.map(ser).join(',')}]`
  : `{${Object.keys(v as Rec).filter((k) => (v as Rec)[k] !== undefined).sort().map((k) => `${jstr(k)}:${ser((v as Rec)[k])}`).join(',')}}`
const normAddr = (v: any): any => typeof v === 'string' ? v.replace(/(function|table|userdata): 0x[0-9a-f]+/g, '$1: <addr>') : Array.isArray(v) ? v.map(normAddr) // eslint-disable-line
  : v && typeof v === 'object' ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, normAddr(x)])) : v

// ---------------------------------------------------------------- Lua chunks
const LUA_ESC = String.raw`
local function esc(s)
  return '"' .. (s:gsub('[%c"\\\127-\255]', function(c)
    if c == '"' then return '\\"' elseif c == '\\' then return '\\\\' elseif c == '\n' then return '\\n' elseif c == '\r' then return '\\r' elseif c == '\t' then return '\\t' end
    return string.format('\\u%04x', c:byte())
  end)) .. '"'
end
`
const XML_BODY = String.raw`
local function node(n)
  if type(n) == "string" then return esc(n) end
  local keys, a, c = {}, {}, {}
  for k in pairs(n.attrib) do keys[#keys + 1] = k end
  table.sort(keys)
  for i, k in ipairs(keys) do a[i] = '[' .. esc(k) .. ',' .. esc(n.attrib[k]) .. ']' end
  for i, ch in ipairs(n) do c[i] = node(ch) end
  return '{"attrib":[' .. table.concat(a, ',') .. '],"children":[' .. table.concat(c, ',') .. '],"elem":' .. esc(n.elem) .. (n.empty and ',"empty":true' or '') .. '}'
end
local out = {}
for i, input in ipairs(CASES) do
  local ok, r, e = pcall(xml.ParseXML, input)
  if not ok then out[i] = '["THROW",' .. esc(tostring(r)) .. ']'
  elseif r == nil then out[i] = '["ERR",' .. esc(e) .. ']'
  else
    local c = {}
    for j, ch in ipairs(r) do c[j] = node(ch) end
    out[i] = '{"children":[' .. table.concat(c, ',') .. ']}'
  end
end
return table.concat(out, "\n")
`
const CFG_BODY = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local function num(v) return '{"num":"' .. RAWF("%.17g", v) .. '"}' end
local function enc(v, d)
  local t = type(v)
  if t == "nil" then return "null" elseif t == "number" then return num(v) elseif t == "string" then return esc(v) elseif t == "boolean" then return tostring(v)
  elseif t == "table" then
    if d > 6 then return '"<deep>"' end
    local keys, parts = {}, {}
    for k in pairs(v) do keys[#keys + 1] = k end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    for i, k in ipairs(keys) do parts[i] = esc(tostring(k)) .. ":" .. enc(v[k], d + 1) end
    return "{" .. table.concat(parts, ",") .. "}"
  end
  return esc(t)
end
local function fmt(list) local t = {} for i, m in ipairs(list) do t[i] = esc(modLib.formatSourceMod(m)) end return "[" .. table.concat(t, ",") .. "]" end
local function dump()
  local b, ct = build, build.configTab
  local ids, sets, order = {}, {}, {}
  for id in pairs(ct.configSets) do ids[#ids + 1] = id end
  table.sort(ids)
  for i, id in ipairs(ids) do local s = ct.configSets[id]
    sets[i] = '[' .. num(id) .. ',{"customModsList":' .. enc(s.customModsList, 0) .. ',"input":' .. enc(s.input, 0) .. ',"placeholder":' .. enc(s.placeholder, 0) .. ',"title":' .. enc(s.title, 0) .. '}]' end
  local oks = {}
  for k in pairs(ct.configSetOrderList) do oks[#oks + 1] = k end
  table.sort(oks)
  for i, k in ipairs(oks) do order[i] = '[' .. num(k) .. ',' .. enc(ct.configSetOrderList[k], 0) .. ']' end
  return '{"activeConfigSetId":' .. enc(ct.activeConfigSetId, 0) .. ',"bandit":' .. enc(b.bandit, 0) .. ',"characterLevel":' .. enc(b.characterLevel, 0)
    .. ',"configSetOrderList":[' .. table.concat(order, ',') .. '],"configSets":[' .. table.concat(sets, ',') .. ']'
    .. ',"enemyLevel":' .. enc(ct.enemyLevel, 0) .. ',"enemyMods":' .. fmt(ct.enemyModList) .. ',"input":' .. enc(ct.input, 0) .. ',"mainSocketGroup":' .. enc(b.mainSocketGroup, 0)
    .. ',"mods":' .. fmt(ct.modList) .. ',"pantheonMajorGod":' .. enc(b.pantheonMajorGod, 0) .. ',"pantheonMinorGod":' .. enc(b.pantheonMinorGod, 0)
    .. ',"placeholder":' .. enc(ct.placeholder, 0) .. '}'
end
local function load1(xmlText)
  __mainObject__.promptMsg = nil
  local ok, err = pcall(loadBuildFromXML, xmlText, "case")
  if not ok then return '{"error":' .. esc(tostring(err)) .. '}' end
  if __mainObject__.promptMsg then return '{"prompt":' .. esc(tostring(__mainObject__.promptMsg)) .. '}' end
  local ok2, d = pcall(dump)
  if not ok2 then return '{"dumpError":' .. esc(tostring(d)) .. '}' end
  return d
end
local out = {}
for i, x in ipairs(CASES) do out[i] = load1(x) end
return table.concat(out, "\n")
`
const luaStr = (buf: string): string => { // byte string -> Lua literal (decimal escapes for all but printable ASCII)
  let s = '"'
  for (const ch of buf) { const b = ch.charCodeAt(0); s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? ch : '\\' + String(b).padStart(3, '0') }
  return s + '"'
}
const casesLua = (cases: string[]): string => `local CASES = {\n${cases.map(luaStr).join(',\n')}\n}\n`
const parseLines = (text: string, n: number): any[] => { const l = text.split('\n'); if (l.length !== n) throw new Error(`${n} cases -> ${l.length} answers`); return l.map((x) => JSON.parse(x)) } // eslint-disable-line

// ---------------------------------------------------------------- engines
function desktop(cases: string[]): { rows: any[]; luajit: string; dll: string } { // eslint-disable-line
  const tool = join(ROOT, 'tools/luajit-win')
  execFileSync('cargo', ['build', '-q', '--release', '--manifest-path', join(tool, 'Cargo.toml')], { stdio: 'inherit' })
  const tmp = mkdtempSync(join(tmpdir(), 'pob-xml-'))
  try {
    writeFileSync(join(tmp, 'x.lua'), `${LUA_ESC}local xml = dofile(${luaStr(POB + '/runtime/lua/xml.lua')})\n${casesLua(cases)}${XML_BODY.replace('return table.concat(out, "\\n")', '')}return jit.version .. "\\n" .. table.concat(out, "\\n")\n`)
    const out = execFileSync(join(tool, 'target/release/luajit-win.exe'), [join(tmp, 'x.lua')], { encoding: 'utf8', maxBuffer: 256 << 20, env: { ...process.env, PATH: `${POB}/runtime;${process.env.PATH ?? ''}` } })
    const nl = out.indexOf('\n')
    return { rows: parseLines(out.slice(nl + 1), cases.length), luajit: out.slice(0, nl), dll: createHash('sha256').update(readFileSync(`${POB}/runtime/lua51.dll`)).digest('hex') }
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
async function docker(code: string): Promise<string> { // luajit-eval.lua: PoB HeadlessWrapper boot, then the chunk; last stdout line = {"result": <string>}
  const tmp = mkdtempSync(join(tmpdir(), 'pob-r38a-'))
  try {
    writeFileSync(join(tmp, 'code.lua'), code)
    const args = ['run', '--rm', '-v', `${POB}:/workdir:ro`, '-v', `${POBTS}/tools/oracle/lua:/bench:ro`, '-v', `${tmp}:/code:ro`, '-w', '/workdir/src',
      '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = (await run('docker', args, { encoding: 'utf8', maxBuffer: 512 << 20 })).stdout } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
async function pool<T, R>(items: T[], n: number, f: (x: T, i: number) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(items.length)
  let next = 0
  await Promise.all(Array.from({ length: n }, async () => { for (;;) { const i = next++; if (i >= items.length) return; res[i] = await f(items[i]!, i) } }))
  return res
}

// ---------------------------------------------------------------- corpus 1: xml.lua
function xmlCorpus(): string[] {
  const c: string[] = []
  const add = (...s: string[]): void => { c.push(...s.map(u8)) }
  // whitespace and text runs
  add('<a>x</a>', '<a> x </a>', '<a>  x  y  </a>', '<a>\n\tx\n</a>', '<a>   </a>', '<a>\n</a>', '<a> </a>', '<a><b/> x <c/></a>', '<a>x<b/>y</a>', '<a> <b/> </a>', '<a>x <b>y </b> z</a>',
    '<a>\r\nx\r\n</a>', '<a>\v\fx\f\v</a>', '<a>\u00a0x\u00a0</a>', 'x<a/>', '<a/>x', '  <a/>  ', '<a/><b/>', '', '   ', 'hello', '\n\n<a/>\n\n', '<a>\n\t<b>\n\t\tt\n\t</b>\n</a>', '<a>x\ny\n z </a>', '<a>1</a><!-- -->  <b>2</b>')
  // attributes
  for (const k of ['name', 'a-b', 'a_b', 'a:b', 'a1', '1a', 'a.b', 'a b', 'Ab', 'xml:lang', '_x', '-x', 'é']) add(`<a ${k}="v"/>`)
  add('<a k = "v"/>', '<a k= "v"/>', '<a k ="v"/>', '<a k=\'v\'/>', '<a k="it\'s"/>', '<a k=\'say "x"\'/>', '<a k=""/>', '<a k=\'\'/>', '<a k="1" k="2"/>', '<a k="1" K="2"/>', '<a k/>', '<a k=v/>', '<a k="1"j="2"/>',
    '<a k="1"   j="2"\n\tl="3"/>', '<a k="a>b"/>', '<a k="a/"/>', '<a k="/"/>', '<a k="a\nb"/>', '<a k="&amp;&lt;&#10;&foo;"/>', "<a k='&quot;'/>", '<a k="1" />', '<a k="1"/ >', '<a/ >', '<a k="v"></a>', '<a k="1" j=\'2\' l="3">t</a>',
    '<a k="x=\'y\'"/>', '<a k=\'x="y"\'/>', '<a k="a" j=b l="c"/>', '<a=1/>', '<a k=1 j="2"/>', '<a k="v>"></a>', '<a k="v" ?>', '<a k="v"?>', '<a k="&amp"/>', '<a\tk="v"/>', '<a\nk="v"\n/>', '<a k="日本"/>', '<a k="1"/><a k="2"/>')
  // entities
  add('<a>&lt;&gt;&amp;&apos;&quot;</a>', '<a>&#10;</a>', '<a>&#x41;</a>', '<a>&foo;</a>', '<a>&</a>', '<a>&amp</a>', '<a>a & b</a>', '<a>&;</a>', '<a>&amp;amp;</a>', '<a>&lt;b&gt;</a>', '<a>& a; b</a>', '<a>&\n;</a>',
    '<a> &amp; </a>', '<a>&lt;</a>', '<a>x&#65;y</a>', '<a>&AMP;</a>', '<a>&a;&b;&amp;</a>', '<a>&&amp;;</a>', '<a><![CDATA[&amp;]]></a>', '<a>1 &lt; 2 &amp;&amp; 3 &gt; 2</a>', '<a>&quot;q&quot;</a>', '<a>a&amp;b;c</a>')
  // comments
  add('<!-- c --><a/>', '<a>x<!-- c -->y</a>', '<a>x <!-- c --> y</a>', '<a><!-- c --></a>', '<a><!-- <b> --></a>', '<!-- x', '<a><!-- x</a>', '<a><!----></a>', '<a><!-- -- --></a>', '<a k="<!-- c -->"/>', '<a><!-- <![CDATA[x]]> --></a>',
    '<a><!-- <!-- --> --></a>', '<!-- c --><?xml version="1.0"?><a/>', '<!-- only -->', '<a <!-- c --> k="v"/>', '<a>\n<!-- c -->\n</a>', '<a><!-- c -->t<!-- d --></a>', '<a>x<!-- c -->\n<!-- d -->y</a>', '<a><!-- c --><b/><!-- d --></a>', '<!-- a --><!-- b -->')
  // CDATA
  add('<a><![CDATA[x]]></a>', '<a><![CDATA[ x ]]></a>', '<a><![CDATA[   ]]></a>', '<a><![CDATA[]]></a>', '<a><![CDATA[<b>]]></a>', '<a>t1<![CDATA[x]]>t2</a>', '<a><![CDATA[x]]><![CDATA[y]]></a>', '<a><![CDATA[a]]b]]></a>',
    '<a><![CDATA[x</a>', '<![CDATA[x]]>', '<a>  <![CDATA[x]]>  </a>', '<a><![CDATA[x]]> </a>', '<a><b/><![CDATA[x]]></a>', '<a><![CDATA[x]]><b/></a>', '<a><![CDATA[\nline1\nline2\n]]></a>', '<a>t &amp; <![CDATA[&amp;]]> &lt;</a>',
    '<a><![CDATA[x]]>t<![CDATA[y]]></a>', '<a><![CDATA[x]]]]></a>', '<a><![CDATA[]]]></a>', '<a>t<![CDATA[]]></a>', '<a><![cdata[x]]></a>')
  // processing instructions, declaration, DOCTYPE
  add('<?xml version="1.0"?><a/>', '<?xml version="1.0" encoding="UTF-8"?>\n<a/>', '<?xml?><?xml?><a/>', '<?php echo ?><a/>', '<?a b>c?><d/>', '<a><?pi x?></a>', '<a?>', '<?a/>', '<!DOCTYPE a><a/>', '<?xml version="1.0"?>',
    '<a>t<?pi?>u</a>', '<a><?pi k="v"?><b/></a>', '<!DOCTYPE a [<!ELEMENT a ANY>]><a/>', '<?xml version="1.0"?>\n<!-- c -->\n<a>\n</a>', '<a ?>', '<a k="v" ?/>')
  // self-closed / empty
  add('<a/>', '<a />', '<a></a>', '<a>\n</a>', '<a><b/></a>', '<a><b></b></a>', '<a/ x>', '<a><b/><b/></a>', '<a>\t</a>', '<a><b>\n</b></a>', '<a x="1"><b y="2"/></a>')
  // broken
  add('<a>', '<a><b></a>', '</a>', '<a></b>', '<a>x', '<a', '<a x="1', '<>', '< a>', '<1a/>', '<a b>', '<a>><</a>', '<a>x < y</a>', '<a>1 < 2</a>', '<a/></a>', '<a><b></b></a></a>', '<a><b></a></b>', '</>', '<a>x</a>y</b>',
    '<a:b/>', '<a-b/>', '<a_b/>', '<a.b/>', '<1/>', '<a>text</a', '<a>  </a  >', '<a></a x>', '<a><b/></a >', '<a></a><b></a>', '<a><b>', '</a><a>', '<a>>', '<a></a', '<<a/>', '<a/>>', '<a><</a>',
    '<a>' + '<b>'.repeat(60) + '</b>'.repeat(60) + '</a>', '<a>' + '<b/>'.repeat(200) + '</a>', '<a>' + 'long text '.repeat(500) + '</a>', '<a:b:c/>', '<a:/>', '<:a/>', '<a1/>', '<A/><a/>')
  // BOM, line ends, encodings
  c.push(BOM + '<a/>', BOM + '<?xml version="1.0"?><a/>', '\xfe\xff<a/>', '<a>\r\n\t<b/>\r\n</a>\r\n', '<a>\r<b/>\r</a>', '<a>\r\nx\ry\nz</a>', '<a>x\0y</a>', '<a>\x01x\x02</a>', '<a k="x\0y"/>', '<a>caf\xe9</a>', '<a>\xff\xfe</a>',
    '<a>\x85x\x85</a>', '<a>' + BOM + 'x</a>', BOM + BOM + '<a/>', ' ' + BOM + '<a/>')
  add('<a>é</a>', '<a>日本語</a>', '<a>😀</a>', '<é/>', '<a k="é">ü</a>', '<a>\u2028x\u2028</a>', '<a>ñ</a>', '<a>\u3000x\u3000</a>', '<日本/>')
  // PoB-like documents
  add('<?xml version="1.0" encoding="UTF-8"?>\n<PathOfBuilding>\n\t<Build level="1" bandit="None"/>\n\t<Config>\n\t\t<Input name="x" number="2"/>\n\t</Config>\n</PathOfBuilding>\n',
    '<PathOfBuilding>\n\t<Notes>\n\tline one\n\n\tline two  \n\t</Notes>\n</PathOfBuilding>',
    '<PathOfBuilding>\n\t<Items>\n\t\t<Item id="1">\n\t\tRarity: RARE\n\t\tName\n\t\tBase &lt;x&gt;\n\t\t</Item>\n\t</Items>\n</PathOfBuilding>',
    '<PathOfBuilding><Config><ConfigSet id="1" title="A"><CustomModifierBlock title="t">+1 to Str<![CDATA[ +2 to Dex]]> tail</CustomModifierBlock></ConfigSet></Config></PathOfBuilding>',
    '<PathOfBuilding>\n\t<Config>\n\t\t<Input name = "x" number = "2"/>\n\t\t<Input name=\'y\' string=\'z\'/>\n\t</Config>\n</PathOfBuilding>',
    '<Config><CustomModifierBlock>a<b/>c</CustomModifierBlock></Config>', '<Config><CustomModifierBlock><b/>c</CustomModifierBlock></Config>', '<Config><CustomModifierBlock/></Config>',
    '<Config><CustomModifierBlock>  </CustomModifierBlock></Config>', '<Config><CustomModifierBlock>x &amp; y &#10; z &lt;</CustomModifierBlock></Config>')
  return c
}

// ---------------------------------------------------------------- corpus 2: full builds
const BASE = readFileSync(`${POB}/spec/TestBuilds/3.13/Dual Savior.xml`, 'latin1').replace(/\r\n/g, '\n')
  .replace(/\t\t\t<EditedNodes>[^]*?<\/EditedNodes>\n/, '')
  .replace(/\t<Items[ >][^]*?<\/Items>\n/, '').replace(/\t<Calcs>[^]*?<\/Calcs>\n/, '') // ASCII; CRLF -> LF (case crlf-whole-doc restores it); <EditedNodes>, <Items>, <Calcs> dropped to keep the fixture <= 1.5 MB (config-only truth)
const BASE_ATTRS: Array<[string, string]> = [['level', '1'], ['targetVersion', '3_0'], ['pantheonMajorGod', 'None'], ['bandit', 'None'], ['className', 'Marauder'], ['ascendClassName', 'Berserker'],
  ['mainSocketGroup', '3'], ['viewMode', 'TREE'], ['pantheonMinorGod', 'None']]
const inp = (name: string, kind: string, v: string, tag = 'Input'): string => `\t\t<${tag} name="${name}" ${kind}="${v}"/>\n`
const cfg = (...parts: string[]): string => `\t<Config>\n${parts.join('')}\t</Config>\n`
const cs = (id: string | null, title: string | null, body = ''): string => `\t\t<ConfigSet${id === null ? '' : ` id="${id}"`}${title === null ? '' : ` title="${title}"`}>\n${body}\t\t</ConfigSet>\n`
const cmb = (text: string, attrs = ' title="Mods"'): string => `\t\t<CustomModifierBlock${attrs}>${text}</CustomModifierBlock>\n`
function mkBuild(attrs: Record<string, string | null>, config: string | null | undefined, post?: (s: string) => string): string {
  const keys = [...BASE_ATTRS.map(([k]) => k), ...Object.keys(attrs).filter((k) => !BASE_ATTRS.some(([b]) => b === k))]
  const base = Object.fromEntries(BASE_ATTRS)
  const tag = '<Build ' + keys.map((k) => [k, k in attrs ? attrs[k] : base[k]] as const).filter(([, v]) => v !== null).map(([k, v]) => `${k}="${v}"`).join(' ') + '>'
  let s = BASE.replace(/<Build [^>]*>/, tag)
  if (config !== undefined) s = s.replace(/\t<Config>[\s\S]*?<\/Config>\n/, config === null ? '' : config)
  return u8(post ? post(s) : s)
}
function configCorpus(): Array<[string, string]> {
  const k: Array<[string, string]> = []
  const add = (n: string, attrs: Record<string, string | null>, config?: string | null, post?: (s: string) => string): void => { k.push([n, mkBuild(attrs, config, post)]) }
  add('base-original', {})
  add('no-config-section', {}, null)
  add('config-selfclosed', {}, '\t<Config/>\n')
  add('config-pair-empty', {}, '\t<Config></Config>\n')
  add('config-whitespace-only', {}, '\t<Config>\n\n\t</Config>\n')
  for (const [n, v] of [['abc', 'abc'], ['empty', ''], ['hex', '0x1F'], ['exp', '1e2'], ['spaces', ' 7 '], ['inf', 'inf'], ['huge', '1e999'], ['negzero', '-0']] as const)
    add(`number-${n}`, {}, cfg(inp('enemyLevel', 'number', v), inp('customVarN', 'number', v)))
  add('number-abc-on-default-var', {}, cfg(inp('bloodsoakedBannerStages', 'number', 'abc'), inp('touchedDebuffsCount', 'number', '')))
  add('number-valid-on-default-var', {}, cfg(inp('bloodsoakedBannerStages', 'number', '3'), inp('touchedDebuffsCount', 'number', '0')))
  add('number-string-boolean-priority', {}, cfg('\t\t<Input name="x" number="5" string="s" boolean="true"/>\n', '\t\t<Input name="y" string="s" boolean="true"/>\n'))
  add('boolean-variants', {}, cfg(inp('b1', 'boolean', 'TRUE'), inp('b2', 'boolean', 'yes'), inp('b3', 'boolean', '1'), inp('b4', 'boolean', 'false'), inp('b5', 'boolean', 'True'), inp('b6', 'boolean', ''), inp('b7', 'boolean', 'true')))
  add('input-missing-name', {}, cfg('\t\t<Input number="5"/>\n'))
  add('input-no-value', {}, cfg('\t\t<Input name="x"/>\n'))
  add('placeholder-missing-name', {}, cfg('\t\t<Placeholder number="5"/>\n'))
  add('placeholder-boolean-only', {}, cfg('\t\t<Placeholder name="x" boolean="true"/>\n'))
  add('placeholder-number', {}, cfg(inp('enemyLevel', 'number', '80', 'Placeholder'), inp('customP', 'number', '2.5', 'Placeholder')))
  add('placeholder-number-bad', {}, cfg(inp('enemyLevel', 'number', 'abc', 'Placeholder'), inp('customP', 'number', '', 'Placeholder')))
  add('placeholder-string-goes-to-input', {}, cfg(inp('customS', 'string', 'hello', 'Placeholder'), inp('enemyIsBoss', 'string', 'Boss', 'Placeholder')))
  add('placeholder-number-and-string', {}, cfg('\t\t<Placeholder name="x" number="3" string="s"/>\n'))
  add('placeholder-after-input', {}, cfg(inp('enemyLevel', 'number', '70'), inp('enemyLevel', 'number', '85', 'Placeholder')))
  for (const v of ['boss', 'Uber Atziri', 'shaper', 'Sirus', 'PINNACLE', 'Guardian', 'uber pinnacle', 'ShAPer sirus'])
    add(`boss-${v.replace(/\W+/g, '_')}`, {}, cfg(inp('enemyIsBoss', 'string', v)))
  for (const v of ['Uber Atziri Flameblast', 'uber lol', 'None', 'Uber Uber X'])
    add(`preset-${v.replace(/\W+/g, '_')}`, {}, cfg(inp('enemyIsBoss', 'string', 'Pinnacle'), inp('presetBossSkills', 'string', v)))
  add('sets-two-active2', {}, cfg(cs('1', 'One', inp('enemyLevel', 'number', '60')), cs('2', 'Two', inp('enemyLevel', 'number', '90'))).replace('<Config>', '<Config activeConfigSet="2">'))
  add('sets-active-missing', {}, cfg(cs('1', 'One')).replace('<Config>', '<Config activeConfigSet="7">'))
  add('sets-active-nonnumeric', {}, cfg(cs('1', 'One'), cs('2', 'Two')).replace('<Config>', '<Config activeConfigSet="abc">'))
  add('sets-active-zero', {}, cfg(cs('1', 'One')).replace('<Config>', '<Config activeConfigSet="0">'))
  add('sets-duplicate-id', {}, cfg(cs('1', 'First', inp('enemyLevel', 'number', '60') + inp('only1', 'string', 'a')), cs('1', 'Second', inp('only2', 'string', 'b'))))
  add('sets-empty-set', {}, cfg(cs('1', 'Empty')))
  add('sets-empty-selfclosed', {}, cfg('\t\t<ConfigSet id="1" title="Empty"/>\n'))
  add('sets-no-id', {}, cfg(cs(null, 'NoId', inp('only', 'string', 'a')), cs(null, null, inp('only', 'string', 'b'))))
  add('sets-ids-5-2', {}, cfg(cs('5', 'Five', inp('enemyLevel', 'number', '50')), cs('2', 'Two', inp('enemyLevel', 'number', '20'))).replace('<Config>', '<Config activeConfigSet="2">'))
  add('sets-id-float-neg', {}, cfg(cs('1.5', 'F'), cs('-3', 'N'), cs('abc', 'A')))
  add('sets-legacy-then-set', {}, cfg(inp('loose', 'string', 'legacy'), cs('1', 'S', inp('inset', 'string', 'x'))))
  add('sets-set-then-legacy', {}, cfg(cs('2', 'S', inp('inset', 'string', 'x')), inp('loose', 'string', 'legacy')))
  add('sets-no-title', {}, cfg(cs('1', null, inp('a', 'string', 'b'))))
  add('sets-title-entities', {}, cfg(cs('1', 'A &amp; B &lt;x&gt; &#10; &foo;')))
  add('legacy-input-only', {}, cfg(inp('enemyLevel', 'number', '75')))
  add('cm-single-block', {}, cfg(cmb('+100 to Strength')))
  add('cm-multi-text-cdata', {}, cfg(cmb('+100 to Strength<![CDATA[ +50 to Dexterity]]> tail\n+10 to Intelligence')))
  add('cm-entities', {}, cfg(cmb('+10 to Strength &amp; &lt;x&gt; &#10; &foo; +5 to Dexterity')))
  add('cm-element-first-child', {}, cfg(cmb('<b/>+10 to Strength')))
  add('cm-text-then-element', {}, cfg(cmb('+10 to Strength<b/>+20 to Dexterity')))
  add('cm-two-blocks', {}, cfg(cmb('+10 to Strength', ' title="A" enabled="true"'), cmb('+20 to Dexterity', ' title="B" enabled="false"'), cmb('+30 to Intelligence', ' enabled="TRUE"')))
  add('cm-legacy-customMods-input', {}, cfg(inp('customMods', 'string', '+10 to Strength')))
  add('cm-legacy-plus-blocks', {}, cfg(inp('customMods', 'string', '+10 to Strength'), cmb('+20 to Dexterity')))
  add('cm-legacy-plus-empty-block', {}, cfg(inp('customMods', 'string', '+10 to Strength'), cmb('')))
  add('cm-in-set', {}, cfg(cs('1', 'S', cmb('+10 to Strength') + cmb('+5 to Dexterity', ' title="Second"')), cs('2', 'T', cmb('+7 to Intelligence'))))
  add('cm-empty-and-whitespace', {}, cfg(cmb(''), cmb('   \n  '), '\t\t<CustomModifierBlock title="sc"/>\n'))
  add('cm-nonascii', {}, cfg(cmb('+10 to Strength \u00f1 \u65e5')))
  add('cm-multiline', {}, cfg(cmb('\n\t\t\t+10 to Strength\n\t\t\t+20 to Dexterity   \n\t\t')))
  add('cm-comment-inside', {}, cfg(cmb('+10 to Str<!-- c -->ength\n+5 to Dexterity')))
  add('bandit-oak-no-config', { bandit: 'Oak' }, null)
  add('bandit-oak-config-empty', { bandit: 'Oak' }, '\t<Config/>\n')
  add('bandit-oak-config-other-input', { bandit: 'Oak' })
  add('bandit-oak-config-override', { bandit: 'Oak' }, cfg(inp('bandit', 'string', 'Alira')))
  add('bandit-absent', { bandit: null })
  add('bandit-bogus', { bandit: 'Zzz' }, null)
  add('pantheon-lunaris-no-config', { pantheonMajorGod: 'Lunaris' }, null)
  add('pantheon-both-with-config', { pantheonMajorGod: 'Lunaris', pantheonMinorGod: 'Gruthkul' })
  add('pantheon-config-input-override', { pantheonMajorGod: 'Lunaris' }, cfg(inp('pantheonMajorGod', 'string', 'Solaris'), inp('pantheonMinorGod', 'string', 'Shakari')))
  add('pantheon-bogus-god', { pantheonMajorGod: 'Soul of Lunaris' }, null)
  add('pantheon-minor-only-no-config', { pantheonMinorGod: 'Shakari' }, null)
  add('pantheon-absent-attrs', { pantheonMajorGod: null, pantheonMinorGod: null, bandit: null }, null)
  add('level-abc', { level: 'abc' })
  add('level-zero', { level: '0' })
  add('level-101', { level: '101' })
  add('level-float', { level: '50.5' })
  add('level-100-enemylevel', { level: '100' }, cfg(inp('enemyLevel', 'number', '83')))
  add('autolevel-true', { characterLevelAutoMode: 'true' })
  add('main-socket-bogus', { mainSocketGroup: 'abc' })
  add('dup-input-last-wins', {}, cfg(inp('enemyLevel', 'number', '10'), inp('enemyLevel', 'number', '20'), inp('s', 'string', 'a'), inp('s', 'number', '1')))
  add('input-weird-names', {}, cfg(inp('a b', 'string', 'x'), inp('a-b', 'string', 'y'), inp('\u00e9', 'string', 'z')))
  add('single-quote-attrs', {}, cfg("\t\t<Input name='enemyLevel' number='88'/>\n"))
  add('spaced-equals', {}, cfg('\t\t<Input name = "enemyLevel" number = "88"/>\n'))
  add('attr-dash-colon-names', {}, cfg('\t\t<Input name="x" number="5" data-x="1" a:b="2" a_b="3"/>\n'))
  add('config-comment-cdata', {}, cfg('\t\t<!-- c -->\n', inp('enemyLevel', 'number', '77'), '\t\t<![CDATA[junk]]>\n', '\t\tstray text\n'))
  add('config-unknown-elems', {}, cfg('\t\t<Foo bar="1"/>\n', inp('enemyLevel', 'number', '66'), '\t\t<Foo><Input name="n" number="1"/></Foo>\n'))
  add('two-config-sections', {}, cfg(inp('enemyLevel', 'number', '10')) + cfg(inp('enemyLevel', 'number', '20')))
  add('config-before-build', {}, cfg(inp('enemyLevel', 'number', '33')), (s) => { const m = /\t<Config>[\s\S]*?<\/Config>\n/.exec(s)![0]; return s.replace(m, '').replace('\t<Build ', m + '\t<Build ') })
  add('crlf-whole-doc', {}, undefined, (s) => s.replace(/\n/g, '\r\n'))
  add('bom-doc', {}, undefined, (s) => String.fromCharCode(0xfeff) + s)
  add('xml-decl-missing', {}, undefined, (s) => s.replace(/^<\?xml[^>]*>\n/, ''))
  add('config-boss-uber-combo', {}, cfg(inp('enemyIsBoss', 'string', 'Uber Atziri'), inp('enemyLevel', 'number', '84'), inp('presetBossSkills', 'string', 'Uber Atziri Flameblast')))
  return k
}

// ---------------------------------------------------------------- main
const write = (name: string, head: Rec, cases: Rec[]): string => {
  const text = `{"format":1,"producer":${ser(head.producer)},${head.reference ? `"reference":${ser(head.reference)},` : ''}"cases":[\n${cases.map(ser).join(',\n')}\n]}\n`
  if (Buffer.byteLength(text) > MAX) throw new Error(`${name}: ${Buffer.byteLength(text)} bytes > ${MAX}`)
  writeFileSync(join(FIX, name), text)
  return createHash('sha256').update(text).digest('hex')
}
const git = (d: string): string => execFileSync('git', ['-C', d, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const T0 = Date.now()
const xmlIn = xmlCorpus()
const dk = desktop(xmlIn)
const xmlLuaSha = createHash('sha256').update(readFileSync(`${POB}/runtime/lua/xml.lua`)).digest('hex')
const dockerRows = parseLines(await docker(`${LUA_ESC}local xml = dofile("../runtime/lua/xml.lua")\n${casesLua(xmlIn)}${XML_BODY}`), xmlIn.length)
const xmlDiff = xmlIn.map((_, i) => i).filter((i) => ser(dk.rows[i]) !== ser(normAddr(dockerRows[i])))
console.log(`xml cases: ${xmlIn.length}; desktop ${dk.luajit}; desktop<->docker differing: ${xmlDiff.length}${xmlDiff.length ? ' -> ' + xmlDiff.slice(0, 20).map((i) => JSON.stringify(xmlIn[i])).join(' | ') : ''}`)
const imageId = execFileSync('docker', ['image', 'inspect', '--format', '{{.Id}}', IMAGE], { encoding: 'utf8' }).trim()
const pobSha = git(POB)
const xmlCases = xmlIn.map((input, i) => ({ input, result: normAddr(dk.rows[i]) }))
const prodXml = { generator: 'tools/lua-corpus/export-xml-config.ts', strings: 'byte strings: bytes <0x20 (not \\n\\r\\t) and >=0x7f as \\u00XX; attribs = [key,value] sorted by key; ERR/THROW = nil,err / error()',
  engine: 'desktop LuaJIT (tools/luajit-win, runtime/lua51.dll) dofile(runtime/lua/xml.lua); docker cross-check: identical', luajit: dk.luajit, lua51Sha256: dk.dll, xmlLuaSha256: xmlLuaSha, pobSha, image: imageId }
const cfgIn = configCorpus()
const CACHE = process.env.R38A_CACHE ? join(tmpdir(), 'pob-r38a-cache') : undefined // dev speed-up only; the final proof runs without it
if (CACHE) mkdirSync(CACHE, { recursive: true })
const cfgOut = await pool(cfgIn, 3, async ([, xml]) => {
  const cf = CACHE && join(CACHE, createHash('sha256').update(xml + CFG_BODY + LUA_ESC).digest('hex'))
  if (cf && existsSync(cf)) return JSON.parse(readFileSync(cf, 'utf8'))
  const r = normAddr(JSON.parse(await docker(`${LUA_ESC}${casesLua([xml])}${CFG_BODY}`)))
  if (cf) writeFileSync(cf, JSON.stringify(r))
  return r
})
// O5/W12: does re-loading in ONE instance equal a fresh instance? (informational; fixture is per-case fresh)
const okIdx = cfgIn.map((_, i) => i).filter((i) => cfgOut[i]!.activeConfigSetId !== undefined)
const chain = [0, 5, 20, 40, 60, 10, 45].filter((i) => okIdx.includes(i)).slice(0, 7)
const chained = (await docker(`${LUA_ESC}${casesLua(chain.map((i) => cfgIn[i]![1]))}${CFG_BODY}`)).split('\n').map((l) => normAddr(JSON.parse(l)))
const chainDiff = chain.filter((ci, j) => ser(chained[j]) !== ser(cfgOut[ci]))
console.log(`config cases: ${cfgIn.length}; same-instance reload chain [${chain}] differs from fresh at: [${chainDiff}]`)
const refIdx = cfgIn.findIndex(([n]) => n === 'config-selfclosed')
const REF = { input: cfgOut[refIdx].input, placeholder: cfgOut[refIdx].placeholder }
// size: input/placeholder are stored as a diff against REF (key -> new value, null = key absent); lossless, applies to the top level and to every configSets entry
const diff = (ref: Rec, cur: Rec): Rec => { const o: Rec = {}; for (const k of Object.keys(cur)) if (ser(cur[k]) !== ser(ref[k])) o[k] = cur[k]; for (const k of Object.keys(ref)) if (!(k in cur)) o[k] = null; return o }
const packSet = (s: Rec): Rec => { const { input, placeholder, ...rest } = s; return { ...rest, inputDiff: diff(REF.input, input), placeholderDiff: diff(REF.placeholder, placeholder) } }
const pack = (r: Rec): Rec => { if (!r.input) return r; const { input, placeholder, configSets, ...rest } = r; return { ...rest, configSets: configSets.map(([id, s]: [Rec, Rec]) => [id, packSet(s)]), inputDiff: diff(REF.input, input), placeholderDiff: diff(REF.placeholder, placeholder) } }
const cfgCases = cfgIn.map(([name, input], i) => ({ name, input, result: pack(cfgOut[i]) }))
const kinds: Rec = {}
for (const c of cfgCases) { const r = c.result; const k = r.error ? 'error' : r.prompt ? 'prompt' : r.dumpError ? 'dumpError' : 'ok'; kinds[k] = (kinds[k] ?? 0) + 1 }
console.log('config answers:', JSON.stringify(kinds))
const prodCfg = { generator: 'tools/lua-corpus/export-xml-config.ts', strings: prodXml.strings + '; numbers {"num":"%.17g"}', engine: 'docker PoB HeadlessWrapper boot + loadBuildFromXML (one instance per case), base = spec/TestBuilds/3.13/Dual Savior.xml',
  luajit: dk.luajit, image: imageId, pobSha, reloadChainDiffers: chainDiff.length }
const sha1 = [write('xml-lua.json', { producer: prodXml }, xmlCases), write('config-load.json', { producer: prodCfg, reference: REF }, cfgCases)]
console.log(`written; sha256 xml ${sha1[0]!.slice(0, 16)} config ${sha1[1]!.slice(0, 16)}; ${((Date.now() - T0) / 1000).toFixed(0)}s`)

// ---------------------------------------------------------------- self-check vs pob-ts (informational)
if (!process.argv.includes('--no-selfcheck')) {
  const { parseXml } = await import(`file:///${POBTS}/src/build/xml.ts`)
  const { loadBuildXml } = await import(`file:///${POBTS}/src/build/index.ts`)
  const { buildConfigModLists, configStateFromBuild } = await import(`file:///${POBTS}/src/config/index.ts`)
  const { formatSourceMod } = await import(`file:///${POBTS}/src/mods/format.ts`)
  const { formatG } = await import(`file:///${POBTS}/src/lua/index.ts`)
  const dec = (s: string): string => Buffer.from(s, 'latin1').toString('utf8')
  const enc = (e: Rec | string, trim: boolean): any => { // eslint-disable-line
    if (typeof e === 'string') return u8(e)
    const kids: any[] = [] // eslint-disable-line
    for (const ch of e.children) { const x = enc(ch, trim); if (typeof x === 'string' && trim) { const t = x.replace(/^\s+|\s+$/g, ''); if (/\S/.test(t)) kids.push(t) } else kids.push(x) }
    return { attrib: Object.entries(e.attrib as Rec).map(([k, v]) => [u8(k), u8(v as string)]).sort((a, b) => (a[0] < b[0] ? -1 : 1)), children: kids, elem: e.name, ...(kids.length === 0 ? { empty: true } : {}) }
  }
  const cls: Rec = {}
  const bump = (k: string, who: string): void => { (cls[k] ??= []).push(who) }
  xmlIn.forEach((input, i) => {
    const lua = dk.rows[i]
    let ts: Rec | undefined
    try { ts = parseXml(dec(input)) } catch { ts = undefined }
    const luaErr = Array.isArray(lua)
    if (luaErr && !ts) return bump('same: both error', '')
    if (luaErr) return bump(`lua ${lua[0]} / ts parses`, JSON.stringify(input.slice(0, 40)))
    if (!ts) return bump('lua parses / ts throws', JSON.stringify(input.slice(0, 40)))
    const l = ser(lua.children)
    if (lua.children.length === 1 && ser([enc(ts, false)]) === l) return bump('same: tree identical', '')
    if (lua.children.length === 1 && ser([enc(ts, true)]) === l) return bump('R6.1 whitespace/text trim only', '')
    bump('tree differs (other: attrib/top-level/CDATA/entities)', JSON.stringify(input.slice(0, 40)))
  })
  console.log('--- self-check xml (pob-ts parseXml vs LuaJIT):')
  for (const [k, v] of Object.entries(cls)) console.log(`${String(v.length).padStart(4)}  ${k}${k.startsWith('same') ? '' : '  e.g. ' + v.slice(0, 3).join(' ; ')}`)
  const N = (v: unknown): unknown => (typeof v === 'number' ? { num: formatG(v, 17) } : v)
  const fields: Rec = {}
  const tsDump = (xml: string): Rec => {
    const model = loadBuildXml(dec(xml))
    const state = configStateFromBuild(model)
    const r = buildConfigModLists(state, { characterLevel: model.level })
    return { bandit: model.bandit, pantheonMajorGod: model.pantheonMajorGod, pantheonMinorGod: model.pantheonMinorGod, enemyLevel: N(state.enemyLevel), input: Object.fromEntries(Object.entries(state.input).map(([k, v]) => [k, N(v)])),
      placeholder: Object.fromEntries(Object.entries(state.placeholder).map(([k, v]) => [k, N(v)])), mods: r.modList.mods.map(formatSourceMod), enemyMods: r.enemyModList.mods.map(formatSourceMod),
      activeConfigSetId: N(model.config.activeConfigSet), configSetIds: model.config.sets.map((s: Rec) => N(s.id)) }
  }
  let luaFail = 0; let tsFail = 0; let both = 0
  for (const [ci, c] of cfgCases.entries()) {
    const r = cfgOut[ci]
    let t: Rec | undefined
    try { t = tsDump(c.input) } catch (e) { t = undefined; if (!r.error && !r.prompt && !r.dumpError) tsFail++ }
    if (r.error || r.prompt || r.dumpError) { if (t) luaFail++; else both++; continue }
    if (!t) continue
    const ids = r.configSets.map((s: Rec[]) => s[0])
    const luaView: Rec = { ...r, configSetIds: ids }
    for (const f of ['bandit', 'pantheonMajorGod', 'pantheonMinorGod', 'enemyLevel', 'input', 'placeholder', 'mods', 'enemyMods', 'activeConfigSetId', 'configSetIds'])
      if (ser(luaView[f]) !== ser(t[f])) (fields[f] ??= []).push(c.name)
  }
  console.log(`--- self-check config (pob-ts vs LuaJIT), ${cfgCases.length} cases: lua errors but ts loads: ${luaFail}; both fail: ${both}; ts throws on lua-ok: ${tsFail}`)
  for (const [f, v] of Object.entries(fields)) console.log(`${String(v.length).padStart(4)}  field ${f}: ${v.slice(0, 6).join(', ')}${v.length > 6 ? ', ...' : ''}`)
}
