// R8.1: LuaJIT truth for R8 (display stats / SaveDB XML / build codes) on the 6 goldens + 20 R7-synth scenarios. Mechanics = export-offence.ts (docker LuaJIT, LUT mount, `%.17g` numbers).
// Per scenario: loadBuildFromXML -> wipeGlobalCache -> calcsTab:BuildOutput() (MAIN+CALCS, oracle path) -> build:RefreshStatList() (REAL Build.lua) ->
//   display = structure of boot.lua engine_displayStatsJSON (copied logic, boot.lua:546-788) + raw displayStats/minionDisplayStats entries (functions evaluated: condFunc/warnFunc/suffix)
//             + real statBox list + warnings (sorted: pairs over 4 string keys in AddDisplayStatList = R13);
//   save    = build:SaveDB("code") (ComposeXML wrapped to capture the tree). R13 sites (pairs over string keys): Build.lua:2288 savers (section order), ComposeXML attribs (xml.lua:120),
//             ConfigTab:Save :970/:983 (Input/Placeholder), CalcsTab:Save :204 (Input), ItemsTab:Save :1387 (Slot/SocketIdURL). Fixture XML = canonical (attribs sorted, those child groups sorted by name,
//             top level: Build first, rest by name); same byte multiset as the raw text (checked);
//   code    = LuaJIT common.base64 over zlib bytes. HeadlessWrapper's Deflate/Inflate are STUBS (return "", PLAN minefield #10), so zlib = Node zlib.deflateSync(canonical xml); LuaJIT does the base64
//             encode + the `+/`->`-_` swap and the decode, both checked against Node.
// Usage (Node 24, repo root):  node tools/lua-corpus/export-r8.ts [name-filter]   -> crates/pob-engine/tests/fixtures/r8/<slug>.{display.json,save.json,save.xml} (2 passes in memory, must be identical)
//                              node tools/lua-corpus/export-r8.ts perf            -> tools/.scratch/perf-r8.json (needs fixtures)
import '../upstream.ts'
import { execFile, execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { promisify } from 'node:util'
import { deflateSync, inflateSync } from 'node:zlib'
const run = promisify(execFile)
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-engine/tests/fixtures/r8')
const SYNTH = resolve(ROOT, 'crates/pob-calc/tests/fixtures/offence-synth')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const JOBS = Number(process.env.R8_JOBS ?? 4)
const roots = resolveRoots()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const median = (xs: number[]): number => { const s = [...xs].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]! }
// ~20 synth scenarios, one or more per class (no Alt gems: R13 ambiguous gem variants)
const SYNTH_SET = ['fd-gen-melee', 'fd-occ-spells', 'fd-occ-dot', 'fd-occ-trap-mine-totem', 'fd-occ-minions', 'fd-mir-mirage', 'fd-mir-bow-ballista', 'fd-cos-coc', 'fd-occ-poison-ignite', 'fd-all-ds',
  't-trap', 't-mine', 't-coc', 't-cwc', 't-cospri', 'mi-sacred-wisps', 'mn-raise-zombie', 'mn-spectre-hydra-mid', 'mn-golem-flame', 'fn-Spark']

async function prepareLuts(): Promise<string> {
  const dir = resolve(SCRATCH, 'luts')
  mkdirSync(dir, { recursive: true })
  const { findRequiredJewelLuts } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/timelessJewelLuts.mjs')).href)
  const { unpackJewelData } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/nodeJewelData.mjs')).href)
  for (const b of GOLDEN_BUILDS) for (const { fileStem } of findRequiredJewelLuts(readFileSync(b.xmlPath(roots), 'utf8'))) writeFileSync(resolve(dir, `${fileStem}.bin`), unpackJewelData(roots.pobRoot, fileStem).bytes)
  return dir
}
const JEWEL_SHIM = String.raw`
local jewelDataDir = os.getenv("POB_JEWEL_DATA_DIR")
local rawIoOpen = io.open
if jewelDataDir then
  io.open = function(path, mode)
    local fileName = path:match("Data/TimelessJewelData/([^/\\]+%.bin)$")
    if fileName and (not mode or mode:match("r")) then return rawIoOpen(jewelDataDir .. "/" .. fileName, mode) end
    return rawIoOpen(path, mode)
  end
end
local headlessNewFileSearch = NewFileSearch
function NewFileSearch(spec, findDirectories)
  if spec:match("Data/TimelessJewelData/.+%.bin$") then
    local file = io.open(spec, "rb")
    if file then file:close() return { GetFileName = function() return spec:match("([^/\\]+)$") end, GetFileModifiedTime = function() return 1 end } end
  end
  return headlessNewFileSearch(spec, findDirectories)
end
`
let LUTS = ''
async function ljEval(code: string): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r81-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${LUTS}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    try { const { stdout } = await run('docker', args, { encoding: 'utf8', maxBuffer: 1024 * 1024 * 1024 }); return JSON.parse(stdout.trim().split('\n').pop()!).result } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
const loadChunk = (xml: Buffer, name: string): string =>
  `loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(name)})\nif __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n`

