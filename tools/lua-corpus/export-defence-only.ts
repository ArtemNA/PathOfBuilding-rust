// R4.3: defence-only fixtures -- pob-ts `perform(env, defence, noopEstimations)` + `finishOutput(env)` for the 6 golden
// builds (the executable spec of CalcDefence.lua:511-1671 on top of R4.2's perform), i.e. what `pob_calc::calculate` with
// the real `defence` and a stub `buildDefenceEstimations` must reproduce exactly (R4.4 plugs the estimations in).
// Run from the pob-rust root (Node 24):
//   node tools/lua-corpus/export-defence-only.ts   -> crates/pob-calc/tests/fixtures/<build>.defence-pobts.json
// Same encoding as export-perform-only.ts: numbers through `formatG(v, 17)` (`%.17g`, exact; `-0` stays "-0" and is also
// listed in `negZero`), strings as `{ s }`, booleans as is. Two runs must be byte-identical (`git diff` / `cmp`).
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
const P = 'file:///C:/pob-work/pob-ts'
const { loadBuildXml } = await import(`${P}/src/build/load.ts`)
const { initEnv } = await import(`${P}/src/calc/initEnv.ts`)
const { finishOutput, perform } = await import(`${P}/src/calc/perform.ts`)
const { defence } = await import(`${P}/src/calc/defence.ts`)
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
  perform(env, defence, noop)
  finishOutput(env)
  const { stats, negZero } = statsDump(outputToStats(env.player.output))
  const fixture = {
    build: build.name,
    producer: 'tools/lua-corpus/export-defence-only.ts (pob-ts perform(env, defence, noop) + finishOutput)',
    statCount: Object.keys(stats).length,
    stats,
    negZero,
    player: { conditions: conds(env.modDB), multipliers: mults(env.modDB) },
    enemy: { conditions: conds(env.enemyDB), multipliers: mults(env.enemyDB) },
    // what perform + defence + finishOutput pushed (initEnv's own entries are R3's gate), in push order.
    unimplemented: env.unimplemented.slice(before),
  }
  const file = resolve(FIX, `${build.name.replace(/ /g, '-')}.defence-pobts.json`)
  writeFileSync(file, JSON.stringify(fixture, null, 1) + '\n')
  console.log(`${build.name}: ${String(fixture.statCount)} stats, ${String(negZero.length)} -0, ${String(fixture.unimplemented.length)} unimplemented -> ${file}`)
}
