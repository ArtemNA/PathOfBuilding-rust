// Golden snapshots from desktop LuaJIT (the official PoB docker image): a build XML -> `build.calcsTab.mainOutput` after
// loadBuildFromXML, as the snapshot crates/pob-golden reads (crates/pob-golden/src/snapshot.rs). Origin: pob-ts
// tools/oracle/{luajit,format,lua-snippets,snapshot}.ts (luajitSnapshot, serializeSnapshot, parseDump17, the channel
// cross-check) at pob-ts commit 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into pob-rust (R-INDEPENDENT-UPDATE
// A3) and owned here since. Differences: zod schemas are plain checks; only the no-mutation case (the goldens) is
// supported; LUTs come from tools/oracle/luajit.ts (the upstream checkout), not pob-engine-wasm; no `numType` (Lua 5.4
// integer subtype -- LuaJIT has none, MINEFIELD O1).
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { resolve } from 'node:path'
import { LUA_DIR } from './luajit-eval.ts'
import { ensureJewelLuts, LUAJIT_IMAGE } from './luajit.ts'
import type { Roots } from './paths.ts'

export const CASE_FORMAT = 1
export const SNAPSHOT_FORMAT = 1
const DUMP_SCRIPT = resolve(LUA_DIR, 'luajit-dump17.lua')
const LUA_PATH_ENV = './?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;'
/** Max relative distance tolerated between the %.14g and %.17g readings of one number (14 digits -> ~5e-14). */
const CHANNEL_REL_TOLERANCE = 1e-13

export type StatValue = number | string | boolean
export type Stats = Record<string, StatValue>
export interface Meta {
  allocNodes: number
  items: number
  socketGroups: number
  mainSocketGroup?: number
  treeVersion: string
  className: string
  ascendName: string
}
export interface OracleCase {
  format: number
  id: string
  build: string
  xmlSha256: string
  mutations: unknown[]
}
export interface OracleSnapshot {
  format: number
  case: OracleCase
  producer: { kind: 'luajit'; pobSha: string; engine: string }
  meta: Meta
  stats: Stats
  stats14: Stats
}

export const sha256Hex = (data: string | Uint8Array): string => createHash('sha256').update(data).digest('hex')
const byteOrder = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0)

/** The case of a golden build: no mutations (format.ts `loadCase`; caseId = the build name). */
export function loadCase(build: string, xml: string): OracleCase {
  return { format: CASE_FORMAT, id: build, build, xmlSha256: sha256Hex(xml), mutations: [] }
}

/** NaN -> "nan", +-Infinity -> "inf"/"-inf", "-nan" -> "nan"; null/undefined = no value. */
function normalizeStatValue(v: unknown): StatValue | undefined {
  if (v === undefined || v === null) return undefined
  if (typeof v === 'number') return Number.isNaN(v) ? 'nan' : v === Infinity ? 'inf' : v === -Infinity ? '-inf' : v
  if (typeof v === 'string') return v === '-nan' ? 'nan' : v
  if (typeof v === 'boolean') return v
  throw new TypeError(`stat value must be a scalar, got ${Array.isArray(v) ? 'array' : typeof v}`)
}

/** Drops `_meta`, normalizes every value, sorts keys (byte order). `-0` is preserved. */
function normalizeStats(raw: Record<string, unknown>): Stats {
  const out: Stats = {}
  for (const key of Object.keys(raw).sort(byteOrder)) {
    if (key === '_meta') continue
    const v = normalizeStatValue(raw[key])
    if (v !== undefined) out[key] = v
  }
  return out
}

// dkjson encodes an empty table as `[]`, so an empty map arrives as an array.
function asMap<T>(x: unknown, what: string): Record<string, T> {
  if (x === undefined || x === null || Array.isArray(x)) return {}
  if (typeof x !== 'object') throw new Error(`luajit dump: '${what}' is not a table`)
  return x as Record<string, T>
}