// --- Lua helpers + display structure (copy of boot.lua:546-788 logic) ----------------------------------------------------------------------------
const HELPERS = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local function N(v) if v ~= v then return "nan" end return RAWF("%.17g", v) end
local function D(v, depth)
  local tv = type(v)
  if tv == "number" then return N(v) end
  if tv == "boolean" then return v end
  if tv == "string" then if v == "nan" or v == "inf" or v == "-inf" or v:find("^%-?[%d%.]+[eE]?[%+%-]?%d*$") then return "$str:" .. v end return v end
  if tv == "function" then return "<function>" end
  if tv ~= "table" then return "<" .. tv .. ">" end
  if depth > 4 then return "<deep>" end
  local n, cnt = #v, 0
  for _ in pairs(v) do cnt = cnt + 1 end
  if n > 0 and n == cnt then local l = {} for i = 1, n do l[i] = D(v[i], depth + 1) end return l end
  local o = obj()
  for k, x in pairs(v) do o[tostring(k)] = D(x, depth + 1) end
  return o
end
local function matchDisplayFlags(reqFlags, notFlags, flags)
  if type(reqFlags) == "string" then reqFlags = { reqFlags } end
  if reqFlags then for _, flag in ipairs(reqFlags) do if not flags[flag] then return false end end end
  if type(notFlags) == "string" then notFlags = { notFlags } end
  if notFlags then for _, flag in ipairs(notFlags) do if flags[flag] then return false end end end
  return true
end
local hexColorNames = {}
for _, name in ipairs({ "LIFE", "MANA", "ES", "RAGE", "STRENGTH", "DEXTERITY", "INTELLIGENCE", "RARE", "PHYS", "FIRE", "COLD", "LIGHTNING", "CHAOS", "EVASION", "CURRENCY", "CUSTOM", "WARNING", "NEGATIVE", "POSITIVE", "WARD", "ARMOUR", "NORMAL" }) do
  local code = colorCodes[name]
  if code and not hexColorNames[code] then hexColorNames[code] = name:lower() end
end
local function colorClassName(code)
  if not code or code == "" then return "default" end
  if hexColorNames[code] then return hexColorNames[code] end
  if code == "^x808080" then return "muted" end
  local digit = code:match("^%^(%d)$")
  if digit then return ({ ["7"] = "default", ["8"] = "muted" })[digit] or ("code" .. digit) end
  if code:match("^%^x%x%x%x%x%x%x$") then return "hex" .. code:sub(3):lower() end
  return "default"
end
local function leadingColorClass(str)
  if not str or str == "" then return "default" end
  local hex = str:match("^(%^x%x%x%x%x%x%x)")
  if hex then return colorClassName(hex) end
  local digit = str:match("^(%^%d)")
  if digit then return colorClassName(digit) end
  return "default"
