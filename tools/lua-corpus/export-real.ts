// R8-real: LuaJIT truth for the user's own PoB builds (`builds/pc/`, a local copy -- read only; personal, kept out of git).
// Per build: the file as desktop reads it (text mode, CRLF -> LF, MINEFIELD-RUST R7) -> loadBuildFromXML -> wipeGlobalCache ->
// calcs.buildOutput(build, "MAIN") -> the player's output + the minion's output + mainSkill info, in the encoding of export-offence-synth.ts
// (`output.{nums,strs,bools}` = every scalar, numbers `%.17g`; `output.tables` = structural dumps). Load notes: ConPrintf lines printed during
// the load, gems the load could not resolve (`errMsg` / no `gemData`), the number of items, `promptMsg`.
// A build that does not load (or whose buildOutput raises) is recorded with `loadError` / `calcError` -- the Rust gate reports it, not gates it.
// Variant `p5stripped` (when it changes the file): the build without the <Party> section (the P5 module pob-calc does not port; tattoo
// overrides are ported since R-tattoo and stay), its XML written next to the fixture. R13: a build with an old-format gem whose variant LuaJIT resolves by `pairs` over string keys is
// run until the process that took Rust's order is seen (`r13` in the fixture). Two passes; a job whose passes differ is reported, no fixture.
// Fixtures: builds/pc-fixtures/<league>__<file>[.p5stripped].luajit.json (gitignored -- they carry the user's build data).
// Run from the pob-rust root (Node 24, docker):
//   node tools/lua-corpus/export-real.ts [--set pc|user|corpus] [regex]  -> fixtures for every build of the set whose relative path matches
//   node tools/lua-corpus/export-real.ts perf [re] -> builds/pc-fixtures/_perf-luajit.json: buildOutput(MAIN) ms (default: 3.29)
//   node tools/lua-corpus/export-real.ts tree-save [re] -> <slug>.tree-save.json for every build of the set with tattoos (`<Override `): LuaJIT's
//     `build.treeTab:Save` composed by xml.lua ComposeXML after the load (PassiveSpec.lua:197-265, R-tattoo; gate crates/pob-engine/tests/tattoo_save.rs)
//   node tools/lua-corpus/export-real.ts perf-load [re] -> <out>/_perf-load-luajit.json: loadBuildFromXML and recalc ms, with and without <Party> (R-party)
//   `--set party-synth`: the XMLs of gen-party-synth.ts, fixtures next to them (crates/pob-calc/tests/fixtures/party-synth)
//   `--out <dir>` (any mode): write there instead of builds/<set>-fixtures (a re-take into a folder of one's own).
// Sets (R-UP, builds/README.md): pc = builds/pc/**/*.xml -> builds/pc-fixtures; user = builds/user/*.{xml,txt} (txt = a build code) ->
// builds/user-fixtures; corpus = builds/corpus/manifest.json -> builds/corpus-fixtures. All three fixture dirs are gitignored.
// Sampling rule for an update (UPDATE-PROMPT.md): pc -- every build of the newest league + 2-3 per older league/folder.
import { POB as UPSTREAM_POB, ORACLE as UPSTREAM_ORACLE } from '../upstream.ts'
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, relative, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { promisify } from 'node:util'
import { inflateSync } from 'node:zlib'
const run = promisify(execFile)
type Rec = Record<string, any> // eslint-disable-line
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const SET = (() => {
  const i = process.argv.indexOf('--set')
  if (i < 0) return 'pc'
  const s = process.argv[i + 1] ?? ''
  process.argv.splice(i, 2)
  return s
})()
if (!['pc', 'user', 'corpus', 'party-synth'].includes(SET)) throw new Error(`--set ${SET}: expected pc | user | corpus | party-synth`)
const SYNTH_DIR = resolve(ROOT, 'crates/pob-calc/tests/fixtures/party-synth') // R-party: XMLs of tools/lua-corpus/gen-party-synth.ts, fixtures next to them
const PC = SET === 'corpus' ? ROOT : SET === 'party-synth' ? SYNTH_DIR : resolve(ROOT, `builds/${SET}`)
const OUT = (() => {
  const i = process.argv.indexOf('--out')
  if (i < 0) return SET === 'party-synth' ? SYNTH_DIR : resolve(ROOT, `builds/${SET}-fixtures`)
  const d = resolve(process.argv[i + 1] ?? '')
  process.argv.splice(i, 2)
  return d
})()
const POB = UPSTREAM_POB
const POBTS = 'C:/pob-work/pob-ts'
const ORACLE = UPSTREAM_ORACLE
const IMAGE = 'ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest'
const JOBS = Number(process.env.REAL_JOBS ?? 5)
const TIMEOUT_MS = Number(process.env.REAL_TIMEOUT_S ?? 60) * 1000 // per docker run: a broken build that loads for minutes is reported as 	imeout, not waited for
const LUTS = resolve(process.env.REAL_LUTS ?? resolve(tmpdir(), 'pob-real-luts'))
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const median = (xs: number[]): number => { const s = [...xs].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]! }