/** `num` (%.17g text) + `str` + `bool` + `nonFinite` -> exact stats; throws on anything that is not a clean %.17g number. */
function parseExact(r: Record<string, unknown>): Stats {
  const stats: Record<string, StatValue> = {}
  for (const [k, s] of Object.entries(asMap<unknown>(r['num'], 'num'))) {
    if (typeof s !== 'string' || !/^-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/.test(s)) {
      throw new Error(`luajit dump: '${k}' is not a %.17g number: ${JSON.stringify(s)}`)
    }
    const n = Number(s)
    if (!Number.isFinite(n)) throw new Error(`luajit dump: '${k}' parsed non-finite from ${JSON.stringify(s)}`)
    stats[k] = n
  }
  for (const [k, s] of Object.entries(asMap<unknown>(r['str'], 'str'))) {
    if (typeof s !== 'string') throw new Error(`luajit dump: '${k}' in str is ${typeof s}`)
    stats[k] = s
  }
  for (const [k, b] of Object.entries(asMap<unknown>(r['bool'], 'bool'))) {
    if (typeof b !== 'boolean') throw new Error(`luajit dump: '${k}' in bool is ${typeof b}`)
    stats[k] = b
  }
  for (const [k, s] of Object.entries(asMap<unknown>(r['nonFinite'], 'nonFinite'))) {
    const v = normalizeStatValue(s)
    if (v !== 'nan' && v !== 'inf' && v !== '-inf') throw new Error(`luajit dump: '${k}' in nonFinite is ${JSON.stringify(s)}`)
    stats[k] = v
  }
  return normalizeStats(stats)
}

/** Identical key sets; numbers within CHANNEL_REL_TOLERANCE (the %.17g side is the denominator -- O8). */
function crossCheckChannels(stats: Stats, stats14: Stats): void {
  const keysExact = Object.keys(stats)
  const keys14 = Object.keys(stats14)
  if (keysExact.length !== keys14.length || keysExact.some((k, i) => k !== keys14[i])) {
    const only17 = keysExact.filter((k) => !(k in stats14))
    const only14 = keys14.filter((k) => !(k in stats))
    throw new Error(`luajit dump: key sets differ (only in num/%.17g: ${only17.join(',') || '-'}; only in num14/%.14g: ${only14.join(',') || '-'})`)
  }
  for (const k of keysExact) {
    const exact = stats[k]!
    const approx = stats14[k]!
    if (typeof exact === 'number' && typeof approx === 'number') {
      if (Math.abs(approx - exact) > Math.abs(exact) * CHANNEL_REL_TOLERANCE) {
        throw new Error(`luajit dump: '${k}' %.17g ${exact} vs %.14g ${approx} differ beyond ${CHANNEL_REL_TOLERANCE} relative`)
      }
    } else if (exact !== approx) {
      throw new Error(`luajit dump: '${k}' num ${JSON.stringify(exact)} vs num14 ${JSON.stringify(approx)} differ`)
    }
  }
}

function checkMeta(m: unknown): Meta {
  const r = asMap<unknown>(m, 'meta')
  const allowed = ['allocNodes', 'items', 'socketGroups', 'mainSocketGroup', 'treeVersion', 'className', 'ascendName']
  for (const k of Object.keys(r)) if (!allowed.includes(k)) throw new Error(`luajit dump: unknown meta field '${k}'`)
  const nat = (k: string) => { const v = r[k]; if (!Number.isInteger(v) || (v as number) < 0) throw new Error(`luajit dump: meta.${k} = ${JSON.stringify(v)}`); return v as number }
  const str = (k: string, nonEmpty: boolean) => { const v = r[k]; if (typeof v !== 'string' || (nonEmpty && !v)) throw new Error(`luajit dump: meta.${k} = ${JSON.stringify(v)}`); return v }
  const meta: Meta = {
    allocNodes: nat('allocNodes'),
    items: nat('items'),
    socketGroups: nat('socketGroups'),
    treeVersion: str('treeVersion', true),
    className: str('className', true),
    ascendName: str('ascendName', false),
  }
  if (r['mainSocketGroup'] !== undefined) {
    if (!Number.isInteger(r['mainSocketGroup'])) throw new Error(`luajit dump: meta.mainSocketGroup = ${JSON.stringify(r['mainSocketGroup'])}`)
    meta.mainSocketGroup = r['mainSocketGroup'] as number
  }
  return meta
}

