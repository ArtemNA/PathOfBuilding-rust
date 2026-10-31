// R3.5b: LuaJIT-truth corpus for the 539 `apply` closures of Modules/ConfigOptions.lua.
// For every var with an `apply` (config-options.json order) and representative values by type -- check: true; count: 0, 1, 7;
// integer/countAllowZero: 0, 1, 7, -3; float: 0, 1, 7, 2.5 (what each EditControl filter lets through, ConfigTab.lua:305);
// list: every value of the list -- the probe calls `varData.apply(val, modList, enemyModList, build)` on two fresh ModLists in
// the config tab of a fresh `newBuild()` (input/placeholder/enemyLevel/controls restored to the same snapshot before every
// call) and records: `modLib.formatSourceMod` of both lists (in call order; sorted when the body iterates with `pairs` over
// string keys -- R13), the Lua error text, and the side effects on the tab (input/placeholder keys changed, enemyLevel,
// dropdown selIndex/enabled). Extra contexts: the map-affix dropdowns under every tier x {no effect, 50% effect}, and
// presetBossSkills under enemyIsBoss = Pinnacle/Uber.
// Truth = docker LuaJIT (PoB HeadlessWrapper; no CRT math in these bodies). Two runs must be byte-identical.
// Usage (PowerShell, Node 24, from pob-rust root): node tools/lua-corpus/export-config-apply.ts [--no-selfcheck]
//   -> crates/pob-build/tests/fixtures/config-apply.json
import { execFile } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
const run = promisify(execFile)
type Rec = Record<string, any> // eslint-disable-line
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const POB = process.env.POB_ROOT ?? 'C:/pob-work/project_15/PathOfBuilding'
const POBTS = 'C:/pob-work/pob-ts'
const OUT = join(ROOT, 'crates/pob-build/tests/fixtures/config-apply.json')
const IMAGE = 'ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest'
const MAX = 1_500_000

const OPTS = JSON.parse(readFileSync(join(ROOT, 'data/config-options.json'), 'utf8'))
const LUA_SRC = readFileSync(`${POB}/src/Modules/ConfigOptions.lua`, 'utf8').replace(/\r\n/g, '\n').split('\n')

type Scalar = number | string | boolean
type Ctx = { input?: Record<string, Scalar>; sel?: Record<string, Scalar> }
type Case = { var: string; val: Scalar; ctx?: Ctx }

const numOf = (v: unknown): Scalar => (v && typeof v === 'object' && '$num' in (v as Rec) ? Number((v as Rec).$num) : (v as Scalar))
const VALUES: Record<string, number[]> = { count: [0, 1, 7], integer: [0, 1, 7, -3], countAllowZero: [0, 1, 7, -3], float: [0, 1, 7, 2.5] }
const MAP_CTX: Ctx[] = []
for (const tier of ['HIGH', 'MED', 'LOW']) for (const eff of [undefined, 50]) MAP_CTX.push({ sel: { multiplierMapModTier: tier }, ...(eff === undefined ? {} : { input: { multiplierMapModEffect: eff } }) })

