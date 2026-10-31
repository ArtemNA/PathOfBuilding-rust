// R7.1: LuaJIT truth for offence on the 6 goldens (R7-ARCH §1 p.1-2, §2 gate (a)): final MAIN `output`, checkpoints OC1..OC6 INSIDE calcs.offence, GlobalCache
// after buildOutput(MAIN), calcFullDPS result, buildActiveSkill/perform/cacheData call logs, "golden = fresh process?" measurement, LuaJIT perf.
// One docker run per golden (PoB boot + loadBuildFromXML, timeless LUTs mounted as export-skills.ts / export-env.ts -- MINEFIELD-RUST R5). Probe steps:
//   0. dump build.calcsTab.mainOutput exactly as left by loadBuildFromXML (no wipe: the "golden" path);
//   A. wipeGlobalCache + calcs.buildOutput(build,"MAIN") (wrappers on initEnv/perform/buildActiveSkill/cacheData/calcFullDPS) -> final output, GlobalCache, fullDPS;
//   B. the same with jit.off()+jit.flush()+debug.sethook(line) -> OC1..OC6 (first hit of the line inside calcs.offence with env==top MAIN env, actor==env.player);
//      B's final output must equal A's (reported);
//   C. wipeGlobalCache + build.calcsTab:BuildOutput() (MAIN+CALCS, the oracle's config path) -> mainOutput, compared with A.
// OC lines (CalcOffence.lua @ ed354c2f8; marker -> first executable line, all at function-body level): OC1 1056->1057, OC2 1937->1938, OC3 2606 (the `for` itself),
// OC4 4155->4156, OC5 5771->5772, OC6 = line 6263 (`end` of calcs.offence: all statements done, hook fires before RET0).
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-offence.ts  -> crates/pob-calc/tests/fixtures/<build>.offence.json (+ -<n> parts if > 1.45 MB)
//                                        node tools/lua-corpus/export-offence.ts perf -> tools/.scratch/perf-offence.json
// Encoding as export-skills.ts: Lua numbers = `%.17g` strings, strings = strings (a string that LOOKS like a number is prefixed `$str:`), booleans, tables = objects keyed by
// tostring(key). Only normalisation: `function|table: 0x<addr>` -> `<addr>`; mod/pairs-order sets sorted (R13).
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line