/** Parser of luajit-dump17.lua's one-line JSON output. */
export function parseLuajitDump(json: string): { stats: Stats; stats14: Stats; meta: Meta } {
  const r = asMap<unknown>(JSON.parse(json), 'result')
  const stats = parseExact(r)
  const stats14 = normalizeStats({ ...asMap(r['num14'], 'num14'), ...asMap(r['str'], 'str'), ...asMap(r['bool'], 'bool'), ...asMap(r['nonFinite'], 'nonFinite') })
  crossCheckChannels(stats, stats14)
  return { stats, stats14, meta: checkMeta(r['meta']) }
}

/** xml -> `docker run` of luajit-dump17.lua (pobRoot:/workdir:ro, tools/oracle/lua:/bench:ro, LUTs:/jewel-data:ro) -> snapshot. */
export function luajitSnapshot(c: OracleCase, xml: string, opts: { roots: Roots; pobSha: string; image?: string }): OracleSnapshot {
  if (sha256Hex(xml) !== c.xmlSha256) throw new Error(`case ${c.id}: XML sha256 differs from the case`)
  if (c.mutations.length > 0) throw new Error(`case ${c.id}: mutations are not supported (goldens only)`)
  const image = opts.image ?? LUAJIT_IMAGE
  const lutsDir = ensureJewelLuts(xml, opts.roots)
  const tmpDir = mkdtempSync(resolve(tmpdir(), 'pob-golden-luajit-'))
  try {
    writeFileSync(resolve(tmpDir, 'build.xml'), xml)
    let stdout: string
    try {
      stdout = execFileSync(
        'docker',
        [
          'run', '--rm',
          '-v', `${opts.roots.pobRoot}:/workdir:ro`, '-v', `${LUA_DIR}:/bench:ro`,
          '-v', `${tmpDir}:/case:ro`, '-v', `${lutsDir}:/jewel-data:ro`,
          '-w', '/workdir/src', '-e', `LUA_PATH=${LUA_PATH_ENV}`, '-e', 'POB_JEWEL_DATA_DIR=/jewel-data', image,
          'luajit', '/bench/luajit-dump17.lua', '/case/build.xml',
        ],
        { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
      )
    } catch (err) {
      const e = err as { stderr?: string; message?: string }
      throw new Error(`luajit docker run failed: ${e.stderr ?? e.message ?? String(err)}`)
    }
    const lastLine = stdout.trim().split('\n').pop()
    if (!lastLine) throw new Error('luajit: empty stdout from docker run')
    const { stats, stats14, meta } = parseLuajitDump(lastLine)
    const scriptSha = sha256Hex(readFileSync(DUMP_SCRIPT))
    return {
      format: SNAPSHOT_FORMAT,
      case: c,
      producer: { kind: 'luajit', pobSha: opts.pobSha, engine: `${image} luajit-dump17.lua sha256:${scriptSha.slice(0, 12)}` },
      meta,
      stats,
      stats14,
    }
  } finally {
    rmSync(tmpDir, { recursive: true, force: true })
  }
}

function sortKeysDeep(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeysDeep)
  if (value !== null && typeof value === 'object') {
    const src = value as Record<string, unknown>
    const out: Record<string, unknown> = {}
    for (const k of Object.keys(src).sort(byteOrder)) if (src[k] !== undefined) out[k] = sortKeysDeep(src[k])
    return out
  }
  return value
}

/** Canonical JSON (byte-ordered keys at every level, 2-space indent, trailing newline); `-0` of `stats` moved to `negZero`. */
export function serializeSnapshot(snap: OracleSnapshot): string {
  const stats: Stats = {}
  const negZero: string[] = []
  for (const [key, raw] of Object.entries(normalizeStats(snap.stats))) {
    if (typeof raw === 'number' && Object.is(raw, -0)) {
      stats[key] = 0
      negZero.push(key)
    } else {
      stats[key] = raw
    }
  }
  const out: Record<string, unknown> = { ...snap, stats, stats14: normalizeStats(snap.stats14) }
  if (negZero.length > 0) out['negZero'] = negZero
  return JSON.stringify(sortKeysDeep(out), null, 2) + '\n'
}
