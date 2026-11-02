// R-UP: re-key the hand-registered ModParser closures after an upstream update moved ModParser.lua lines.
// Every entry of crates/pob-parser/src/closures.rs (HELPERS) and closures_manual.rs (MANUAL) is `r("<src>:<from>", <to>, "<sha256 of the body>", ...)`;
// the sha identifies the body, so the new `<from>`/`<to>` are those of the one `fns` entry of data/mod-parser.json with that sha.
// A sha found 0 or > 1 times = the body changed upstream (or is ambiguous): the entry is reported and left as is -> re-port it by hand.
// Run from the pob-rust root (Node 24), after tools/sync-data.ps1:  node tools/modparser-gen/rekey.ts [--check]
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const FNS: Record<string, { src: string; from: number; to: number; sha: string }> = JSON.parse(
  readFileSync(`${ROOT}/data/mod-parser.json`, 'utf8'),
).fns
const bySha = new Map<string, string[]>()
for (const [k, v] of Object.entries(FNS)) bySha.set(v.sha, [...(bySha.get(v.sha) ?? []), k])
const check = process.argv.includes('--check')
let moved = 0
let bad = 0
for (const f of ['crates/pob-parser/src/closures.rs', 'crates/pob-parser/src/closures_manual.rs']) {
  const path = `${ROOT}/${f}`
  const text = readFileSync(path, 'utf8')
  const out = text.replace(/"((?:Modules\/\w+)\.lua:\d+)",(\s+)(\d+),(\s+)"([0-9a-f]{64})"/g, (all, id: string, s1: string, to: string, s2: string, sha: string) => {
    const keys = bySha.get(sha) ?? []
    if (keys.length !== 1) {
      bad++
      console.error(`${f}: ${id} (sha ${sha.slice(0, 12)}): ${keys.length} fns with this body -- changed upstream? re-port by hand`)
      return all
    }
    const nid = keys[0]!
    const nto = FNS[nid]!.to
    if (nid !== id || nto !== Number(to)) moved++
    return `"${nid}",${s1}${nto},${s2}"${sha}"`
  })
  if (!check && out !== text) writeFileSync(path, out)
}
console.log(`rekey: ${moved} entries moved${check ? ' (check only)' : ''}, ${bad} without a unique body`)
process.exit(bad ? 1 : 0)