if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_450_000
const roots = resolveRoots()
const git = (dir: string): string => execFileSync('git', ['-C', dir, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)

// --- docker twin of export-env.ts ljEval (LUT mount + io.open/NewFileSearch shim, luajit-dump17.lua:35-59) -----------------------------------
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
async function prepareLuts(): Promise<string> {
  const dir = resolve(SCRATCH, 'luts')
  mkdirSync(dir, { recursive: true })
  const { findRequiredJewelLuts } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/timelessJewelLuts.mjs')).href)
  const { unpackJewelData } = await import(pathToFileURL(resolve(roots.oracleRoot, 'src/nodeJewelData.mjs')).href)
  for (const b of GOLDEN_BUILDS) for (const { fileStem } of findRequiredJewelLuts(readFileSync(b.xmlPath(roots), 'utf8'))) writeFileSync(resolve(dir, `${fileStem}.bin`), unpackJewelData(roots.pobRoot, fileStem).bytes)
  return dir
}
function ljEval(code: string, lutsDir: string): Rec {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r71-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${lutsDir}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    try { out = execFileSync('docker', args, { encoding: 'utf8', maxBuffer: 1024 * 1024 * 1024 }) } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
const loadChunk = (xml: Buffer, name: string): string =>
  `loadBuildFromXML(${luaBytes(xml)}, ${JSON.stringify(name)})\nif __mainObject__.promptMsg then error("FATAL build load failed: " .. tostring(__mainObject__.promptMsg)) end\n`

// --- Lua: shared wrappers ----------------------------------------------------------------------------------------------------------------
// topEnv = the env returned by the FIRST calcs.initEnv(build,"MAIN") after install (= buildOutput's own env; buildActiveSkill/calcFullDPS call initEnv later).
const WRAP = String.raw`
local calcs = build.calcsTab.calcs
local W = { topEnv = nil, st = nil }
local function resetLogs() W.topEnv = nil; W.st = { init = 0, perform = {}, depth = 0, bas = {}, cache = {}, offence = {}, fullDPS = nil } end
resetLogs()
local oInit, oPerform, oBAS, oFull, oCD, oOffence = calcs.initEnv, calcs.perform, calcs.buildActiveSkill, calcs.calcFullDPS, cacheData, calcs.offence
local function onInit(mode, env, ...) W.st.init = W.st.init + 1; if not W.topEnv and mode == "MAIN" then W.topEnv = env end return env, ... end
local function installBase()
  calcs.initEnv = function(b, mode, ...) return onInit(mode, oInit(b, mode, ...)) end
  calcs.perform = function(env, skipEHP)
    local st = W.st
    local ok, u = pcall(cacheSkillUUID, env.player.mainSkill, env)
    st.perform[#st.perform + 1] = { depth = st.depth, mode = env.mode, skipEHP = skipEHP and true or false, top = env == W.topEnv, uuid = ok and u or "<err>" }
    st.depth = st.depth + 1
    local r = oPerform(env, skipEHP)
    st.depth = st.depth - 1
    return r
  end
  calcs.buildActiveSkill = function(env, mode, skill, targetUUID, limited)
    local st = W.st
    local lim = {} for i, u in ipairs(limited or {}) do lim[i] = u end
    local ok, u = pcall(cacheSkillUUID, skill, env)
    st.bas[#st.bas + 1] = { depth = st.depth, mode = mode, targetUUID = targetUUID, skillUUID = ok and u or "<err>", limited = lim, envMode = env.mode }
    return oBAS(env, mode, skill, targetUUID, limited)
  end
  calcs.calcFullDPS = function(...) local r = oFull(...) W.st.fullDPS = r return r end
  calcs.offence = function(env, actor, sk)
    local st = W.st
    local ok, u = pcall(cacheSkillUUID, sk, env)
    st.offence[#st.offence + 1] = { depth = st.depth, mode = env.mode, top = env == W.topEnv, player = actor == env.player, uuid = ok and u or "<err>" }
    return oOffence(env, actor, sk)
  end
  cacheData = function(uuid, env) local st = W.st st.cache[#st.cache + 1] = { mode = env.mode, uuid = uuid, top = env == W.topEnv } return oCD(uuid, env) end
end
local function uninstallBase() calcs.initEnv, calcs.perform, calcs.buildActiveSkill, calcs.calcFullDPS, cacheData, calcs.offence = oInit, oPerform, oBAS, oFull, oCD, oOffence end
`

// --- Lua: dump helpers + probe (fixture mode) -------------------------------------------------------------------------------------------
const DUMP_HELPERS = String.raw`
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local function N(v) if v ~= v then return "nan" end return RAWF("%.17g", v) end
local fmtMod = modLib.formatSourceMod
local ambiguous = 0
local function className(t) local mt = getmetatable(t) if type(mt) == "table" then return rawget(mt, "_className") end end
local function isMod(t) return rawget(t, "name") ~= nil and rawget(t, "type") ~= nil and rawget(t, "flags") ~= nil and rawget(t, "keywordFlags") ~= nil end
local D
D = function(v, depth, seen)
  local tv = type(v)
  if tv == "number" then return N(v) end
  if tv == "boolean" then return v end
  if tv == "string" then
    if v == "nan" or v == "inf" or v == "-inf" or v:find("^%-?[%d%.]+[eE]?[%+%-]?%d*$") then ambiguous = ambiguous + 1 return "$str:" .. v end
    return v
  end
  if tv ~= "table" then return "<" .. tv .. ">" end
  if isMod(v) then return fmtMod(v) end
  local cn = className(v)
  if cn == "ModList" then local t = {} for i, m in ipairs(v) do t[i] = fmtMod(m) end table.sort(t) local o = obj() o["$ModList"] = t return o end
  if cn then return "<class " .. cn .. ">" end
  if depth > 6 or seen[v] then return "<deep>" end
  seen[v] = true
  local o, nk = obj(), 0
  for k, x in pairs(v) do
    if type(k) == "table" then nk = nk + 1 else o[tostring(k)] = D(x, depth + 1, seen) end
  end
  if nk > 0 then o["$tableKeys"] = N(nk) end
  seen[v] = nil
  return o
end
local function dbCount(db) local n = 0 for _, b in pairs(db.mods) do n = n + #b end return n end
local function keyCount(t) local n = 0 for _ in pairs(t) do n = n + 1 end return n end
local function chainCounts(ml) -- own mods of a ModList and of every parent up the chain
  local t, m = {}, ml
  while m do
    if className(m) == "ModDB" then t[#t + 1] = "ModDB:" .. dbCount(m) m = m.parent else t[#t + 1] = "ModList:" .. #m m = m.parent end
  end
  return t
end

local TARGET = { [1057] = "OC1", [1938] = "OC2", [1987] = "OC2", [2606] = "OC3", [4156] = "OC4", [5772] = "OC5", [6263] = "OC6" }
local function snap(line, env, actor, sk)
  local s, out = obj(), actor.output
  s.line, s.output, s.skillFlags = N(line), D(out, 0, {}), D(sk.skillFlags, 0, {})
  s.outputKeys = N(keyCount(out))
  s.mainHandKeys = out.MainHand and N(keyCount(out.MainHand)) or nil
  s.offHandKeys = out.OffHand and N(keyCount(out.OffHand)) or nil
  s.skillModList, s.skillModListChain = N(#sk.skillModList), chainCounts(sk.skillModList)
  s.playerModDB, s.enemyModDB = N(dbCount(actor.modDB)), N(dbCount(env.enemy.modDB))
  s.minionModDB = env.minion and N(dbCount(env.minion.modDB)) or nil
  s.playerConditions, s.playerMultipliers = D(actor.modDB.conditions, 1, {}), D(actor.modDB.multipliers, 1, {})
  s.mainSkillName = sk.activeEffect.grantedEffect.name
  return s
end

local function sortedKeys(t) local k = {} for u in pairs(t) do k[#k + 1] = u end table.sort(k) return k end
local function dumpCache(topEnv)
  local res, envIds, nenv = obj(), {}, 0
  for _, mode in ipairs({ "MAIN", "CALCS", "CALCULATOR" }) do
    local m, outIds = obj(), {}
    for _, u in ipairs(sortedKeys(GlobalCache.cachedData[mode])) do
      local c, o = GlobalCache.cachedData[mode][u], obj()
      for k, v in pairs(c) do if type(v) ~= "table" then o[k] = D(v, 0, {}) end end
      local e = c.Env
      if not envIds[e] then nenv = nenv + 1 envIds[e] = nenv end
      local po, sk = e.player.output, c.ActiveSkill
      o.envId, o.envMode, o.envIsTop = N(envIds[e]), e.mode, e == topEnv
      o.activeSkillName, o.activeSkillIsEnvMain = sk.activeEffect.grantedEffect.name, sk == e.player.mainSkill
      for i, x in ipairs(e.player.activeSkillList) do if x == sk then o.activeSkillIndex = N(i) end end
      o.Duration = po.Duration ~= nil and D(po.Duration, 0, {}) or nil
      o.dpsMultiplier = sk.skillData.dpsMultiplier ~= nil and D(sk.skillData.dpsMultiplier, 0, {}) or nil
      o.outputKeys = N(keyCount(po))
      if outIds[po] then o.output = { ["$sameOutputAs"] = outIds[po] } else outIds[po] = u o.output = D(po, 0, {}) end
      m[u] = o
    end
    res[mode] = m
  end
  return res
end
local function logs(topEnv)
  local st, o = W.st, obj()
  local byMode, nTop, nOff, nOffTop = {}, 0, #st.offence, 0
  for _, p in ipairs(st.offence) do if p.top and p.player then nOffTop = nOffTop + 1 end end
  for _, p in ipairs(st.perform) do byMode[p.mode] = (byMode[p.mode] or 0) + 1 if p.top then nTop = nTop + 1 end end
  o.initEnvCalls, o.performCalls, o.performTopCalls, o.performByMode = N(st.init), N(#st.perform), N(nTop), D(byMode, 0, {})
  o.offenceCalls, o.offenceTopPlayerCalls, o.offenceLog = N(nOff), N(nOffTop), D(st.offence, 0, {})
  o.performLog, o.buildActiveSkillLog, o.cacheWriteLog = D(st.perform, 0, {}), D(st.bas, 0, {}), D(st.cache, 0, {})
  return o
end
local function mainInfo(env)
  local ms, o = env.player.mainSkill, obj()
  o.infoTrigger, o.infoMessage, o.infoMessage2, o.skillPartName = ms.infoTrigger, ms.infoMessage, ms.infoMessage2, ms.skillPartName
  o.uuid, o.name = cacheSkillUUID(ms, env), ms.activeEffect.grantedEffect.name
  if ms.mirage then local mo = obj() for k, v in pairs(ms.mirage) do if type(v) ~= "table" then mo[k] = D(v, 0, {}) end end mo.outputKeys = ms.mirage.output and N(keyCount(ms.mirage.output)) or nil o.mirage = mo end
  local list = {}
  for i, sk in ipairs(env.player.activeSkillList) do
    local e = obj()
    e.index, e.name, e.uuid = N(i), sk.activeEffect.grantedEffect.name, cacheSkillUUID(sk, env)
    e.isMain, e.includeInFullDPS, e.disable = sk == ms, sk.socketGroup and sk.socketGroup.includeInFullDPS or false, sk.skillFlags and sk.skillFlags.disable or false
    list[i] = e
  end
  o.activeSkills = list
  o.mainSkillIndex = N(0)
  for i, sk in ipairs(env.player.activeSkillList) do if sk == ms then o.mainSkillIndex = N(i) end end
  return o
end
local function cacheIndex() local t = obj() for _, mode in ipairs({ "MAIN", "CALCS", "CALCULATOR" }) do t[mode] = sortedKeys(GlobalCache.cachedData[mode]) end return t end

`
const PROBE = String.raw`
local res = obj()
res.jitStatus = jit.status() and "on" or "off"
res.outputLoad, res.cacheAfterLoad = D(build.calcsTab.mainOutput, 0, {}), cacheIndex()
installBase()
-- A: fresh MAIN buildOutput after wipeGlobalCache
wipeGlobalCache(); resetLogs()
local envA = calcs.buildOutput(build, "MAIN")
res.topEnvIsReturned = envA == W.topEnv
res.outputFresh, res.mainInfo = D(envA.player.output, 0, {}), mainInfo(envA)
res.cache, res.fullDPS, res.logs = dumpCache(envA), D(W.st.fullDPS, 0, {}), logs(envA)
-- B: the same under the line hook (JIT off); OC1..OC6 of the top MAIN env's main-skill offence
wipeGlobalCache(); resetLogs()
local oc, hits, nested, nestedEnv = obj(), obj(), obj(), nil
local function hook(_, line)
  local name = TARGET[line]
  if not name then return end
  local info = debug.getinfo(2, "S")
  if not info.source:find("CalcOffence", 1, true) then return end
  local n1, e = debug.getlocal(2, 1)
  local n2, a = debug.getlocal(2, 2)
  local n3, sk = debug.getlocal(2, 3)
  if n1 ~= "env" or n2 ~= "actor" or n3 ~= "activeSkill" or a ~= e.player then return end
  if e == W.topEnv then
    hits[name] = hits[name] and (hits[name] + 1) or 1
    if not oc[name] then oc[name] = snap(line, e, a, sk) end
  elseif not nestedEnv or nestedEnv == e then -- fallback when the top env never reaches offence (mirages() returned true): first other env's player offence
    nestedEnv = e
    if not nested[name] then nested[name] = snap(line, e, a, sk) nested[name].envMode = e.mode end
  end
end
local jitWas = jit.status()
jit.off(); jit.flush()
debug.sethook(hook, "l")
local envB = calcs.buildOutput(build, "MAIN")
debug.sethook()
if jitWas then jit.on() end
res.checkpoints, res.checkpointHits, res.outputHookRun = oc, hits, D(envB.player.output, 0, {})
res.checkpointsFirstNested = nested
res.hookRunLogs = logs(envB)
-- C: the oracle's config path: wipeGlobalCache + calcsTab:BuildOutput() (MAIN then CALCS)
wipeGlobalCache(); resetLogs()
build.calcsTab:BuildOutput()
res.outputWipeBuildOutput = D(build.calcsTab.mainOutput, 0, {})
uninstallBase()
res.ambiguousStrings = N(ambiguous)
return res
`

const DUMP = DUMP_HELPERS + PROBE

// --- R7.1b: every calcs.offence call of one buildOutput(MAIN) (mode, uuid, depth, ordinal) + OC1..OC6 of each player call, perform markers ---------
const RUNS_PROBE = String.raw`
local topEnv, seq, ostack, pstack, runs, performs, ord = nil, 0, {}, {}, {}, {}, {}
local oInit2, oPerform2, oOffence2, oMir2 = calcs.initEnv, calcs.perform, calcs.offence, calcs.mirages
local function uuidOf(sk, env) local ok, u = pcall(cacheSkillUUID, sk, env) return ok and u or "<err>" end
local function install()
  calcs.initEnv = function(b, mode, ...) local env, r2, r3 = oInit2(b, mode, ...) if not topEnv and mode == "MAIN" then topEnv = env end return env, r2, r3 end
  calcs.mirages = function(env) local r = oMir2(env) local f = pstack[#pstack] if f and f.env == env then f.rec.mirages = r and true or false end return r end
  calcs.perform = function(env, skipEHP)
    local rec = { seq = #performs + 1, mode = env.mode, uuid = uuidOf(env.player.mainSkill, env), depth = #pstack, top = env == topEnv, skipEHP = skipEHP and true or false, limited = env.limitedSkills ~= nil, offenceCalled = false }
    performs[rec.seq] = rec
    pstack[#pstack + 1] = { env = env, rec = rec }
    local r = oPerform2(env, skipEHP)
    pstack[#pstack] = nil
    rec.disable = env.player.mainSkill.skillFlags.disable and true or false
    if not rec.offenceCalled then
      rec.reason = rec.mirages == true and "CalcPerform.lua:3798 calcs.mirages(env) returned true (offence skipped)" or ("offence not reached: mirages " .. tostring(rec.mirages) .. ", skill disabled " .. tostring(rec.disable))
    end
    return r
  end
  calcs.offence = function(env, actor, sk)
    local u, isPlayer = uuidOf(sk, env), actor == env.player
    local pf = pstack[#pstack]
    if isPlayer and pf and pf.env == env then pf.rec.offenceCalled = true end
    seq = seq + 1
    local key = env.mode .. "|" .. u .. "|" .. tostring(isPlayer) .. "|" .. tostring(env.limitedSkills ~= nil)
    ord[key] = (ord[key] or 0) + 1
    local rec = { seq = seq, mode = env.mode, uuid = u, depth = #pstack, top = env == topEnv, player = isPlayer, limited = env.limitedSkills ~= nil, ordinal = ord[key],
      name = sk.activeEffect.grantedEffect.name, performSeq = pf and pf.rec.seq or 0, hits = obj() }
    rec.oc = obj()
    runs[seq] = rec
    ostack[#ostack + 1] = { env = env, actor = actor, sk = sk, rec = rec }
    local r = oOffence2(env, actor, sk)
    ostack[#ostack] = nil
    return r
  end
end
local function uninstall() calcs.initEnv, calcs.perform, calcs.offence, calcs.mirages = oInit2, oPerform2, oOffence2, oMir2 end
local function hook(_, line)
  local name = TARGET[line]
  if not name then return end
  local top = ostack[#ostack]
  if not top or not top.rec.player then return end
  if not debug.getinfo(2, "S").source:find("CalcOffence", 1, true) then return end
  local n1, e = debug.getlocal(2, 1)
  local n2, a = debug.getlocal(2, 2)
  local n3, sk = debug.getlocal(2, 3)
  if n1 ~= "env" or n2 ~= "actor" or n3 ~= "activeSkill" or e ~= top.env or a ~= top.actor or sk ~= top.sk then return end
  local rec = top.rec
  rec.hits[name] = (rec.hits[name] or 0) + 1
  if not rec.oc[name] then rec.oc[name] = snap(line, e, a, sk) end
end
local function sig() local t = {} for i, r in ipairs(runs) do t[i] = r.seq .. "|" .. r.mode .. "|" .. r.uuid .. "|" .. r.depth .. "|" .. tostring(r.player) end return t end
local function strip(l) local t = {} for i, r in ipairs(l) do local c = obj() for k, v in pairs(r) do if k ~= "oc" then c[k] = D(v, 0, {}) end end t[i] = c end return t end
local res = obj()
install()
wipeGlobalCache(); topEnv, seq, ostack, pstack, runs, performs, ord = nil, 0, {}, {}, {}, {}, {}
local envA = calcs.buildOutput(build, "MAIN") -- A: plain wrapped run
res.sigA, res.outputA, res.performsA = sig(), D(envA.player.output, 0, {}), #performs
wipeGlobalCache(); topEnv, seq, ostack, pstack, runs, performs, ord = nil, 0, {}, {}, {}, {}, {}
local jitWas = jit.status()
jit.off(); jit.flush()
debug.sethook(hook, "l")
local envB = calcs.buildOutput(build, "MAIN") -- B: the same under the line hook
debug.sethook()
if jitWas then jit.on() end
uninstall()
res.sigB, res.outputB = sig(), D(envB.player.output, 0, {})
res.runs, res.performs = {}, strip(performs)
for i, r in ipairs(runs) do
  local c = obj()
  for k, v in pairs(r) do if k ~= "oc" then c[k] = D(v, 0, {}) end end
  c.oc = r.oc
  res.runs[i] = c
end
res.ambiguousStrings = N(ambiguous)
return res
`

const PERF = (runs: number): string => String.raw`
${WRAP}
jit.on()
local RUNS = ${runs}
local function now() return os.clock() * 1000 end
local a, off, full, nest, tot2, nperf = {}, {}, {}, {}, {}, {}
-- pass 1: no wrappers -- the plain buildOutput(MAIN) time
for i = 0, RUNS do
  wipeGlobalCache()
  local t0 = now()
  calcs.buildOutput(build, "MAIN")
  if i > 0 then a[i] = now() - t0 end -- run 0 = warm-up
end
-- pass 2: wrappers (offence of the top MAIN env's main skill, calcFullDPS, perform counts)
local oOff = calcs.offence
local offT, fullT
calcs.offence = function(env, actor, sk)
  if env == W.topEnv and actor == env.player then local t0 = now() local r = oOff(env, actor, sk) offT = (offT or 0) + now() - t0 return r end
  return oOff(env, actor, sk)
end
local oF2 = oFull
calcs.calcFullDPS = function(...) local t0 = now() local r = oF2(...) fullT = (fullT or 0) + now() - t0 return r end
calcs.initEnv = function(b, mode, ...) return onInit(mode, oInit(b, mode, ...)) end
local nP = 0
calcs.perform = function(env, skipEHP) nP = nP + 1 return oPerform(env, skipEHP) end
local mainOnly, nTotal = {}, {}
for i = 0, RUNS do
  wipeGlobalCache(); resetLogs(); offT, fullT, nP = nil, nil, 0
  local t0 = now()
  calcs.buildOutput(build, "MAIN")
  local t = now() - t0
  if i > 0 then tot2[i], off[i], full[i], nest[i] = t, offT or 0, fullT or 0, nP - 1 end
end
calcs.offence, calcs.initEnv, calcs.perform, calcs.calcFullDPS = oOff, oInit, oPerform, oFull
return { jit = jit.version, status = jit.status() and "on" or "off", buildOutput_ms = a, buildOutputWrapped_ms = tot2, offenceMain_ms = off, calcFullDPS_ms = full, nestedPerform = nest }
`

// --- deterministic JSON -----------------------------------------------------------------------------------------------------------------
const normAddr = (s: string): string => s.replace(/(function|table): 0x[0-9a-f]+/g, '$1: <addr>')
function norm(v: any): any { // eslint-disable-line
  if (typeof v === 'string') return normAddr(v)
  if (Array.isArray(v)) return v.map(norm)
  if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [normAddr(k), norm(x)]))
  return v
}
function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
const median = (xs: number[]): number => { const s = [...xs].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]! }
// key-level diff of two dumped output tables (objects keyed by tostring(key)); values that differ are kept (shortened when huge)
function diffOutputs(a: Rec, b: Rec, aName: string, bName: string): Rec {
  const keys = [...new Set([...Object.keys(a), ...Object.keys(b)])].sort(byteOrder)
  const short = (x: any): any => { const s = ser(x); return s.length > 400 ? { sha256: sha(s), len: s.length } : x } // eslint-disable-line
  const diff: Rec[] = []
  const onlyA: string[] = [], onlyB: string[] = []
  for (const k of keys) {
    if (!(k in a)) onlyB.push(k)
    else if (!(k in b)) onlyA.push(k)
    else if (ser(a[k]) !== ser(b[k])) diff.push({ key: k, [aName]: short(a[k]), [bName]: short(b[k]) })
  }
  return { equal: diff.length === 0 && !onlyA.length && !onlyB.length, keysA: Object.keys(a).length, keysB: Object.keys(b).length, [`onlyIn_${aName}`]: onlyA, [`onlyIn_${bName}`]: onlyB, differing: diff }
}

