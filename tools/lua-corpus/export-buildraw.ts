// R8.2b: LuaJIT truth for `Item:BuildRaw` / `BuildAndParseRaw` (Classes/Item.lua :1772-2052). Mechanics = export-r8.ts (docker LuaJIT, HeadlessWrapper boot).
// Inputs (raw text in, BuildRaw out):
//   builds  = every <Item> of the 6 goldens + the 20 R7-synth scenarios of R8.1 (export-r8.ts SYNTH_SET), built exactly like ItemsTab:Load (ItemsTab.lua:1222-1263, copied verbatim:
//             Item(""), id/variant/variantAlt* attributes, ParseRaw(text), <ModRange>, BuildModList when the base resolved);
//   uniques = data/uniques.json `byType` (the converter's copy of data.uniques), every 3rd entry per type, built like Main.lua:166 `new("Item"):Item(raw, "UNIQUE", true)` (sanitiseText inside);
//             items with a variant list also get the UI variant switch on a fresh copy (`item.variant = 1; item:BuildAndParseRaw()`, ItemsTab variant dropdown).
// Per item the sequence is b1 = BuildRaw(); BuildAndParseRaw(); b2 = BuildRaw(); BuildAndParseRaw(); b3 = BuildRaw()  (b2/b3 stored only when they differ from the previous one).
// Usage (Node 24, repo root):  node tools/lua-corpus/export-buildraw.ts        -> crates/pob-items/tests/fixtures/buildraw.json (2 docker passes, must be byte-identical)
//                              node tools/lua-corpus/export-buildraw.ts perf   -> tools/.scratch/perf-buildraw.json (LuaJIT ns per item)
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
const run = promisify(execFile)
const P = 'file:///C:/pob-work/pob-ts'
const { GOLDEN_BUILDS, REPO_ROOT, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
const { LUAJIT_IMAGE, dockerAvailable } = await import(`${P}/tools/oracle/luajit.ts`)
type Rec = Record<string, any> // eslint-disable-line
if (!dockerAvailable()) throw new Error('docker is not available (infrastructure) -- STOP')
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const OUT = resolve(ROOT, 'crates/pob-items/tests/fixtures/buildraw.json')
const SYNTH = resolve(ROOT, 'crates/pob-calc/tests/fixtures/offence-synth')
const SCRATCH = resolve(ROOT, 'tools/.scratch')
const MAX_BYTES = 1_500_000
const UNIQUE_STEP = 3
const roots = resolveRoots()
const sha = (b: Buffer | string): string => createHash('sha256').update(b).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)
const SYNTH_SET = ['fd-gen-melee', 'fd-occ-spells', 'fd-occ-dot', 'fd-occ-trap-mine-totem', 'fd-occ-minions', 'fd-mir-mirage', 'fd-mir-bow-ballista', 'fd-cos-coc', 'fd-occ-poison-ignite', 'fd-all-ds',
  't-trap', 't-mine', 't-coc', 't-cwc', 't-cospri', 'mi-sacred-wisps', 'mn-raise-zombie', 'mn-spectre-hydra-mid', 'mn-golem-flame', 'fn-Spark']

async function ljEval(code: string): Promise<Rec> {
  const tmp = mkdtempSync(resolve(tmpdir(), 'pob-r82b-'))
  try {
    writeFileSync(resolve(tmp, 'code.lua'), code)
    const args = ['run', '--rm', '-v', `${roots.pobRoot}:/workdir:ro`, '-v', `${resolve(REPO_ROOT, 'tools/oracle/lua')}:/bench:ro`, '-v', `${tmp}:/code:ro`,
      '-w', '/workdir/src', '-e', 'LUA_PATH=./?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;', LUAJIT_IMAGE, 'luajit', '/bench/luajit-eval.lua', '/code/code.lua']
    try { const { stdout } = await run('docker', args, { encoding: 'utf8', maxBuffer: 1024 * 1024 * 1024 }); return JSON.parse(stdout.trim().split('\n').pop()!).result } catch (e) { throw new Error(`docker/luajit failed (infrastructure): ${(e as Rec).stderr ?? (e as Error).message}`) }
  } finally { rmSync(tmp, { recursive: true, force: true }) }
}
function luaBytes(buf: Buffer): string {
  let s = '"'
  for (const b of buf) s += b === 0x5c ? '\\\\' : b === 0x22 ? '\\"' : b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '\\' + String(b).padStart(3, '0')
  return s + '"'
}