// --- builds ------------------------------------------------------------------------------------------------------------------------
function walk(dir: string): string[] {
  return readdirSync(dir).sort(byteOrder).flatMap((n) => { const p = resolve(dir, n); return statSync(p).isDirectory() ? walk(p) : n.endsWith('.xml') ? [p] : [] })
}
const relOf = (p: string): string => CORPUS_REL.get(p) ?? relative(PC, p).replace(/\\/g, '/')
/** corpus: absolute path -> its manifest path (`$POB/...`, `$ORACLE/...` or repo-relative) and set id */
const CORPUS_REL = new Map<string, string>()
const CORPUS_SET = new Map<string, string>()
const globRe = (glob: string): RegExp => new RegExp('^' + glob.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '$')
/** the build files of SET (absolute paths) */
function listBuilds(): string[] {
  if (SET === 'pc' || SET === 'party-synth') return walk(PC)
  if (SET === 'user') return readdirSync(PC).sort(byteOrder).filter((n) => /\.(xml|txt)$/i.test(n)).map((n) => resolve(PC, n))
  const m = JSON.parse(readFileSync(resolve(ROOT, 'builds/corpus/manifest.json'), 'utf8')) as { sets: { id: string; paths: string[] }[] }
  const out: string[] = []
  for (const s of m.sets) {
    for (const pat of s.paths) {
      const exp = pat.replace(/^\$POB\//, `${POB}/`).replace(/^\$ORACLE\//, `${ORACLE}/`)
      const abs = resolve(ROOT, exp)
      const dir = dirname(abs)
      const base = abs.slice(dir.length + 1)
      const files = base.includes('*') ? readdirSync(dir).sort(byteOrder).filter((n) => globRe(base).test(n)).map((n) => resolve(dir, n)) : [abs]
      for (const f of files) {
        CORPUS_REL.set(f, base.includes('*') ? pat.slice(0, pat.lastIndexOf('/') + 1) + f.slice(dir.length + 1) : pat)
        CORPUS_SET.set(f, s.id)
        out.push(f)
      }
    }
  }
  return out
}
/** the fixture slug of a build file of SET */
const slugFor = (f: string): string =>
  SET === 'party-synth'
    ? relOf(f).replace(/\.xml$/, '')
    : SET === 'pc'
    ? slugOf(relOf(f))
    : SET === 'user'
      ? `user__${relOf(f).replace(/\.(xml|txt)$/i, '')}`
      : `${CORPUS_SET.get(f)}__${relOf(f).replace(/^.*\//, '').replace(/\.xml$/, '')}`
/** fixture slug: `<league>__<file stem>`; the top-level files of builds/pc are the `Builds` folder of PoB. */
export const slugOf = (rel: string): string => (rel.includes('/') ? rel : `Builds/${rel}`).replace(/\.xml$/, '').replace(/\//g, '__')
/** desktop text mode (Build.lua:2267 io.open(path, "r")): CRLF -> LF, bytes kept */
const desktopBytes = (p: string): Buffer =>
  /\.txt$/i.test(p) ? decodeCode(p) : Buffer.from(readFileSync(p).toString('latin1').replace(/\r\n/g, '\n'), 'latin1')
/** a build code (pobb.in / "Generate"; ImportTab.lua: base64 with `-_`, zlib) -> the XML ImportTab hands to loadBuildFromXML; a first line
 *  that is not a code (a note, builds/README.md) is skipped */
function decodeCode(p: string): Buffer {
  const code = readFileSync(p, 'utf8').split(/\r?\n/).map((l) => l.trim()).find((l) => /^[A-Za-z0-9_\-+/=]{20,}$/.test(l))
  if (!code) throw new Error(`${p}: no build code line`)
  return inflateSync(Buffer.from(code.replace(/-/g, '+').replace(/_/g, '/'), 'base64'))
}
// R13 (MINEFIELD-RUST R13): SkillsTab.lua:472-479 resolves an old-format <Gem> (gemId + skillId, no variantId) by `pairs(possibleVariants)` --
// string keys, the order differs per process. Rust takes the data order (gems.json = insertion order, base variant first). `rustR13Key` = the
// ids Rust resolves for the gems of the build whose gameId has > 1 variant granting the same effect (the Lua probe `r13Key` in DUMP).
const gemsData: Rec = JSON.parse(readFileSync(resolve(ROOT, 'data/gems.json'), 'utf8')).gems
const byGameId = new Map<string, [string, Rec][]>()
for (const [id, g] of Object.entries(gemsData)) { const l = byGameId.get(g.gameId) ?? []; l.push([id, g]); byGameId.set(g.gameId, l) }
function rustR13Key(xml: Buffer): { key: string; ambiguous: number } {
  const ids: string[] = []
  let ambiguous = 0
  for (const m of xml.toString('latin1').matchAll(/<Gem\s([^>]*)>/g)) {
    const attr = (n: string): string | undefined => new RegExp(`(?:^|\\s)${n}="([^"]*)"`).exec(m[1]!)?.[1]
    const gameId = attr('gemId'), variantId = attr('variantId'), skillId = attr('skillId')
    const variants = gameId ? byGameId.get(gameId) : undefined
    if (!variants) continue
    const hit = variantId !== undefined ? variants.find(([, g]) => g.variantId === variantId) : skillId !== undefined ? variants.find(([, g]) => g.grantedEffectId === skillId) : undefined
    if (hit && variants.filter(([, g]) => g.grantedEffectId === hit[1].grantedEffectId).length > 1) { ids.push(hit[0]); if (variantId === undefined) ambiguous++ }
  }
  return { key: ids.sort(byteOrder).join('|'), ambiguous }
}
const R13_TRIES = 30
/** the build without the inputs of the P5 module pob-calc does not port: the party tab (`<Party>`); tattoos are ported (R-tattoo) */
const stripP5 = (xml: Buffer): Buffer => Buffer.from(xml.toString('latin1').replace(/\n?[ \t]*<Party[\s>][\s\S]*?<\/Party>/g, ''), 'latin1')

// --- docker LuaJIT (the ljEval of export-offence-synth.ts, LUT dir of this probe) ---------------------------------------------------
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
async function ljEval(code: string): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-real-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), JEWEL_SHIM + code)
    const args = ['run', '--rm', '-v', `${POB}:/workdir:ro`, '-v', `${POBTS}/tools/oracle/lua:/bench:ro`, '-v', `${tmp}:/code:ro`, '-v', `${LUTS}:/jewel-data:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    let out: string
    const name = `pob-real-${process.pid}-${Math.random().toString(36).slice(2)}`
    args.splice(2, 0, '--name', name)
    try { out = (await run('docker', args, { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024, timeout: TIMEOUT_MS })).stdout } catch (e) {
      if ((e as Rec).killed || (e as Rec).signal) { await run('docker', ['rm', '-f', name]).catch(() => undefined); return { timeout: `timeout: LuaJIT load + buildOutput > ${TIMEOUT_MS / 1000} s` } }
      throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
    return JSON.parse(out.trim().split('\n').pop()!).result
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}
async function prepareLuts(files: string[]): Promise<void> {
  mkdirSync(LUTS, { recursive: true })
  const { findRequiredJewelLuts } = await import(pathToFileURL(resolve(ORACLE, 'src/timelessJewelLuts.mjs')).href)
  const { unpackJewelData } = await import(pathToFileURL(resolve(ORACLE, 'src/nodeJewelData.mjs')).href)
  const stems = new Set<string>(files.flatMap((f) => findRequiredJewelLuts(desktopBytes(f).toString('utf8')).map((x: Rec) => x.fileStem as string)))
  for (const s of stems) if (!existsSync(resolve(LUTS, `${s}.bin`))) writeFileSync(resolve(LUTS, `${s}.bin`), unpackJewelData(POB, s).bytes)
  console.error(`LUTs (${LUTS}): ${[...stems].sort(byteOrder).join(', ') || 'none'}`)
}

// --- Lua -------------------------------------------------------------------------------------------------------------------------------
// load with notes; `__REAL_LOAD__` = { ok, notes, error }
const loadChunk = (xml: Buffer, name: string): string => String.raw`
local NOTES = {}
local rawConPrintf = ConPrintf
ConPrintf = function(fmt, ...) local ok, s = pcall(string.format, fmt, ...) NOTES[#NOTES + 1] = "ConPrintf: " .. (ok and s or tostring(fmt)) end
local okLoad, errLoad = pcall(loadBuildFromXML, ${luaBytes(xml)}, ${JSON.stringify(name)})
ConPrintf = rawConPrintf
local LOAD = { notes = NOTES }
if not okLoad then LOAD.error = "loadBuildFromXML raised: " .. tostring(errLoad)
elseif __mainObject__.promptMsg then LOAD.error = "promptMsg: " .. tostring(__mainObject__.promptMsg)
elseif not build or not build.calcsTab then LOAD.error = "no build after load" end
if not LOAD.error then
  for gi, group in ipairs(build.skillsTab.socketGroupList) do
    for i, gem in ipairs(group.gemList) do
      if gem.errMsg then NOTES[#NOTES + 1] = string.format("gem %d.%d %q: %s", gi, i, tostring(gem.nameSpec), tostring(gem.errMsg))
      elseif not gem.gemData and not gem.grantedEffect then NOTES[#NOTES + 1] = string.format("gem %d.%d %q: unresolved (no gemData)", gi, i, tostring(gem.nameSpec)) end
    end
  end
  local n = 0 for _ in pairs(build.itemsTab.items) do n = n + 1 end
  LOAD.items = n
end
`
// export-offence-synth.ts DUMP (identical encoding), behind the load result; buildOutput errors are caught
const DUMP = String.raw`
if LOAD.error then return { loadError = LOAD.error, loadNotes = LOAD.notes } end
local RAWF = string.format
for i = 1, 20 do local n, v = debug.getupvalue(string.format, i); if not n then break end; if n == "s_format" then RAWF = v break end end
local OBJ = { __jsontype = "object" }
local function obj() return setmetatable({}, OBJ) end
local N = function(v) return RAWF("%.17g", v) end
local fmtMod = modLib.formatSourceMod
local function className(t) local mt = getmetatable(t) if type(mt) == "table" then return rawget(mt, "_className") end end
local function isMod(t) return rawget(t, "name") ~= nil and rawget(t, "type") ~= nil and rawget(t, "flags") ~= nil and rawget(t, "keywordFlags") ~= nil end
local D
D = function(v, depth, seen)
  local tv = type(v)
  if tv == "number" then return N(v) end
  if tv == "boolean" then return v end
  if tv == "string" then
    if v == "nan" or v == "inf" or v == "-inf" or v:find("^%-?[%d%.]+[eE]?[%+%-]?%d*$") then return "$str:" .. v end
    return v
  end
  if tv ~= "table" then return "<" .. tv .. ">" end
  if isMod(v) then return fmtMod(v) end
  local cn = className(v)
  if cn == "ModList" then local t = {} for i, m in ipairs(v) do t[i] = fmtMod(m) end table.sort(t) local o = obj() o["$ModList"] = t return o end
  if cn then return "<class " .. cn .. ">" end
  if depth > 8 or seen[v] then return "<deep>" end
  seen[v] = true
  local o, nk = obj(), 0
  for k, x in pairs(v) do if type(k) == "table" then nk = nk + 1 else o[tostring(k)] = D(x, depth + 1, seen) end end
  if nk > 0 then o["$tableKeys"] = N(nk) end
  seen[v] = nil
  return o
end
local function split(out)
  local r = { nums = obj(), strs = obj(), bools = obj(), tables = obj() }
  for k, v in pairs(out) do
    local t = type(v)
    if t == "number" then r.nums[k] = N(v) elseif t == "string" then r.strs[k] = v elseif t == "boolean" then r.bools[k] = v elseif t == "table" then r.tables[tostring(k)] = D(v, 0, {}) end
  end
  return r
end
wipeGlobalCache()
local okCalc, env = pcall(build.calcsTab.calcs.buildOutput, build, "MAIN")
if not okCalc then return { calcError = tostring(env), loadNotes = LOAD.notes, items = LOAD.items } end
local ms, res = env.player.mainSkill, obj()
res.loadNotes, res.items = LOAD.notes, LOAD.items
res.output = split(env.player.output)
res.minion = env.minion and split(env.minion.output) or nil
res.minionName = env.minion and env.minion.minionData and env.minion.minionData.name or nil
local info = obj()
if ms then
  info.name, info.uuid = ms.activeEffect.grantedEffect.name, cacheSkillUUID(ms, env)
  info.skillPartName = ms.skillPartName
  info.disable, info.hasMinion, info.hasMirage = ms.skillFlags.disable or false, ms.minion ~= nil, ms.mirage ~= nil
end
info.activeSkills = #env.player.activeSkillList
info.treeVersion = build.spec.treeVersion
info.targetVersion = build.targetVersion
res.mainSkill = info
-- R13 probe (MINEFIELD-RUST R13, SkillsTab.lua:474 pairs(possibleVariants)): the resolved ids of the gems whose gameId has more than one
-- variant granting the same effect -- which variant this process took
local ids = {}
for _, set in pairs(build.skillsTab.skillSets) do for _, g in ipairs(set.socketGroupList) do for _, gem in ipairs(g.gemList) do
  local gd = gem.gemData
  if gd and gd.gameId then
    local n = 0
    for _, v in pairs(build.data.gemsByGameId[gd.gameId] or {}) do if v.grantedEffectId == gd.grantedEffectId then n = n + 1 end end
    if n > 1 then ids[#ids + 1] = gd.id end
  end
end end end
table.sort(ids)
res.r13Key = table.concat(ids, "|")
return res
`
const PERF = (runs: number): string => String.raw`
if LOAD.error then return { loadError = LOAD.error } end
jit.on()
local R = {}
for i = 0, ${runs} do
  wipeGlobalCache()
  local t0 = os.clock()
  build.calcsTab.calcs.buildOutput(build, "MAIN")
  local t1 = os.clock()
  if i > 0 then R[i] = (t1 - t0) * 1000 end
end
return { ms = R, jit = jit.version }
`

// --- deterministic JSON ----------------------------------------------------------------------------------------------------------------
function ser(v: unknown): string {
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}
const norm = (s: string): string => s.replace(/(function|table): 0x[0-9a-f]+/g, '$1: <addr>')
async function pool<T, R>(items: T[], f: (x: T) => Promise<R>): Promise<R[]> {
  const res: R[] = new Array(items.length)
  let next = 0
  await Promise.all(Array.from({ length: JOBS }, async () => {
    for (;;) { const i = next++; if (i >= items.length) return; res[i] = await f(items[i]!); console.error(`  done ${i + 1}/${items.length}`) }
  }))
  return res
}
/** the keys whose values differ between two dumps (pass 1 vs pass 2) */
function diffKeys(a: Rec, b: Rec): string[] {
  const out: string[] = []
  for (const part of ['output', 'minion']) for (const t of ['nums', 'strs', 'bools', 'tables']) {
    const x = (a[part]?.[t] ?? {}) as Rec, y = (b[part]?.[t] ?? {}) as Rec
    for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) if (norm(ser(x[k] ?? null)) !== norm(ser(y[k] ?? null))) out.push(`${part}.${t}.${k}`)
  }
  for (const k of ['timeout', 'loadError', 'calcError', 'r13Error', 'loadNotes', 'mainSkill']) if (norm(ser(a[k] ?? null)) !== norm(ser(b[k] ?? null))) out.push(k)
  return out.sort(byteOrder)
}

async function fixtures(filter?: RegExp): Promise<void> {
  const files = listBuilds().filter((f) => !filter || filter.test(relOf(f)))
  if (!files.length) throw new Error(`no builds of set ${SET} match ${filter}`)
  await prepareLuts(files)
  mkdirSync(OUT, { recursive: true })
  const image = (await run('docker', ['image', 'inspect', '--format', '{{index .RepoDigests 0}}', IMAGE]).catch(() => ({ stdout: IMAGE }))).stdout.trim()
  const pobSha = (await run('git', ['-C', POB, 'rev-parse', '--short', 'HEAD'])).stdout.trim()
  // jobs: every build as is, plus -- when it carries one -- the build with the inputs of the unported P5 module removed (variant
  // `p5stripped`: the <Party> section, PartyTab), written next to the fixture: the gate on
  // everything else the build exercises.
  type Job = { rel: string; slug: string; xml: Buffer; variant?: string; xmlFile?: string }
  const jobs: Job[] = files.flatMap((f) => {
    const rel = relOf(f), slug = slugFor(f), xml = desktopBytes(f)
    const s = stripP5(xml)
    return s.equals(xml) || SET === 'party-synth' ? [{ rel, slug, xml }] : [{ rel, slug, xml }, { rel, slug: `${slug}.p5stripped`, xml: s, variant: 'p5stripped', xmlFile: `${slug}.p5stripped.xml` }]
  })
  // one LuaJIT result per job; a build with an R13-ambiguous gem: repeated until the run that took Rust's order is seen (and, for the record,
  // another order -- max R13_TRIES runs); the fixture is the run of Rust's order, `r13` lists what the other orders change (export-offence-synth r13Eval)
  const once = async (j: Job): Promise<Rec> => {
    const code = loadChunk(j.xml, j.rel.replace(/^.*\//, '').replace(/\.xml$/, '')) + DUMP
    const want = rustR13Key(j.xml)
    if (!want.ambiguous) { const o = await ljEval(code); delete o.r13Key; return o }
    const byKey = new Map<string, Rec>()
    for (let i = 0; i < R13_TRIES && !(byKey.has(want.key) && byKey.size > 1); i++) {
      const o = await ljEval(code)
      const k = String(o.r13Key ?? '')
      delete o.r13Key
      if (!byKey.has(k)) byKey.set(k, o)
      if (o.timeout || o.loadError || o.calcError) return o
    }
    const got = byKey.get(want.key)
    if (!got) return { r13Error: `R13 order ${want.key} not seen in ${R13_TRIES} runs (seen ${[...byKey.keys()].join(' ; ')})` }
    const otherOrder: Rec = {}
    for (const [k, o] of byKey) if (k !== want.key) otherOrder[k] = diffKeys(got, o)
    return { ...got, r13: { class: 'lj-strhash', place: 'Classes/SkillsTab.lua:474 pairs(possibleVariants) (old-format <Gem>: gemId + skillId, no variantId)', ambiguousGems: want.ambiguous, order: want.key, ordersSeen: [...byKey.keys()].sort(byteOrder), otherOrder } }
  }
  console.error(`pass 1/2 (${files.length} builds, ${jobs.length} jobs)`)
  const a = await pool(jobs, once)
  console.error('pass 2/2')
  const b = await pool(jobs, once)
  const rows: Rec[] = []
  jobs.forEach((j, i) => {
    const { rel, slug, xml } = j
    const d = diffKeys(a[i]!, b[i]!)
    const o = a[i]!
    const row: Rec = { rel: j.variant ? `${rel} [${j.variant}]` : rel, slug, status: o.timeout ? 'timeout' : o.loadError ? 'loadError' : o.calcError ? 'calcError' : o.r13Error ? 'r13Error' : 'ok', passesDiffer: d.length, keys: Object.keys(o.output?.nums ?? {}).length + Object.keys(o.output?.strs ?? {}).length + Object.keys(o.output?.bools ?? {}).length, minionKeys: o.minion ? Object.keys(o.minion.nums ?? {}).length : 0, notes: (o.loadNotes ?? []).length, main: o.mainSkill?.name ?? null }
    rows.push(row)
    if (d.length) { console.log(`DIFFERS between passes: ${row.rel}: ${d.slice(0, 20).join(', ')}${d.length > 20 ? ' ...' : ''}`); writeFileSync(resolve(OUT, `${slug}.pass2-differs.txt`), d.join('\n') + '\n'); return }
    if (o.r13Error) { console.log(`R13 STOP: ${row.rel}: ${o.r13Error}`); return }
    if (j.xmlFile) writeFileSync(resolve(OUT, j.xmlFile), xml)
    const text = norm(ser({ build: rel, set: SET, slug, variant: j.variant, xmlFile: j.xmlFile, xmlSha256: sha(xml), ...o, producer: { image, luajit: 'desktop-path LuaJIT docker (Alpine/musl libm, MINEFIELD-RUST R2), desktop text mode -> loadBuildFromXML -> wipeGlobalCache -> calcs.buildOutput(build,"MAIN")', pobSha, generator: 'tools/lua-corpus/export-real.ts' } })) + '\n'
    writeFileSync(resolve(OUT, `${slug}.luajit.json`), text)
  })
  for (const r of rows) console.log(`${r.status}\t${r.passesDiffer ? 'DIFF ' + r.passesDiffer : 'same'}\tkeys ${r.keys}\tminion ${r.minionKeys}\tnotes ${r.notes}\t${r.rel}\t${r.main}`)
  console.log(`builds ${rows.length}: ok ${rows.filter((r) => r.status === 'ok').length}, loadError ${rows.filter((r) => r.status === 'loadError').length}, calcError ${rows.filter((r) => r.status === 'calcError').length}, timeout ${rows.filter((r) => r.status === 'timeout').length} ${rows.filter((r) => r.status === 'timeout').map((r) => r.rel).join(', ')}, passes differ ${rows.filter((r) => r.passesDiffer).length}`)
}

async function perf(filter: RegExp): Promise<void> {
  const RUNS = 9
  const files = walk(PC).filter((f) => filter.test(relOf(f)))
  await prepareLuts(files)
  mkdirSync(OUT, { recursive: true })
  const out: Rec = { runs: RUNS, note: 'LuaJIT docker jit.on(), os.clock CPU ms; run 0 = warm-up (dropped); each run = wipeGlobalCache + calcs.buildOutput(build, "MAIN")', builds: {} }
  for (const f of files) { // one container at a time: timings without contention
    const r = await ljEval(loadChunk(desktopBytes(f), 'perf') + PERF(RUNS))
    if (r.loadError || r.timeout) { out.builds[slugOf(relOf(f))] = { loadError: r.loadError ?? r.timeout }; continue }
    const xs = Object.values(r.ms as Rec).map(Number)
    out.builds[slugOf(relOf(f))] = { median: median(xs), min: Math.min(...xs), n: xs.length, jit: r.jit }
    console.error(`perf ${relOf(f)}: ${median(xs).toFixed(2)} ms`)
  }
  writeFileSync(resolve(OUT, '_perf-luajit.json'), JSON.stringify(out, null, 1) + '\n')
}

/** R-party: `loadBuildFromXML` (which runs the calc) and a recalc (`wipeGlobalCache` + `buildOutput(MAIN)`) ms, in one warm process; also the build without its <Party>. */
async function perfLoad(filter: RegExp): Promise<void> {
  const RUNS = 9
  const files = walk(PC).filter((f) => filter.test(relOf(f)))
  await prepareLuts(files)
  mkdirSync(OUT, { recursive: true })
  const code = (xml: Buffer): string => String.raw`
jit.on()
local xml = ${luaBytes(xml)}
local L, C = {}, {}
for i = 0, ${RUNS} do
  local t0 = os.clock()
  loadBuildFromXML(xml, "perf")
  local t1 = os.clock()
  wipeGlobalCache()
  local t2 = os.clock()
  build.calcsTab.calcs.buildOutput(build, "MAIN")
  local t3 = os.clock()
  if i > 0 then L[i] = (t1 - t0) * 1000; C[i] = (t3 - t2) * 1000 end
end
return { load = L, recalc = C, jit = jit.version }
`
  const out: Rec = { runs: RUNS, note: 'LuaJIT docker jit.on(), os.clock CPU ms, median; run 0 = warm-up (dropped)', builds: {} }
  for (const f of files) {
    const xml = desktopBytes(f)
    for (const [slug, x] of [[slugOf(relOf(f)), xml], [`${slugOf(relOf(f))}.noparty`, stripP5(xml)]] as [string, Buffer][]) {
      const r = await ljEval(code(x))
      if (r.timeout) throw new Error(`${slug}: ${r.timeout}`)
      out.builds[slug] = { load: median(Object.values(r.load as Rec).map(Number)), recalc: median(Object.values(r.recalc as Rec).map(Number)) }
      console.log(slug, JSON.stringify(out.builds[slug]))
    }
  }
  writeFileSync(resolve(OUT, '_perf-load-luajit.json'), JSON.stringify(out, null, 1) + '\n')
}

/** R-tattoo: LuaJIT's `<Tree>` of SaveDB (TreeTab:Save -> every PassiveSpec:Save, PassiveSpec.lua:197-265) for the builds with tattoos, two runs. */
async function treeSave(filter?: RegExp): Promise<void> {
  const files = listBuilds().filter((f) => (!filter || filter.test(relOf(f))) && desktopBytes(f).includes('<Override '))
  if (!files.length) throw new Error(`no build with tattoos of set ${SET} matches ${filter}`)
  await prepareLuts(files)
  mkdirSync(OUT, { recursive: true })
  const pobSha = (await run('git', ['-C', POB, 'rev-parse', '--short', 'HEAD'])).stdout.trim()
  const SAVE = String.raw`
if LOAD.error then return { loadError = LOAD.error, loadNotes = LOAD.notes } end
local t = { elem = "Tree" }
build.treeTab:Save(t)
local text, err = require("xml").ComposeXML(t)
return { tree = text, composeError = err, loadNotes = LOAD.notes }
`
  const one = (f: string): Promise<Rec> => ljEval(loadChunk(desktopBytes(f), relOf(f).replace(/^.*\//, '').replace(/\.xml$/, '')) + SAVE)
  const a = await pool(files, one)
  const b = await pool(files, one)
  files.forEach((f, i) => {
    const rel = relOf(f), slug = slugFor(f)
    if (ser(a[i]) !== ser(b[i])) { console.log(`DIFFERS between passes: ${rel}`); return }
    const o = a[i]!
    const text = ser({ build: rel, set: SET, slug, xmlSha256: sha(desktopBytes(f)), ...o, producer: { luajit: 'LuaJIT docker, desktop text mode -> loadBuildFromXML -> build.treeTab:Save -> xml.ComposeXML', pobSha, generator: 'tools/lua-corpus/export-real.ts tree-save' } }) + '\n'
    writeFileSync(resolve(OUT, `${slug}.tree-save.json`), text)
    console.log(`${o.loadError ? 'loadError' : o.composeError ? 'composeError' : 'ok'}\t${(desktopBytes(f).toString('latin1').match(/<Override /g) ?? []).length} overrides\t${rel}`)
  })
}

if (process.argv[2] === 'tree-save') await treeSave(process.argv[3] ? new RegExp(process.argv[3]) : undefined)
else if (process.argv[2] === 'perf') await perf(new RegExp(process.argv[3] ?? '^3\\.29/'))
else if (process.argv[2] === 'perf-load') await perfLoad(new RegExp(process.argv[3] ?? '^3\\.29/build-c'))
else await fixtures(process.argv[2] ? new RegExp(process.argv[2]) : undefined)