async function fixtures(): Promise<void> {
  const luts = await prepareLuts()
  let digest = ''
  try { digest = execFileSync('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', LUAJIT_IMAGE], { encoding: 'utf8' }).trim() } catch { /* tag only */ }
  const lutSha = sha(Buffer.concat(readdirSync(luts).sort().map((f) => Buffer.concat([Buffer.from(f), readFileSync(resolve(luts, f))]))))
  const producer = { generator: 'tools/lua-corpus/export-offence.ts', image: digest || LUAJIT_IMAGE, pobSha: git(roots.pobRoot), dataManifestSha256: sha(readFileSync(resolve(ROOT, 'data/manifest.json'))),
    lutSha, libm: 'docker = Alpine/musl (MINEFIELD-RUST R2)', ocLines: 'CalcOffence.lua OC1=1057 OC2=1938 OC3=2606 OC4=4156 OC5=5772 OC6=6263 (end of calcs.offence)' }
  mkdirSync(FIX, { recursive: true })
  const report: Rec = {}
  for (const b of GOLDEN_BUILDS) {
    const xml = readFileSync(b.xmlPath(roots))
    const r = norm(ljEval(loadChunk(xml, b.name) + `local function probe()\n${WRAP}\n${DUMP}\nend\nreturn probe()\n`, luts))
    const slug = b.name.replace(/ /g, '-')
    const cmp = {
      loadVsFresh: diffOutputs(r.outputLoad, r.outputFresh, 'load', 'fresh'),
      hookRunVsFresh: diffOutputs(r.outputHookRun, r.outputFresh, 'hook', 'fresh'),
      wipeBuildOutputVsFresh: diffOutputs(r.outputWipeBuildOutput, r.outputFresh, 'wipeBuildOutput', 'fresh'),
    }
    const { outputLoad, outputHookRun, outputWipeBuildOutput, jitStatus, checkpoints, cache, checkpointsFirstNested, ...rest } = r
    const topComplete = ['OC1', 'OC2', 'OC3', 'OC4', 'OC5', 'OC6'].every((k) => checkpoints[k])
    if (!topComplete) rest.checkpointsFirstNested = checkpointsFirstNested
    const head = { format: 1, producer: { ...producer, jitStatusAtStart: jitStatus }, build: b.name, xmlSha256: sha(xml) }
    const body = { ...rest, comparisons: cmp }
    const text = (o: Rec): string => ser(o) + '\n'
    const whole = text({ ...head, ...body, checkpoints, cache })
    const files: { file: string; bytes: number }[] = []
    const put = (name: string, o: Rec): void => {
      const t = text(o)
      if (Buffer.byteLength(t) > MAX_BYTES) throw new Error(`${name}: ${Buffer.byteLength(t)} bytes > ${MAX_BYTES} -- STOP (split further, card allows parts <= 1.5 MB)`)
      const file = resolve(FIX, name)
      writeFileSync(file, t, 'utf8')
      files.push({ file: file.replace(`${ROOT}\\`, ''), bytes: statSync(file).size })
    }
    if (Buffer.byteLength(whole) <= MAX_BYTES) put(`${slug}.offence.json`, { ...head, ...body, checkpoints, cache })
    else {
      // units = checkpoints and cache entries; greedily packed into parts (the main file keeps everything else + the part index)
      const units: { path: string[]; v: any }[] = [] // eslint-disable-line
      for (const k of Object.keys(checkpoints).sort(byteOrder)) units.push({ path: ['checkpoints', k], v: checkpoints[k] })
      for (const m of Object.keys(cache).sort(byteOrder)) for (const u of Object.keys(cache[m]).sort(byteOrder)) units.push({ path: ['cache', m, u], v: cache[m][u] })
      const parts: Rec[] = [{}]
      let size = 0
      for (const u of units) {
        const sz = ser(u.v).length + 60
        if (size + sz > MAX_BYTES - 2000 && size > 0) { parts.push({}); size = 0 }
        size += sz
        let cur = parts[parts.length - 1]!
        for (const p of u.path.slice(0, -1)) cur = (cur[p] ??= {})
        cur[u.path[u.path.length - 1]!] = u.v
      }
      put(`${slug}.offence.json`, { ...head, ...body, parts: parts.map((_, i) => `${slug}.offence-${i + 1}.json`) })
      parts.forEach((p, i) => put(`${slug}.offence-${i + 1}.json`, { ...head, part: i + 1, ...p }))
    }
    const keyCount = (o: Rec): number => Object.keys(o).length
    const perf = r.logs
    report[b.name] = {
      outputKeys: keyCount(r.outputFresh), mainSkillIndex: r.mainInfo.mainSkillIndex, mainSkill: r.mainInfo.name, activeSkills: r.mainInfo.activeSkills.length,
      ocLines_keys: Object.fromEntries(Object.keys(checkpoints).sort().map((k) => [k, `${checkpoints[k].line}: ${checkpoints[k].outputKeys} keys, hits ${r.checkpointHits[k]}`])),
      ocNestedFallback: topComplete ? undefined : Object.fromEntries(Object.keys(checkpointsFirstNested).sort().map((k) => [k, `${checkpointsFirstNested[k].line}: ${checkpointsFirstNested[k].outputKeys} keys, env ${checkpointsFirstNested[k].envMode}`])),
      offenceCalls: perf.offenceCalls, offenceTopPlayerCalls: perf.offenceTopPlayerCalls,
      ocMissing: ['OC1', 'OC2', 'OC3', 'OC4', 'OC5', 'OC6'].filter((k) => !checkpoints[k]),
      cacheUuids: Object.fromEntries(Object.keys(cache).map((m) => [m, Object.keys(cache[m]).length])),
      cacheAfterLoad: Object.fromEntries(Object.keys(r.cacheAfterLoad).map((m) => [m, r.cacheAfterLoad[m].length])),
      distinctEnvsInCache: new Set(Object.values(cache).flatMap((m: any) => Object.values(m).map((e: any) => e.envId))).size, // eslint-disable-line
      performCalls: perf.performCalls, performTop: perf.performTopCalls, nestedPerform: Number(perf.performCalls) - Number(perf.performTopCalls), performByMode: perf.performByMode,
      buildActiveSkillCalls: Object.keys(perf.buildActiveSkillLog).length, cacheWrites: Object.keys(perf.cacheWriteLog).length, initEnvCalls: perf.initEnvCalls,
      fullDPS_skills: Object.keys(r.fullDPS.skills ?? {}).length, FullDPS: r.outputFresh.FullDPS,
      loadVsFresh: { equal: cmp.loadVsFresh.equal, keysLoad: cmp.loadVsFresh.keysA, keysFresh: cmp.loadVsFresh.keysB, onlyLoad: cmp.loadVsFresh.onlyIn_load, onlyFresh: cmp.loadVsFresh.onlyIn_fresh, differing: cmp.loadVsFresh.differing.map((d: Rec) => d.key) },
      hookRunEqualsFresh: cmp.hookRunVsFresh.equal, wipeBuildOutputEqualsFresh: cmp.wipeBuildOutputVsFresh.equal, ambiguousStrings: r.ambiguousStrings, topEnvIsReturned: r.topEnvIsReturned, jitStatusAtStart: jitStatus, files }
    console.error(`${b.name}: done`)
  }
  console.log(JSON.stringify(report, null, 1))
}