type Scenario = { slug: string; kind: 'golden' | 'synth'; xml: Buffer }
type Unique = { type: string; index: number; raw: string }
function scenarios(): Scenario[] {
  const s: Scenario[] = GOLDEN_BUILDS.map((b: Rec) => ({ slug: b.name.replace(/ /g, '-'), kind: 'golden' as const, xml: readFileSync(b.xmlPath(roots)) }))
  for (const n of SYNTH_SET) s.push({ slug: n, kind: 'synth', xml: readFileSync(resolve(SYNTH, `${n}.xml`)) })
  return s
}
function uniques(): Unique[] {
  const j = JSON.parse(readFileSync(resolve(ROOT, 'data/uniques.json'), 'utf8')) as Rec
  const out: Unique[] = []
  for (const type of Object.keys(j.byType)) (j.byType[type] as string[]).forEach((raw, index) => { if (index % UNIQUE_STEP === 0) out.push({ type, index, raw }) })
  return out
}

const LUA_COMMON = String.raw`
local function itemFromNode(node) -- ItemsTab.lua:1222-1263 (verbatim, minus the tab bookkeeping)
  local item = new("Item"):Item("")
  item.id = tonumber(node.attrib.id)
  item.variant = tonumber(node.attrib.variant)
  if node.attrib.variantAlt then item.hasAltVariant = true item.variantAlt = tonumber(node.attrib.variantAlt) end
  if node.attrib.variantAlt2 then item.hasAltVariant2 = true item.variantAlt2 = tonumber(node.attrib.variantAlt2) end
  if node.attrib.variantAlt3 then item.hasAltVariant3 = true item.variantAlt3 = tonumber(node.attrib.variantAlt3) end
  if node.attrib.variantAlt4 then item.hasAltVariant4 = true item.variantAlt4 = tonumber(node.attrib.variantAlt4) end
  if node.attrib.variantAlt5 then item.hasAltVariant5 = true item.variantAlt5 = tonumber(node.attrib.variantAlt5) end
  for _, child in ipairs(node) do
    if type(child) == "string" then
      item:ParseRaw(child)
    elseif child.elem == "ModRange" then
      local id = tonumber(child.attrib.id) or 0
      local range = tonumber(child.attrib.range) or 1
      for _, list in ipairs{item.buffModLines, item.enchantModLines, item.scourgeModLines, item.implicitModLines, item.explicitModLines, item.crucibleModLines} do
        if id <= #list then list[id].range = range break end
        id = id - #list
      end
    end
  end
  if item.base then item:BuildModList() return item end
end
local function buildItems(xmlText)
  local dbXML = common.xml.ParseXML(xmlText)
  local out = {}
  for _, top in ipairs(dbXML) do
    if top.elem == "PathOfBuilding" then
      for _, sec in ipairs(top) do
        if type(sec) == "table" and sec.elem == "Items" then
          for _, node in ipairs(sec) do
            if type(node) == "table" and node.elem == "Item" then
              local item = itemFromNode(node)
              if item then out[#out + 1] = item end
            end
          end
        end
      end
    end
  end
  return out
end
-- R13: BuildRaw walks pairs(self.baseLines) (Item.lua:1951/1981), a hash keyed by base name -- with 2+ written base lines their order is the hash order,
-- which differs between LuaJIT processes (and between re-parses in one process). Such an item is flagged r13 (count of written base lines >= 2 in any state).
local function writtenBaseLines(item)
  local n = 0
  for _, bl in pairs(item.baseLines or {}) do
    if (item.variantList and (bl.variantList or bl.versionList or bl.variantGroupList)) or (not item.variantList and (bl.versionList or bl.variantGroupList)) then n = n + 1 end
  end
  return n
end
-- Canonical form for r13 items (orchestrator, R13 rule of R8.1): every run of base lines (a line whose text after its leading
-- {..} tags is a key of self.baseLines, from line 4 on -- lines 1-3 are rarity/title/base) is sorted bytewise; the rest is verbatim.
local function stripTags(l)
  local n
  repeat l, n = l:gsub("^{[^}]*}", "") until n == 0
  return l
end
local function canon(text, item)
  local lines = {}
  local NL = string.char(10)
  for l in (text .. NL):gmatch("([^" .. NL .. "]*)" .. NL) do lines[#lines + 1] = l end
  local function isBase(i) return i > 3 and item.baseLines[stripTags(lines[i])] ~= nil end
  local out, i = {}, 1
  while i <= #lines do
    if isBase(i) then
      local run = {}
      while i <= #lines and isBase(i) do run[#run + 1] = lines[i] i = i + 1 end
      table.sort(run)
      for _, x in ipairs(run) do out[#out + 1] = x end
    else
      out[#out + 1] = lines[i]
      i = i + 1
    end
  end
  return table.concat(out, NL)
end
local function seq(item)
  local r13 = writtenBaseLines(item) >= 2
  local ok, b1 = pcall(item.BuildRaw, item)
  if not ok then return { err = tostring(b1) } end
  local c1 = canon(b1, item)
  item:BuildAndParseRaw()
  r13 = r13 or writtenBaseLines(item) >= 2
  local b2 = item:BuildRaw()
  local c2 = canon(b2, item)
  item:BuildAndParseRaw()
  r13 = r13 or writtenBaseLines(item) >= 2
  local b3 = item:BuildRaw()
  local c3 = canon(b3, item)
  if r13 then return { b1 = c1, b2 = c2, b3 = c3, r13 = true } end
  return { b1 = b1, b2 = b2, b3 = b3 }
end
`