end
local function buildDisplayStatSections(statList, actor)
  local sections, currentRows, slot = {}, nil, 1
  local function closeSection() if currentRows and #currentRows > 0 then sections[#sections + 1] = { slot = slot, rows = currentRows } end currentRows = nil slot = slot + 1 end
  local function rows() currentRows = currentRows or {} return currentRows end
  for _, statData in ipairs(statList) do
    if matchDisplayFlags(statData.flag, statData.notFlag, actor.mainSkill.skillFlags) then
      if statData.stat then
        local statVal = actor.output[statData.stat]
        if statVal and statData.childStat then statVal = statVal[statData.childStat] end
        if statVal and ((statData.condFunc and statData.condFunc(statVal, actor.output)) or (not statData.condFunc and statVal ~= 0)) then
          local overCapStatVal = actor.output[statData.overCapStat]
          if overCapStatVal and statData.overCapStatCondFunc and not statData.overCapStatCondFunc(statVal, actor.output) then overCapStatVal = nil end
          if statData.stat == "SkillDPS" then
            table.sort(statVal, function(a, b) return (a.dps * a.count) > (b.dps * b.count) end)
            local out = rows()
            for _, skillData in ipairs(statVal) do
              local label = skillData.name
              if skillData.count and skillData.count >= 2 then label = tostring(skillData.count) .. "x " .. label end
              if skillData.trigger and skillData.trigger ~= "" then label = label .. " (" .. skillData.trigger .. ")" end
              if skillData.skillPart then label = label .. " -- " .. skillData.skillPart end
              if skillData.source then label = label .. " [" .. skillData.source .. "]" end
              local formatted = build:FormatStat({ fmt = ".1f", compactValue = statData.compactValue }, skillData.dps * skillData.count, overCapStatVal)
              out[#out + 1] = { label = label, value = StripEscapes(formatted), color = colorClassName(colorCodes.CUSTOM), valueColor = leadingColorClass(formatted) }
            end
          elseif not statData.hideStat then
            local colorOverride = nil
            if actor.output[statData.stat .. "Warning"] or (statData.warnFunc and statData.warnFunc(statVal, actor.output) and statData.warnColor) then colorOverride = colorCodes.NEGATIVE end
            local formatted = build:FormatStat(statData, statVal, overCapStatVal, colorOverride)
            if statData.suffix and (not statData.suffixCondFunc or statData.suffixCondFunc(statVal, actor.output)) then
              local suffix = statData.suffix
              if type(suffix) == "function" then suffix = suffix(statVal, actor.output) end
              if suffix then formatted = formatted .. "^x808080 (" .. suffix .. ")" end
            end
            local out = rows()
            out[#out + 1] = { label = StripEscapes(statData.label), value = StripEscapes(formatted), color = colorClassName(statData.color or "^7"), valueColor = leadingColorClass(formatted),
              statKey = (not statData.childStat) and statData.stat or nil }
          end
        end
      elseif statData.label and statData.condFunc and statData.condFunc(actor.output) then
        local raw = actor.output[statData.labelStat]
        local out = rows()
        out[#out + 1] = { label = StripEscapes(statData.label), value = StripEscapes(tostring(raw) .. "%" .. (statData.val and (" (" .. statData.val .. ")") or "")), color = colorClassName(statData.color or "^7"),
          valueColor = "default", statKey = statData.labelStat }
      else closeSection() end
    end
  end
  closeSection()
  return sections
end
local playerTitles = { "Offence", "Attributes", "Devotion", "Effective Hit Pool", "Life", "Mana", "Energy Shield", "Ward", "Rage", "Recovery / Degen", "Evasion", "Armour", "Block, Dodge & Suppression", "Resistances", "Misc", "Full DPS", "Skill DPS" }
local function titled(rowSections, titles, actorName)
  local out = {}
  for _, s in ipairs(rowSections) do out[#out + 1] = { actor = actorName, title = titles[s.slot] or ("More " .. actorName .. " stats (slot " .. s.slot .. ")"), rows = s.rows, slot = s.slot } end
  return out
end
local function displayStructure() -- == engine_displayStatsJSON before dkjson.encode
  local sections, mainEnv = {}, build.calcsTab.mainEnv
  if mainEnv.minion then for _, s in ipairs(titled(buildDisplayStatSections(build.minionDisplayStats, mainEnv.minion), { "Minion" }, "minion")) do sections[#sections + 1] = s end end
  for _, s in ipairs(titled(buildDisplayStatSections(build.displayStats, mainEnv.player), playerTitles, "player")) do sections[#sections + 1] = s end
  return sections
end
`
const PROBE = String.raw`
local function rawList(list, actor)
  local t = {}
  for i, s in ipairs(list) do
    local e = obj()
    e.index = N(i)
    for k, v in pairs(s) do e[k] = D(v, 0) end
    e.separator = (s.stat == nil and s.label == nil) or nil
    e.flagMatch = matchDisplayFlags(s.flag, s.notFlag, actor.mainSkill.skillFlags)
    if s.stat then
      local out = actor.output
      local v = out[s.stat]
      if v and s.childStat then v = v[s.childStat] end
      e.valueType = type(v)
      if type(v) == "number" then e.value = N(v) end
      if type(v) == "table" then e.valueLen = N(#v) end
      if s.condFunc and v ~= nil then local ok, r = pcall(s.condFunc, v, out) e.condResult = ok and r or ("<error " .. tostring(r) .. ">") end
      if s.warnFunc and v ~= nil then local ok, r = pcall(s.warnFunc, v, out) e.warnResult = ok and D(r, 0) or ("<error " .. tostring(r) .. ">") end
      if s.suffixCondFunc and v ~= nil then local ok, r = pcall(s.suffixCondFunc, v, out) e.suffixCondResult = ok and r or "<error>" end
      if type(s.suffix) == "function" and v ~= nil then local ok, r = pcall(s.suffix, v, out) e.suffixResult = ok and D(r, 0) or "<error>" end
      if s.overCapStat then e.overCapValue = out[s.overCapStat] ~= nil and D(out[s.overCapStat], 0) or nil end
      e.hasWarningKey = out[s.stat .. "Warning"] ~= nil
    end
    t[i] = e
  end
  return t
end
local res = obj()
wipeGlobalCache()
build.calcsTab:BuildOutput()
local mainEnv = build.calcsTab.mainEnv
build:RefreshStatList()
res.sections = displayStructure()
res.rawPlayer, res.rawMinion = rawList(build.displayStats, mainEnv.player), mainEnv.minion and rawList(build.minionDisplayStats, mainEnv.minion) or nil
res.hasMinion = mainEnv.minion ~= nil
res.infoMessage, res.infoMessage2, res.disableReason = mainEnv.player.mainSkill.infoMessage, mainEnv.player.mainSkill.infoMessage2, mainEnv.player.mainSkill.disableReason
res.mainSkillDisabled = mainEnv.player.mainSkill.skillFlags.disable and true or false
local wl = {}
for i, l in ipairs(build.controls.warnings.lines) do wl[i] = l end
res.warningsRawCount = N(#wl)
table.sort(wl)
res.warnings = wl
local iw = obj()
for k, v in pairs(mainEnv.itemWarnings or {}) do iw[k] = D(v, 0) end
res.itemWarnings = iw
local box = {}
for i, e in ipairs(build.controls.statBox.list) do
  local b = obj()
  b.height, b.align, b.x, b.text, b.value = e.height and N(e.height), e.align, e.x and N(e.x), e[1], e[2]
  if e.underline then b.underline = D(e.underline, 0) end
  b.actorName, b.modNames = e.actorName, e.modNames and D(e.modNames, 0)
  if type(e.breakdown) == "table" then b.breakdownLines = N(#e.breakdown) end
  if e.ignoredSections then local k = {} for key in pairs(e.ignoredSections) do k[#k + 1] = tostring(key) end table.sort(k) b.ignoredSections = k end
  if e.breakdownColour then b.breakdownColour = D(e.breakdownColour, 0) end
  box[i] = b
end
res.statBox = box
-- SaveDB: capture the composed tree
local cx = common.xml
local origCompose, captured = cx.ComposeXML, nil
cx.ComposeXML = function(root) captured = root return origCompose(root) end
local raw = build:SaveDB("code")
cx.ComposeXML = origCompose
local function encodeContent(text) local sub = { ["<"] = "lt", [">"] = "gt", ["&"] = "amp", ["'"] = "apos", ['"'] = "quot" } return (text:gsub("[<>&'\"]", function(c) return "&" .. sub[c] .. ";" end)) end
local RANK = { ConfigSet = { Input = "1", Placeholder = "2" }, Calcs = { Input = "1" }, ItemSet = { Slot = "1", SocketIdURL = "1" } }
local multi, sorted, specNodes = {}, {}, {}
local r13Gems = {}
local function canon(node)
  if node.elem == "Gem" and node.attrib and node.attrib.gemId and data.gemsByGameId[node.attrib.gemId] then -- R13: SkillsTab.lua:472-477 pairs over variants sharing grantedEffectId
    local cands = {}
    for vid, v in pairs(data.gemsByGameId[node.attrib.gemId]) do if v.grantedEffectId == node.attrib.skillId then cands[#cands + 1] = tostring(vid) .. "=" .. tostring(v.nameSpec or v.name) end end
    if #cands > 1 then
      table.sort(cands)
      r13Gems[#r13Gems + 1] = { skillId = node.attrib.skillId, candidates = cands, observed = node.attrib.variantId }
      local a = {}
      for k, v in pairs(node.attrib) do a[k] = v end
      a.nameSpec, a.variantId = "<R13-ambiguous>", "<R13-ambiguous>"
      node = { elem = node.elem, attrib = a }
    end
  end
  if node.elem == "Spec" and node.attrib then -- PassiveSpec:Save :199/:203 pairs over allocNodes/masterySelections (+ :226 jewels, :239 overrides below)
    local a = {}
    for k, v in pairs(node.attrib) do a[k] = v end
    specNodes[#specNodes + 1] = a.nodes
    local ids = {}
    for id in a.nodes:gmatch("[^,]+") do ids[#ids + 1] = tonumber(id) end
    table.sort(ids)
    for i, id in ipairs(ids) do ids[i] = RAWF("%d", id) end
    a.nodes = table.concat(ids, ",")
    local ms = {}
    for t in a.masteryEffects:gmatch("%b{}") do ms[#ms + 1] = t end
    table.sort(ms)
    a.masteryEffects = table.concat(ms, ",")
    node = { elem = node.elem, attrib = a, unpack(node) }
    sorted.Spec = (sorted.Spec or 0) + #ids
  end
  if node.elem == "Sockets" or node.elem == "Overrides" then
    local l = {}
    for i, c in ipairs(node) do l[i] = c end
    table.sort(l, function(x, y) return tonumber(x.attrib.nodeId) < tonumber(y.attrib.nodeId) end)
    node = { elem = node.elem, attrib = node.attrib, unpack(l) }
    sorted[node.elem] = (sorted[node.elem] or 0) + #l
  end
  local kids, rank, pos, grp = {}, RANK[node.elem], {}, {}
  for i, c in ipairs(node) do
    kids[i] = type(c) == "table" and canon(c) or c
    if rank and type(c) == "table" and rank[c.elem] then pos[#pos + 1] = i grp[#grp + 1] = kids[i] end
  end
  if #grp > 0 then
    table.sort(grp, function(a, b) local ka, kb = rank[a.elem] .. "\0" .. a.attrib.name, rank[b.elem] .. "\0" .. b.attrib.name return ka < kb end)
    for j, i in ipairs(pos) do kids[i] = grp[j] end
    sorted[node.elem] = (sorted[node.elem] or 0) + #grp
  end
  local nattr = 0
  for _ in pairs(node.attrib or {}) do nattr = nattr + 1 end
  if nattr >= 2 then multi[node.elem] = (multi[node.elem] or 0) + 1 end
  return { elem = node.elem, attrib = node.attrib, kids = kids }
end
local c = canon(captured)
local top = { c.kids[1] }
local rest = {}
for i = 2, #c.kids do rest[#rest + 1] = c.kids[i] end
table.sort(rest, function(a, b) return a.elem < b.elem end)
for _, k in ipairs(rest) do top[#top + 1] = k end
c.kids = top
local frag = { '<?xml version="1.0" encoding="UTF-8"?>\n' }
local function compose(n, lvl)
  frag[#frag + 1] = string.rep("\t", lvl) .. "<" .. n.elem
  local keys = {}
  for k, v in pairs(n.attrib or {}) do if v then keys[#keys + 1] = k end end
  table.sort(keys)
  for _, k in ipairs(keys) do frag[#frag + 1] = " " .. k .. '="' .. encodeContent(n.attrib[k]) .. '"' end
  if not n.kids[1] then frag[#frag + 1] = "/>\n" return end
  frag[#frag + 1] = ">\n"
  for _, k in ipairs(n.kids) do
    if type(k) == "table" then compose(k, lvl + 1) else frag[#frag + 1] = string.rep("\t", lvl + 1) .. encodeContent(k) .. "\n" end
  end
  frag[#frag + 1] = string.rep("\t", lvl) .. "</" .. n.elem .. ">\n"
end
compose(c, 0)
local canonical = table.concat(frag)
local function hist(s) local h = {} for i = 1, 256 do h[i] = 0 end for i = 1, #s do local b = s:byte(i) + 1 h[b] = h[b] + 1 end return table.concat(h, ",") end
local rawOrder = {}
for i, n in ipairs(captured) do rawOrder[i] = n.elem end
res.save = { raw = raw, canonical = canonical, sameByteMultiset = hist(raw) == hist(canonical), rawTopOrder = rawOrder, multiAttrElems = multi, sortedGroups = sorted,
  specNodes = specNodes, r13Gems = r13Gems, configSets = #build.configTab.configSetOrderList, itemSets = #build.itemsTab.itemSetOrderList }
return res
`
// phase 2: LuaJIT base64 over the zlib bytes (+ libz availability via ffi, + timing of the real Lua base64 for the perf rows)
const CODE_PROBE = (items: { z: Buffer; code: string }[]): string => String.raw`
local base64 = common.base64
local ffi = require("ffi")
local zok = pcall(ffi.load, "z")
local items = { ${items.map((i) => `{ z = ${luaBytes(i.z)}, code = ${JSON.stringify(i.code)} }`).join(',\n')} }
local out = { libzFfi = zok, items = {} }
for i, it in ipairs(items) do
  local code = (base64.encode(it.z):gsub("+", "-"):gsub("/", "_"))
  local back = base64.decode((code:gsub("-", "+"):gsub("_", "/")))
  out.items[i] = { code = code, codeEqual = code == it.code, decodeEqual = back == it.z, len = #code }
end
return out
`

// --- deterministic JSON (as export-offence.ts) ---------------------------------------------------------------------------------------------------
function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
const b64url = (z: Buffer): string => z.toString('base64').replace(/\+/g, '-').replace(/\//g, '_')
type Scenario = { slug: string; name: string; xml: Buffer; kind: 'golden' | 'synth' }
function scenarios(filter?: string): Scenario[] {
  const s: Scenario[] = GOLDEN_BUILDS.map((b: Rec) => ({ slug: b.name.replace(/ /g, '-'), name: b.name, xml: readFileSync(b.xmlPath(roots)), kind: 'golden' as const }))
  for (const n of SYNTH_SET) s.push({ slug: n, name: n, xml: readFileSync(resolve(SYNTH, `${n}.xml`)), kind: 'synth' })
  return filter ? s.filter((x) => x.slug.includes(filter)) : s
}
async function pool<T, R>(xs: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(xs.length)
  let next = 0
  await Promise.all(Array.from({ length: JOBS }, async () => { for (;;) { const i = next++; if (i >= xs.length) return; res[i] = await f(xs[i]!); console.error(`  done ${i + 1}/${xs.length}`) } }))
  return res
}

// one full pass: docker probes -> files (path -> text) + report rows
async function pass(list: Scenario[]): Promise<{ files: Map<string, string>; rows: Rec[]; rawFirst: string; libz: boolean; codeFail: string[] }> {
  const probes = await pool(list, (s) => ljEval(loadChunk(s.xml, s.name) + `local function probe()\n${HELPERS}\n${PROBE}\nend\nreturn probe()\n`))
  const zs = probes.map((r) => deflateSync(Buffer.from(r.save.canonical, 'utf8')))
  const codes = zs.map(b64url)
  const c2 = await ljEval(CODE_PROBE(zs.map((z, i) => ({ z, code: codes[i]! }))))
  const files = new Map<string, string>()
  const rows: Rec[] = []
  const codeFail: string[] = []
  const producer = { generator: 'tools/lua-corpus/export-r8.ts', image: LUAJIT_IMAGE, note: 'LuaJIT docker (Alpine/musl); display/SaveDB = real PoB code, Deflate/Inflate = Node zlib (LuaJIT stubs, minefield #10), base64 = LuaJIT common.base64' }
  list.forEach((s, i) => {
    const r = probes[i]!
    const xml = r.save.canonical as string
    const it = c2.items[i]
    const nodeInflate = inflateSync(zs[i]!).toString('utf8') === xml
    if (!it.codeEqual || !it.decodeEqual || !nodeInflate || (!r.save.sameByteMultiset && r.save.r13Gems.length === 0)) codeFail.push(s.slug)
    const nrows = (r.sections as Rec[]).reduce((a, x) => a + x.rows.length, 0)
    const disp = { format: 1, producer, build: s.name, kind: s.kind, xmlSha256: sha(s.xml), sections: r.sections, rawPlayer: r.rawPlayer, rawMinion: r.rawMinion, hasMinion: r.hasMinion, infoMessage: r.infoMessage, infoMessage2: r.infoMessage2,
      disableReason: r.disableReason, mainSkillDisabled: r.mainSkillDisabled, warnings: r.warnings, warningsRawCount: r.warningsRawCount, warningsNote: 'sorted: pairs over 4 string keys (Build.lua AddDisplayStatList) = R13', itemWarnings: r.itemWarnings, statBox: r.statBox }
    const save = { format: 1, producer, build: s.name, kind: s.kind, xmlSha256: sha(s.xml), canonicalXmlSha256: sha(xml), canonicalXmlLen: Buffer.byteLength(xml), configSets: r.save.configSets, r13Gems: (r.save.r13Gems as Rec[]).map(({ observed, ...x }) => x), itemSets: r.save.itemSets,
      multiAttrElems: r.save.multiAttrElems, sortedGroups: r.save.sortedGroups, code: codes[i], codeLen: codes[i]!.length, zlibLen: zs[i]!.length, base64: { luajitEncodeEqualsNode: it.codeEqual, luajitDecodeEqualsZlib: it.decodeEqual },
      zlib: 'node zlib.deflateSync(canonical xml) default level', rule: 'canonical = attribs sorted; ConfigSet Input/Placeholder, Calcs Input, ItemSet Slot/SocketIdURL sorted by name; top level Build first then by name' }
    for (const [n, t] of [[`${s.slug}.display.json`, ser(disp) + '\n'], [`${s.slug}.save.json`, ser(save) + '\n'], [`${s.slug}.save.xml`, xml]] as const) {
      if (Buffer.byteLength(t) > MAX_BYTES) throw new Error(`${n}: ${Buffer.byteLength(t)} bytes > ${MAX_BYTES} -- STOP`)
      files.set(n, t)
    }
    rows.push({ build: s.name, kind: s.kind, sections: r.sections.length, rows: nrows, minionSections: (r.sections as Rec[]).filter((x) => x.actor === 'minion').length, rawStats: r.rawPlayer.length, warnings: r.warnings.length,
      statBox: r.statBox.length, xmlLen: Buffer.byteLength(xml), rawLen: Buffer.byteLength(r.save.raw), zlibLen: zs[i]!.length, codeLen: codes[i]!.length, rawTopOrder: r.save.rawTopOrder.join(','), rawIsCanonical: r.save.raw === xml,
      rawSha: sha(r.save.raw), specNodesSha: sha((r.save.specNodes as string[]).join('|')), specNodesRawSorted: (r.save.specNodes as string[]).every((n) => { const a = n.split(',').map(Number); return a.every((x, j) => j === 0 || a[j - 1]! <= x) }), multiAttr: r.save.multiAttrElems, r13GemsObserved: (r.save.r13Gems as Rec[]).map((g) => g.observed), sorted: r.save.sortedGroups, itemWarnings: Object.keys(r.itemWarnings).length })
  })
  return { files, rows, rawFirst: probes[0]!.save.raw, libz: c2.libzFfi, codeFail }
}

async function fixtures(filter?: string): Promise<void> {
  LUTS = await prepareLuts()
  const list = scenarios(filter)
  console.error(`pass 1 (${list.length} scenarios)`)
  const a = await pass(list)
  console.error('pass 2')
  const b = await pass(list)
  const diff = [...a.files.keys()].filter((k) => a.files.get(k) !== b.files.get(k))
  const rawDiff = a.rows.filter((r, i) => r.rawSha !== b.rows[i]!.rawSha).map((r) => r.build)
  const nodesDiff = a.rows.filter((r, i) => r.specNodesSha !== b.rows[i]!.specNodesSha).map((r) => r.build)
  const orderDiff = a.rows.filter((r, i) => r.rawTopOrder !== b.rows[i]!.rawTopOrder).map((r) => r.build)
  if (diff.length && process.env.R8_DEBUG) for (const k of diff) { writeFileSync(resolve(SCRATCH, `dbg-a-${k}`), a.files.get(k)!); writeFileSync(resolve(SCRATCH, `dbg-b-${k}`), b.files.get(k)!) }
  if (diff.length) { console.log(JSON.stringify({ STOP: 'passes differ', diff }, null, 1)); process.exitCode = 1; return }
  if (a.codeFail.length) { console.log(JSON.stringify({ STOP: 'code/canonical checks failed', fail: a.codeFail }, null, 1)); process.exitCode = 1; return }
  mkdirSync(OUT, { recursive: true })
  const shas: Rec = {}
  for (const [n, t] of a.files) { writeFileSync(resolve(OUT, n), t, 'utf8'); shas[n] = sha(t) }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'r8-raw-save-sample.xml'), a.rawFirst)
  console.log(JSON.stringify({ scenarios: list.length, files: a.files.size, passesIdentical: true, combinedSha256: sha(Object.keys(shas).sort(byteOrder).map((k) => `${k}:${shas[k]}`).join('\n')), libzViaFfi: a.libz,
    rawTextDiffersBetweenPasses: rawDiff, rawTopOrderDiffersBetweenPasses: orderDiff, specNodesRawOrderDiffersBetweenPasses: nodesDiff, rows: a.rows.map(({ rawSha, specNodesSha, ...x }) => x) }, null, 1))
}

async function perf(): Promise<void> {
  LUTS = await prepareLuts()
  const RUNS = 7
  const out: Rec = { runs: RUNS, note: 'LuaJIT docker jit.on(), os.clock CPU ms, run 0 = warm-up (dropped). refreshStatList = build:RefreshStatList() (real Build.lua incl. EstimatePlayerProgress); displayStructure = engine_displayStatsJSON-equivalent builder; saveDB = build:SaveDB("code"); base64Encode/Decode = real Lua common.base64 on the zlib bytes (+/- swap included). Deflate/Inflate in LuaJIT are stubs: nodeZlib_* is Node zlib (NOT LuaJIT, for scale only)', builds: {} }
  for (const b of GOLDEN_BUILDS as Rec[]) {
    const slug = b.name.replace(/ /g, '-')
    const xmlPath = resolve(OUT, `${slug}.save.xml`)
    if (!existsSync(xmlPath)) throw new Error(`run fixtures first: ${xmlPath} missing`)
    const xml = readFileSync(xmlPath)
    const z = deflateSync(xml)
    const lua = String.raw`
local function now() return os.clock() * 1000 end
jit.on()
wipeGlobalCache()
build.calcsTab:BuildOutput()
local z = ${luaBytes(z)}
local base64 = common.base64
local R = { refreshStatList = {}, displayStructure = {}, saveDB = {}, base64Encode = {}, base64Decode = {} }
for i = 0, ${RUNS} do
  local t0 = now(); build:RefreshStatList(); local t1 = now(); displayStructure(); local t2 = now(); build:SaveDB("code"); local t3 = now()
  local code = (base64.encode(z):gsub("+", "-"):gsub("/", "_")); local t4 = now()
  base64.decode((code:gsub("-", "+"):gsub("_", "/"))); local t5 = now()
  if i > 0 then R.refreshStatList[i], R.displayStructure[i], R.saveDB[i], R.base64Encode[i], R.base64Decode[i] = t1 - t0, t2 - t1, t3 - t2, t4 - t3, t5 - t4 end
end
R.jit = jit.version
return R
`
    const r = await ljEval(loadChunk(readFileSync(b.xmlPath(roots)), b.name) + `local function probe()\n${HELPERS}\nlocal displayStructure = displayStructure\n${lua}\nend\nreturn probe()\n`)
    const t = (f: () => void): number[] => { const xs: number[] = []; for (let i = 0; i <= RUNS; i++) { const t0 = performance.now(); f(); if (i > 0) xs.push(performance.now() - t0) } return xs }
    const nz = t(() => { deflateSync(xml) }), ni = t(() => { inflateSync(z) })
    const s = (xs: number[]) => ({ median: median(xs), min: Math.min(...xs), n: xs.length })
    out.builds[b.name] = { luajit: r.jit, xmlBytes: xml.length, zlibBytes: z.length, refreshStatList: s(r.refreshStatList), displayStructure: s(r.displayStructure), saveDB: s(r.saveDB), base64Encode: s(r.base64Encode), base64Decode: s(r.base64Decode),
      nodeZlib_deflate: s(nz), nodeZlib_inflate: s(ni), series: { refreshStatList: r.refreshStatList, displayStructure: r.displayStructure, saveDB: r.saveDB, base64Encode: r.base64Encode, base64Decode: r.base64Decode } }
    console.error(`perf ${b.name}: refresh ${median(r.refreshStatList).toFixed(2)} ms, structure ${median(r.displayStructure).toFixed(2)}, saveDB ${median(r.saveDB).toFixed(2)}, b64enc ${median(r.base64Encode).toFixed(2)}, b64dec ${median(r.base64Decode).toFixed(2)}`)
  }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-r8.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(Object.fromEntries(Object.entries(out.builds).map(([k, v]: [string, any]) => [k, Object.fromEntries(['refreshStatList', 'displayStructure', 'saveDB', 'base64Encode', 'base64Decode', 'nodeZlib_deflate', 'nodeZlib_inflate'].map((m) => [m, +v[m].median.toFixed(3)]))])), null, 1)) // eslint-disable-line
}

void readdirSync
if (process.argv[2] === 'perf') await perf()
else await fixtures(process.argv[2])
