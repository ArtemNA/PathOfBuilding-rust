// Wire fixups between `pob-ts convert --wire-only` and `--extract-only` (R-UP, tools/sync-data.ps1 -Convert).
//
// pob-ts is READ-ONLY for pob-rust, and its extractors validate the wire shape strictly (an unknown key = error).
// When a new upstream adds a field that pob-rust does not consume, the converter dies; instead of editing pob-ts we
// drop exactly that field here, in the intermediate wire JSON, and say so in data.lock (`wireFixups`). Each fixup
// names the upstream line that introduced the field and why dropping it changes nothing the extractor would emit.
// A fixup that finds nothing to do is an error (stale rule -> remove it), so the list never silently rots.
//
//   node tools/convert-fixups.ts <wireDir>        -> prints a JSON array of applied fixups (for data.lock)
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

interface Fixup {
  id: string
  /** wire files touched (relative to wireDir); `after/<f>` too when present */
  files: string[]
  upstream: string
  why: string
  apply(v: unknown): number // returns how many values were changed
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

const FIXUPS: Fixup[] = [
  {
    id: 'itemBaseLists.searchFilter',
    files: ['itemBaseLists.json'],
    upstream: 'Modules/Data.lua:1261-1279 (3dbca4463, #10318 "Search for items by modifier in craft item menu")',
    why:
      'UI-only search string (name + implicit + enchant + flask/tincture buff lines, all already in itemBases); the item-bases ' +
      'extractor emits only {label,name} per entry and rejects unknown keys -- dropping it leaves its output identical.',
    apply(v) {
      if (!isObj(v)) throw new Error('itemBaseLists: expected an object')
      let n = 0
      for (const arr of Object.values(v)) {
        if (!Array.isArray(arr)) continue
        for (const e of arr) if (isObj(e) && 'searchFilter' in e) { delete e['searchFilter']; n++ }
      }
      return n
    },
  },
]

const wireDir = process.argv[2]
if (!wireDir) { console.error('usage: node tools/convert-fixups.ts <wireDir>'); process.exit(2) }
const applied: { id: string; upstream: string; why: string; changed: Record<string, number> }[] = []
for (const f of FIXUPS) {
  const changed: Record<string, number> = {}
  for (const rel of f.files.flatMap((x) => [x, `after/${x}`])) {
    const p = join(wireDir, rel)
    if (!existsSync(p)) continue
    const v = JSON.parse(readFileSync(p, 'utf8'))
    const n = f.apply(v)
    if (n > 0) { writeFileSync(p, JSON.stringify(v)); changed[rel] = n }
  }
  if (Object.keys(changed).length === 0) { console.error(`fixup ${f.id}: nothing to do -- stale rule, remove it`); process.exit(1) }
  applied.push({ id: f.id, upstream: f.upstream, why: f.why, changed })
}
console.log(JSON.stringify(applied))