async function perf(): Promise<void> {
  const luts = await prepareLuts()
  const RUNS = 7
  const out: Rec = { runs: RUNS, note: 'LuaJIT docker, jit.on(), os.clock CPU ms, wipeGlobalCache before every run, run 0 = warm-up (dropped). buildOutput_ms = plain calcs.buildOutput(build,"MAIN") (pass 1, no wrappers); the other series are pass 2 with wrappers: offenceMain_ms = calcs.offence of the top MAIN env main skill, calcFullDPS_ms = calcs.calcFullDPS, nestedPerform = calcs.perform calls minus the top one', builds: {} }
  for (const b of GOLDEN_BUILDS) {
    const r = ljEval(loadChunk(readFileSync(b.xmlPath(roots)), b.name) + `local function probe()\n${PERF(RUNS)}\nend\nreturn probe()\n`, luts)
    const s = (xs: number[]) => ({ median: median(xs), min: Math.min(...xs) })
    out.builds[b.name] = { luajit: r.jit, jit: r.status, buildOutput: s(r.buildOutput_ms), buildOutputWrapped: s(r.buildOutputWrapped_ms), offenceMain: s(r.offenceMain_ms), calcFullDPS: s(r.calcFullDPS_ms),
      nestedPerform: r.nestedPerform, series: { buildOutput_ms: r.buildOutput_ms, buildOutputWrapped_ms: r.buildOutputWrapped_ms, offenceMain_ms: r.offenceMain_ms, calcFullDPS_ms: r.calcFullDPS_ms } }
    console.error(`perf ${b.name}: buildOutput ${median(r.buildOutput_ms).toFixed(1)} ms, offence(main) ${median(r.offenceMain_ms).toFixed(1)} ms, calcFullDPS ${median(r.calcFullDPS_ms).toFixed(1)} ms, nested perform ${r.nestedPerform[0]}`)
  }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-offence.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(Object.fromEntries(Object.entries(out.builds).map(([k, v]: [string, any]) => [k, { buildOutput: v.buildOutput, offenceMain: v.offenceMain, calcFullDPS: v.calcFullDPS, nestedPerform: v.nestedPerform[0] }])), null, 1)) // eslint-disable-line
}


