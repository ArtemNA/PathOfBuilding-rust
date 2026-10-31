// R4.2: perform-only fixtures -- pob-ts `perform(env, noopDefence, noopEstimations)` + `finishOutput(env)` for the 6 golden
// builds (the executable spec of CalcPerform.lua:1286-3792 without skills, CALC-ARCH §8), i.e. what `pob_calc::calculate`
// must reproduce exactly while defence/estimations are stubs (R4.3/R4.4 plug the real ones in).
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-perform-only.ts   -> crates/pob-calc/tests/fixtures/<build>.perform-pobts.json
// Numbers go through `formatG(v, 17)` (`%.17g`, exact; `-0` stays "-0" and is also listed in `negZero`), so the Rust side
// compares bits, never the 14-digit channel (MINEFIELD O2). Two runs must be byte-identical (`git diff` / `cmp`).
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { loadBuildXml } = await import(`${P}/src/build/load.ts`)
const { initEnv } = await import(`${P}/src/calc/initEnv.ts`)
const { finishOutput, perform } = await import(`${P}/src/calc/perform.ts`)
const { outputToStats } = await import(`${P}/src/calc/output.ts`)
const { formatG } = await import(`${P}/src/lua/index.ts`)
const { GOLDEN_BUILDS, resolveRoots } = await import(`${P}/tools/oracle/paths.ts`)
type Rec = Record<string, any> // eslint-disable-line

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FIX = resolve(ROOT, 'crates/pob-calc/tests/fixtures')
const roots = resolveRoots()

const noop = (): void => {}
const sortObj = <T>(o: Record<string, T>): Record<string, T> => Object.fromEntries(Object.keys(o).sort().map((k) => [k, o[k]!]))
const num = (v: number): string => formatG(v, 17)

/** `stats`: number -> `%.17g` string, string -> `{ s }`, boolean as is (outputToStats already mapped NaN/inf to strings). */
function statsDump(stats: Rec): { stats: Rec; negZero: string[] } {
  const out: Rec = {}
  const negZero: string[] = []
  for (const k of Object.keys(stats).sort()) {
    const v = stats[k]
    if (typeof v === 'number') {
      out[k] = num(v)
      if (Object.is(v, -0)) negZero.push(k)
    } else if (typeof v === 'string') out[k] = { s: v }
    else out[k] = v
  }
  return { stats: out, negZero }
}
const conds = (db: Rec): Rec => sortObj(db.conditions as Record<string, boolean>)
const mults = (db: Rec): Rec => sortObj(Object.fromEntries(Object.entries(db.multipliers as Record<string, number>).map(([k, v]) => [k, num(v)])))

for (const build of GOLDEN_BUILDS) {
  const xml = readFileSync(build.xmlPath(roots), 'utf8')
  const model = loadBuildXml(xml)
  const env = initEnv(model)
  const before = env.unimplemented.length
  perform(env, noop, noop)
  finishOutput(env)
  const { stats, negZero } = statsDump(outputToStats(env.player.output))
  const fixture = {
    build: build.name,
    producer: 'tools/lua-corpus/export-perform-only.ts (pob-ts perform(env, noop, noop) + finishOutput)',
    statCount: Object.keys(stats).length,
    stats,
    negZero,
    player: { conditions: conds(env.modDB), multipliers: mults(env.modDB) },
    enemy: { conditions: conds(env.enemyDB), multipliers: mults(env.enemyDB) },
    // initEnv's own entries are R3's gate; these are the ones perform + finishOutput pushed, in push order.
    unimplemented: env.unimplemented.slice(before),
  }
  const file = resolve(FIX, `${build.name.replace(/ /g, '-')}.perform-pobts.json`)
  writeFileSync(file, JSON.stringify(fixture, null, 1) + '\n')
  console.log(`${build.name}: ${String(fixture.statCount)} stats, ${String(negZero.length)} -0, ${String(fixture.unimplemented.length)} unimplemented -> ${file}`)
}
