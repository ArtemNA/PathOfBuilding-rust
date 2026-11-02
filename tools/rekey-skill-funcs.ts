// R-UP: re-key the skill-function registry (crates/pob-calc/src/skill_funcs/mod.rs `SKILL_FUNCS`) after an upstream update moved lines
// of Data/Skills/*.lua. Each entry `e("<path>", "<file>", <from>, <to>, "<sha256 of the body>", ...)` is matched with the closures.json
// record of the same `path`: same sha -> the new from/to are written; another sha (the body changed upstream) or no record -> reported,
// left as is (re-port by hand; tests/offence_cp.rs `skill_func_registry_matches_closures_json` stays red until then).
// Run from the pob-rust root (Node 24), after tools/sync-data.ps1:  node tools/rekey-skill-funcs.ts [--check]
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const FILE = `${ROOT}/crates/pob-calc/src/skill_funcs/mod.rs`
const recs: { path: string; src: string; from: number; to: number; sha256: string }[] = JSON.parse(
  readFileSync(`${ROOT}/data/closures.json`, 'utf8'),
).closures
const byPath = new Map(recs.map((r) => [r.path, r]))
const check = process.argv.includes('--check')
let moved = 0
let bad = 0
const text = readFileSync(FILE, 'utf8')
const out = text.replace(/e\("([^"]+)",(\s*)"([^"]+)",(\s*)(\d+),(\s*)(\d+),(\s*)"([0-9a-f]{64})"/g, (all, path: string, s1: string, file: string, s2: string, from: string, s3: string, to: string, s4: string, sha: string) => {
  const r = byPath.get(path)
  if (!r || r.sha256 !== sha || r.src !== `Data/Skills/${file}`) {
    bad++
    console.error(`${path}: ${r ? `body/src changed (${r.src}:${r.from}-${r.to} ${r.sha256.slice(0, 12)})` : 'not in closures.json'} -- re-port by hand`)
    return all
  }
  if (r.from !== Number(from) || r.to !== Number(to)) moved++
  return `e("${path}",${s1}"${file}",${s2}${r.from},${s3}${r.to},${s4}"${sha}"`
})
if (!check && out !== text) writeFileSync(FILE, out)
console.log(`rekey-skill-funcs: ${moved} entries moved${check ? ' (check only)' : ''}, ${bad} to re-port`)
process.exit(bad ? 1 : 0)