// R7.1b: `node tools/lua-corpus/export-offence.ts runs` -> crates/pob-calc/tests/fixtures/<build>.offence-runs.json (+ -<n> parts with the OC snapshots)
async function runsMode(): Promise<void> {
  const luts = await prepareLuts()
  const report: Rec = {}
  for (const b of GOLDEN_BUILDS) {
    const xml = readFileSync(b.xmlPath(roots))
    const r = norm(ljEval(loadChunk(xml, b.name) + `local function probe()\n${WRAP}\n${DUMP_HELPERS}\n${RUNS_PROBE}\nend\nreturn probe()\n`, luts))
    const slug = b.name.replace(/ /g, '-')
    const OCS = ['OC1', 'OC2', 'OC3', 'OC4', 'OC5', 'OC6']
    const runs: Rec[] = r.runs
    const text = (o: Rec): string => ser(o) + '\n'
    const outEq = diffOutputs(r.outputA, r.outputB, 'plain', 'hook')
    const sigEqual = ser(r.sigA) === ser(r.sigB)
    // parts: units = (run seq, OCn) packed greedily; the main file keeps the headers, markers and the part index
    const parts: Rec[] = [{}]
    let size = 0
    for (const run of runs) for (const k of OCS) {
      const v = run.oc[k]
      if (!v) continue
      const sz = ser(v).length + 40
      if (size + sz > MAX_BYTES - 2000 && size > 0) { parts.push({}); size = 0 }
      size += sz
      const cur = parts[parts.length - 1]!
      ;(cur[run.seq] ??= {})[k] = v
    }
    const head = { format: 1, producer: { generator: 'tools/lua-corpus/export-offence.ts runs', image: LUAJIT_IMAGE, pobSha: git(roots.pobRoot), dataManifestSha256: sha(readFileSync(resolve(ROOT, 'data/manifest.json'))),
      libm: 'docker = Alpine/musl (MINEFIELD-RUST R2)', ocLines: 'CalcOffence.lua OC1=1057 OC2=1938 OC3=2606 OC4=4156 OC5=5772 OC6=6263' }, build: b.name, xmlSha256: sha(xml) }
    const headers = runs.map((x) => { const { oc, ...h } = x; return { ...h, ocPresent: OCS.filter((k) => oc[k]) } })
    const names = parts.map((_, i) => `${slug}.offence-runs-${i + 1}.json`)
    const files: { file: string; bytes: number }[] = []
    const put = (name: string, o: Rec): void => {
      const t = text(o)
      if (Buffer.byteLength(t) > MAX_BYTES) throw new Error(`${name}: ${Buffer.byteLength(t)} bytes > ${MAX_BYTES} -- STOP`)
      const file = resolve(FIX, name)
      writeFileSync(file, t, 'utf8')
      files.push({ file: file.replace(`${ROOT}\\`, ''), bytes: statSync(file).size })
    }
    put(`${slug}.offence-runs.json`, { ...head, runs: headers, performs: r.performs, parts: names, consistency: { plainEqualsHookSignature: sigEqual, plainEqualsHookOutput: outEq.equal, runsPlain: r.sigA.length, runsHook: r.sigB.length } })
    parts.forEach((p, i) => put(names[i]!, { ...head, part: i + 1, runs: p }))
    const pl = runs.filter((x) => x.player)
    report[b.name] = {
      offenceCalls: runs.length, playerCalls: pl.length, minionCalls: runs.length - pl.length, topPlayerCalls: pl.filter((x) => x.top).length, limitedPlayerCalls: pl.filter((x) => x.limited).length,
      maxDepth: Math.max(...runs.map((x) => Number(x.depth))), distinctKeys: new Set(pl.map((x) => `${x.mode}|${x.uuid}|${x.limited}`)).size,
      ocComplete: pl.filter((x) => OCS.every((k) => x.oc[k])).length, ocIncomplete: pl.filter((x) => !OCS.every((k) => x.oc[k])).map((x) => `seq ${x.seq} ${x.name}: missing ${OCS.filter((k) => !x.oc[k]).join(',')}`),
      performs: r.performs.length, performsNoOffence: r.performs.filter((x: Rec) => !x.offenceCalled).map((x: Rec) => `perform ${x.seq} ${x.mode} depth ${x.depth} top ${x.top} ${x.uuid}: ${x.reason}`),
      plainEqualsHookSignature: sigEqual, plainEqualsHookOutput: outEq.equal, ambiguousStrings: r.ambiguousStrings, files }
    console.error(`${b.name}: done (${runs.length} offence calls)`)
  }
  console.log(JSON.stringify(report, null, 1))
}

if (process.argv[2] === 'runs') await runsMode()
else if (process.argv[2] === 'perf') await perf()
else await fixtures()