const PROBE = (sc: Scenario[], un: Unique[]): string => LUA_COMMON + String.raw`
local scen = { ${sc.map((s) => `{ slug = ${JSON.stringify(s.slug)}, xml = ${luaBytes(s.xml)} }`).join(',\n')} }
local uniq = { ${un.map((u) => luaBytes(Buffer.from(u.raw, 'utf8'))).join(',\n')} }
local res = { builds = {}, uniques = {} }
for i, s in ipairs(scen) do
  local items = {}
  for _, item in ipairs(buildItems(s.xml)) do
    local r = seq(item)
    r.id = item.id
    items[#items + 1] = r
  end
  res.builds[i] = { slug = s.slug, items = items }
end
for i, raw in ipairs(uniq) do
  local r = {}
  local item = new("Item"):Item(raw, "UNIQUE", true)
  if item.base then
    r.seq = seq(item)
    local item2 = new("Item"):Item(raw, "UNIQUE", true)
    if item2.variantList and #item2.variantList > 1 then
      item2.variant = 1
      local ok, e = pcall(item2.BuildAndParseRaw, item2)
      r.variant1 = ok and seq(item2) or { err = tostring(e) }
    end
  else
    r.noBase = true
  end
  res.uniques[i] = r
end
return res
`

const PERF = (sc: Scenario[], un: Unique[], reps: number): string => LUA_COMMON + String.raw`
local scen = { ${sc.map((s) => luaBytes(s.xml)).join(',\n')} }
local uniq = { ${un.map((u) => luaBytes(Buffer.from(u.raw, 'utf8'))).join(',\n')} }
jit.on()
local items = {}
for _, x in ipairs(scen) do for _, it in ipairs(buildItems(x)) do items[#items + 1] = it end end
for _, raw in ipairs(uniq) do local it = new("Item"):Item(raw, "UNIQUE", true) if it.base then items[#items + 1] = it end end
local R = { items = #items, buildRaw = {}, buildAndParseRaw = {}, jit = jit.version }
for r = 0, ${reps} do
  local t0 = os.clock()
  for _, it in ipairs(items) do it:BuildRaw() end
  local t1 = os.clock()
  for _, it in ipairs(items) do it:BuildAndParseRaw() end
  local t2 = os.clock()
  if r > 0 then R.buildRaw[r] = (t1 - t0) * 1e9 / #items R.buildAndParseRaw[r] = (t2 - t1) * 1e9 / #items end
end
return R
`

type ItemOut = { id?: number; b1?: string; b2?: string; b3?: string; err?: string; r13?: boolean }
function compactSeq(r: Rec): ItemOut {
  if (r.err) return { err: r.err }
  return { b1: r.b1, b2: r.b2 !== r.b1 ? r.b2 : undefined, b3: r.b3 !== r.b2 ? r.b3 : undefined, r13: r.r13 || undefined }
}

function ser(v: unknown): string {
  if (typeof v === 'number') return Object.is(v, -0) ? '-0.0' : JSON.stringify(v)
  if (v === null || typeof v !== 'object') return JSON.stringify(v)
  if (Array.isArray(v)) return `[${v.map(ser).join(',')}]`
  const o = v as Rec
  return `{${Object.keys(o).filter((k) => o[k] !== undefined).sort(byteOrder).map((k) => `${JSON.stringify(k)}:${ser(o[k])}`).join(',')}}`
}