function cases(): { list: Case[]; sorted: string[] } {
  const list: Case[] = []
  const sorted: string[] = []
  for (const v of OPTS.vars) {
    if (!v.apply) continue
    const body = LUA_SRC.slice(v.apply.from - 1, v.apply.to).join('\n')
    if (/(^|[^i])pairs\(/.test(body)) sorted.push(v.var)
    const vals: Scalar[] = v.type === 'check' ? [true] : v.type === 'list' ? v.list.map((e: Rec) => numOf(e.val)) : VALUES[v.type] ?? []
    if (!vals.length) throw new Error(`${v.var}: type ${v.type} has no probe values`)
    for (const val of vals) {
      list.push({ var: v.var, val })
      if (v.var === 'MapPrefix1' || v.var === 'MapSuffix1') for (const ctx of MAP_CTX) list.push({ var: v.var, val, ctx })
      if (v.var === 'presetBossSkills') for (const boss of ['Pinnacle', 'Uber']) list.push({ var: v.var, val, ctx: { input: { enemyIsBoss: boss }, sel: { enemyIsBoss: boss } } })
    }
  }
  return { list, sorted }
}

// ---------------------------------------------------------------- Lua
const luaStr = (s: string): string => {
  let o = '"'
  for (const ch of Buffer.from(s, 'utf8').toString('latin1')) { const b = ch.charCodeAt(0); o += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? ch : '\\' + String(b).padStart(3, '0') }
  return o + '"'
}
const luaVal = (v: Scalar): string => (typeof v === 'string' ? luaStr(v) : typeof v === 'boolean' ? String(v) : Number.isInteger(v) ? String(v) : v.toPrecision(17))
const luaMap = (m: Record<string, Scalar> | undefined): string => `{${Object.entries(m ?? {}).map(([k, x]) => `[${luaStr(k)}]=${luaVal(x)}`).join(',')}}`
const casesLua = (cs: Case[], sorted: string[]): string =>
  `local CASES = {\n${cs.map((c) => `{var=${luaStr(c.var)},val=${luaVal(c.val)},input=${luaMap(c.ctx?.input)},sel=${luaMap(c.ctx?.sel)}}`).join(',\n')}\n}\n`
  + `local SORTED = {${sorted.map((v) => `[${luaStr(v)}]=true`).join(',')}}\n`

const BODY = String.raw`
local function esc(s)
  return '"' .. (s:gsub('[%c"\\\127-\255]', function(c)
    if c == '"' then return '\\"' elseif c == '\\' then return '\\\\' elseif c == '\n' then return '\\n' elseif c == '\r' then return '\\r' elseif c == '\t' then return '\\t' end
    return string.format('\\u%04x', c:byte())
  end)) .. '"'
end
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local function enc(v)
  local t = type(v)
  if t == "nil" then return "null" elseif t == "number" then return '{"num":"' .. RAWF("%.17g", v) .. '"}' elseif t == "string" then return esc(v)
  elseif t == "boolean" then return tostring(v) end
  return esc("<" .. t .. ">")
end
local function sortedKeys(...)
  local seen, keys = {}, {}
  for _, t in ipairs({...}) do for k in pairs(t) do if not seen[k] then seen[k] = true; keys[#keys + 1] = k end end end
  table.sort(keys)
  return keys
end
local function fmt(list, sort)
  local t = {}
  for i, m in ipairs(list) do t[i] = modLib.formatSourceMod(m) end
  if sort then table.sort(t) end
  for i, s in ipairs(t) do t[i] = esc(s) end
  return "[" .. table.concat(t, ",") .. "]"
end

local varList = require("Modules.ConfigOptions")
local byVar = {}
for _, vd in ipairs(varList) do if vd.var and vd.apply then byVar[vd.var] = vd end end
newBuild()
local ct = build.configTab
local function copy(t) local r = {} for k, v in pairs(t) do r[k] = v end return r end
local S0 = { input = copy(ct.input), placeholder = copy(ct.placeholder), enemyLevel = ct.enemyLevel, ctl = {} }
for var, c in pairs(ct.varControls) do S0.ctl[var] = { sel = c.selIndex, en = c.enabled, ph = c.placeholder } end
local function refill(t, src) for k in pairs(copy(t)) do t[k] = nil end for k, v in pairs(src) do t[k] = v end end
local function restore()
  refill(ct.input, S0.input)
  refill(ct.placeholder, S0.placeholder)
  ct.enemyLevel = S0.enemyLevel
  for var, c in pairs(ct.varControls) do local s = S0.ctl[var]; c.selIndex = s.sel; c.enabled = s.en; c.placeholder = s.ph end
end
local function tdiff(before, after)
  local out = {}
  for _, k in ipairs(sortedKeys(before, after)) do
    if before[k] ~= after[k] then out[#out + 1] = "[" .. esc(k) .. "," .. enc(after[k]) .. "]" end
  end
  return "[" .. table.concat(out, ",") .. "]"
end
local function ctlsnap() local r = {} for var, c in pairs(ct.varControls) do r[var] = { sel = c.selIndex, en = c.enabled } end return r end
local function ctldiff(b)
  local out = {}
  for _, var in ipairs(sortedKeys(ct.varControls)) do
    local c, s = ct.varControls[var], b[var]
    if c.selIndex ~= s.sel then out[#out + 1] = '["sel",' .. esc(var) .. ',' .. enc(c.selIndex) .. ']' end
    if c.enabled ~= s.en then out[#out + 1] = '["en",' .. esc(var) .. ',' .. enc(c.enabled) .. ']' end
  end
  return "[" .. table.concat(out, ",") .. "]"
end

-- the start state, as the Rust side rebuilds it
local start = {}
do
  local function kv(t) local o = {} for _, k in ipairs(sortedKeys(t)) do o[#o + 1] = "[" .. esc(k) .. "," .. enc(t[k]) .. "]" end return "[" .. table.concat(o, ",") .. "]" end
  local sels, ens = {}, {}
  for _, var in ipairs(sortedKeys(ct.varControls)) do
    local c = ct.varControls[var]
    if c.list and c.selIndex then sels[#sels + 1] = "[" .. esc(var) .. "," .. enc(c.selIndex) .. "]" end
    if c.enabled == false then ens[#ens + 1] = esc(var) end
  end
  start = '{"characterLevel":' .. enc(build.characterLevel) .. ',"disabled":[' .. table.concat(ens, ",") .. '],"enemyLevel":' .. enc(ct.enemyLevel)
    .. ',"input":' .. kv(ct.input) .. ',"placeholder":' .. kv(ct.placeholder) .. ',"selIndex":[' .. table.concat(sels, ",") .. ']}'
end

local out = { '{"luajit":' .. esc(jit.version) .. ',"start":' .. start .. ',"cases":[' }
for i, cs in ipairs(CASES) do
  restore()
  for k, v in pairs(cs.input) do ct.input[k] = v end
  for var, val in pairs(cs.sel) do
    local c = ct.varControls[var]
    for idx, e in ipairs(c.list) do if e.val == val then c.selIndex = idx end end
  end
  local before = { input = copy(ct.input), placeholder = copy(ct.placeholder), enemyLevel = ct.enemyLevel, ctl = ctlsnap() }
  local ml, eml = new("ModList"):ModList(), new("ModList"):ModList()
  local ok, err = pcall(byVar[cs.var].apply, cs.val, ml, eml, build)
  local sort = SORTED[cs.var] and true or false
  local parts = { '{"var":' .. esc(cs.var) .. ',"val":' .. enc(cs.val) }
  local ctxk = sortedKeys(cs.input, cs.sel)
  if #ctxk > 0 then
    local ci, cl = {}, {}
    for _, k in ipairs(sortedKeys(cs.input)) do ci[#ci + 1] = "[" .. esc(k) .. "," .. enc(cs.input[k]) .. "]" end
    for _, k in ipairs(sortedKeys(cs.sel)) do cl[#cl + 1] = "[" .. esc(k) .. "," .. enc(cs.sel[k]) .. "]" end
    parts[#parts + 1] = ',"ctx":{"input":[' .. table.concat(ci, ",") .. '],"sel":[' .. table.concat(cl, ",") .. ']}'
  end
  if not ok then parts[#parts + 1] = ',"error":' .. esc(tostring(err)) end
  parts[#parts + 1] = ',"sorted":' .. tostring(sort) .. ',"mods":' .. fmt(ml, sort) .. ',"enemyMods":' .. fmt(eml, sort)
    .. ',"input":' .. tdiff(before.input, ct.input) .. ',"placeholder":' .. tdiff(before.placeholder, ct.placeholder)
    .. ',"enemyLevel":' .. (ct.enemyLevel ~= before.enemyLevel and enc(ct.enemyLevel) or "null") .. ',"controls":' .. ctldiff(before.ctl) .. '}'
  out[#out + 1] = table.concat(parts) .. (i < #CASES and "," or "")
end
out[#out + 1] = "]}"
return table.concat(out, "\n")
`

async function docker(code: string): Promise<string> { // luajit-eval.lua: PoB HeadlessWrapper boot, then the chunk; last stdout line = {"result": <string>}
  const tmp = mkdtempSync(join(tmpdir(), 'pob-r35b-'))
  try {
    writeFileSync(join(tmp, 'code.lua'), code)
    const args = ['run', '--rm', '-v', `${POB}:/workdir:ro`, '-v', `${POBTS}/tools/oracle/lua:/bench:ro`, '-v', `${tmp}:/code:ro`, '-w', '/workdir/src',
      '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = (await run('docker', args, { encoding: 'utf8', maxBuffer: 512 << 20 })).stdout } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}

const { list, sorted } = cases()
const code = `${casesLua(list, sorted)}${BODY}`
const normAddr = (s: string): string => s.replace(/(function|table|userdata): 0x[0-9a-f]+/g, '$1: <addr>')
const text1 = normAddr(await docker(code))
const parsed = JSON.parse(text1)
if (parsed.cases.length !== list.length) throw new Error(`${list.length} cases -> ${parsed.cases.length} answers`)
if (!process.argv.includes('--no-selfcheck')) {
  const text2 = normAddr(await docker(code))
  if (text2 !== text1) throw new Error('two docker runs differ (nondeterminism: normalise it, R13)')
  console.log('selfcheck: 2 runs byte-identical')
}
if (text1.length > MAX) throw new Error(`fixture ${text1.length} bytes > ${MAX}`)
writeFileSync(OUT, text1 + '\n')
const errs = parsed.cases.filter((c: Rec) => c.error).length
const side = parsed.cases.filter((c: Rec) => c.input.length || c.placeholder.length || c.enemyLevel !== null || c.controls.length).length
const vars = new Set(parsed.cases.map((c: Rec) => c.var)).size
console.log(`${OUT}: ${parsed.cases.length} cases, ${vars} vars, ${errs} Lua errors, ${side} with side effects, sorted (pairs) vars: ${sorted.join(', ') || '-'}; ${text1.length} bytes; ${parsed.luajit}`)
