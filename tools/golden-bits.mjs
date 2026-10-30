// Prints `<build>\t<key>\t<f64 bits hex>` for every numeric stat of the six pob-ts goldens, as JS JSON.parse reads them
// (negZero restored). Pair: `cargo run -p pob-golden --example dump_bits` -- the two outputs must be byte-identical.
import { readFileSync } from 'node:fs'

const root = (process.env.POB_TS_ROOT ?? 'C:/pob-work/pob-ts') + '/tools/oracle/goldens/'
const builds = ['Dual Savior', 'Dual Wield Cospris CoC', 'Dual Wield Cospris CoC - Lethal Pride', 'Generals Perforate Zerker',
  'Mirage Archer Toxic Rain', 'OccVortex']
const view = new DataView(new ArrayBuffer(8))
const lines = []
for (const b of builds) {
  const snap = JSON.parse(readFileSync(root + b + '.json', 'utf8'))
  for (const k of snap.negZero ?? []) snap.stats[k] = -0
  for (const k of Object.keys(snap.stats).sort((x, y) => (x < y ? -1 : x > y ? 1 : 0))) {
    const v = snap.stats[k]
    if (typeof v !== 'number') continue
    view.setFloat64(0, v)
    lines.push(`${b}\t${k}\t${view.getBigUint64(0).toString(16).padStart(16, '0')}`)
  }
}
process.stdout.write(lines.join('\n') + '\n')