async function pass(sc: Scenario[], un: Unique[]): Promise<{ text: string; stats: Rec }> {
  const r = await ljEval(PROBE(sc, un))
  const builds = sc.map((s, i) => {
    const b = r.builds[i]
    if (b.slug !== s.slug) throw new Error(`order mismatch ${b.slug} != ${s.slug}`)
    return { slug: s.slug, kind: s.kind, xmlSha256: sha(s.xml), items: ((b.items ?? []) as Rec[]).map((x) => ({ id: x.id, ...compactSeq(x) })) }
  })
  const uniquesOut = un.map((u, i) => {
    const x = r.uniques[i] as Rec
    return { type: u.type, index: u.index, rawSha256: sha(u.raw), noBase: x.noBase || undefined, seq: x.seq ? compactSeq(x.seq) : undefined, variant1: x.variant1 ? compactSeq(x.variant1) : undefined }
  })
  const stats = {
    buildItems: builds.reduce((a, b) => a + b.items.length, 0),
    buildItemsB2Differs: builds.reduce((a, b) => a + b.items.filter((x) => x.b2 !== undefined).length, 0),
    buildItemsB3Differs: builds.reduce((a, b) => a + b.items.filter((x) => x.b3 !== undefined).length, 0),
    buildItemErrors: builds.reduce((a, b) => a + b.items.filter((x) => x.err !== undefined).length, 0),
    uniques: uniquesOut.length,
    uniquesNoBase: uniquesOut.filter((x) => x.noBase).length,
    uniquesWithVariant1: uniquesOut.filter((x) => x.variant1).length,
    uniquesB2Differs: uniquesOut.filter((x) => x.seq?.b2 !== undefined).length,
    uniquesB3Differs: uniquesOut.filter((x) => x.seq?.b3 !== undefined || x.variant1?.b3 !== undefined).length,
    uniqueErrors: uniquesOut.filter((x) => x.seq?.err !== undefined || x.variant1?.err !== undefined).length,
    r13BuildItems: builds.reduce((a, b) => a + b.items.filter((x) => x.r13).length, 0),
    r13Uniques: uniquesOut.filter((x) => x.seq?.r13).length,
    r13UniqueVariant1: uniquesOut.filter((x) => x.variant1?.r13).length,
  }
  const producer = { generator: 'tools/lua-corpus/export-buildraw.ts', image: LUAJIT_IMAGE, note: 'LuaJIT docker; r13 = 2+ written base lines (pairs(self.baseLines) Item.lua:1951/1981: order differs between LuaJIT processes); r13 texts are CANONICAL: each run of base lines (line 4+, text after leading {..} tags is a baseLines key) sorted bytewise, rest verbatim; items built like ItemsTab:Load (builds) / Main.lua uniqueDB (uniques); b1 = BuildRaw(), b2/b3 = BuildRaw() after 1/2 BuildAndParseRaw() (only when different from the previous)' }
  const text = ser({ format: 1, producer, uniqueStep: UNIQUE_STEP, stats, builds, uniques: uniquesOut }) + '\n'
  return { text, stats }
}

async function fixtures(): Promise<void> {
  const sc = scenarios(), un = uniques()
  console.error(`pass 1 (${sc.length} scenarios, ${un.length} uniques)`)
  const a = await pass(sc, un)
  console.error('pass 2')
  const b = await pass(sc, un)
  mkdirSync(SCRATCH, { recursive: true })
  if (a.text !== b.text) {
    writeFileSync(resolve(SCRATCH, 'buildraw-pass1.json'), a.text)
    writeFileSync(resolve(SCRATCH, 'buildraw-pass2.json'), b.text)
    console.log(JSON.stringify({ STOP: 'passes differ', files: ['tools/.scratch/buildraw-pass1.json', 'tools/.scratch/buildraw-pass2.json'] }))
    process.exitCode = 1
    return
  }
  const bytes = Buffer.byteLength(a.text)
  if (bytes > MAX_BYTES) throw new Error(`fixture ${bytes} bytes > ${MAX_BYTES} -- STOP`)
  mkdirSync(dirname(OUT), { recursive: true })
  writeFileSync(OUT, a.text, 'utf8')
  console.log(JSON.stringify({ passesIdentical: true, bytes, sha256: sha(a.text), ...a.stats }, null, 1))
}

async function perf(): Promise<void> {
  const reps = 10
  const r = await ljEval(PERF(scenarios(), uniques(), reps))
  const med = (xs: number[]): number => { const s = [...xs].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]! }
  const out = { note: 'LuaJIT docker jit.on(), os.clock CPU, ns per item, run 0 = warm-up (dropped); items = all build items + sampled uniques with a base', jit: r.jit, items: r.items, reps,
    buildRawNsPerItem: { median: med(r.buildRaw), min: Math.min(...r.buildRaw) }, buildAndParseRawNsPerItem: { median: med(r.buildAndParseRaw), min: Math.min(...r.buildAndParseRaw) }, series: { buildRaw: r.buildRaw, buildAndParseRaw: r.buildAndParseRaw } }
  mkdirSync(SCRATCH, { recursive: true })
  writeFileSync(resolve(SCRATCH, 'perf-buildraw.json'), JSON.stringify(out, null, 2) + '\n')
  console.log(JSON.stringify(out, null, 1))
}

if (process.argv[2] === 'perf') await perf()
else await fixtures()
