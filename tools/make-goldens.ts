// Golden snapshots of the six golden builds for the upstream of tools/upstream.json, from desktop LuaJIT (the official PoB
// docker image; tools/oracle/snapshot.ts). Until R-INDEPENDENT-UPDATE A3 these came from the pob-engine-wasm oracle (Lua 5.4
// plus shims) through pob-ts code; LuaJIT is the reference (MINEFIELD O1), and nothing here needs pob-ts or pob-engine-wasm.
// Golden XMLs: tools/oracle/paths.ts GOLDEN_BUILDS (upstream spec/TestBuilds/3.13 + builds/corpus). Snapshots go to
// `goldensDir` of tools/upstream.json (builds/corpus/goldens; POB_GOLDENS_DIR overrides). Read by crates/pob-golden.
//
//   node tools/make-goldens.ts [--only <name>] [--verify]
import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { dockerAvailable } from './oracle/luajit.ts'
import { GOLDEN_BUILDS, resolveRoots } from './oracle/paths.ts'
import { loadCase, luajitSnapshot, serializeSnapshot } from './oracle/snapshot.ts'
import { CFG, GOLDENS } from './upstream.ts'

const arg = (n: string) => { const i = process.argv.indexOf(n); return i >= 0 ? process.argv[i + 1] : undefined }
const only = arg('--only')
const verify = process.argv.includes('--verify')
if (!dockerAvailable()) throw new Error('docker is not available (the LuaJIT oracle runs in the PoB test image)')
const roots = resolveRoots()
const pobSha = execFileSync('git', ['-C', roots.pobRoot, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
if (pobSha !== CFG.pobSha && !process.env.POB_ROOT) {
  throw new Error(`upstream checkout is at ${pobSha.slice(0, 9)}, tools/upstream.json says ${CFG.pobSha.slice(0, 9)}`)
}
mkdirSync(GOLDENS, { recursive: true })
let failures = 0
for (const b of GOLDEN_BUILDS) {
  if (only && b.name !== only) continue
  const t = Date.now()
  try {
    const xml = readFileSync(b.xmlPath(roots), 'utf8')
    const text = serializeSnapshot(luajitSnapshot(loadCase(b.name, xml), xml, { roots, pobSha }))
    const out = join(GOLDENS, `${b.name}.json`)
    if (verify) {
      const same = readFileSync(out, 'utf8') === text
      if (!same) failures++
      console.log(`[${b.name}] ${same ? 'verified' : 'MISMATCH'} ${Date.now() - t} ms`)
    } else {
      writeFileSync(out, text)
      console.log(`[${b.name}] written ${Date.now() - t} ms`)
    }
  } catch (e) {
    failures++
    console.error(`[${b.name}] FAILED: ${e instanceof Error ? e.message : String(e)}`)
  }
}
console.log(`pobSha ${pobSha.slice(0, 9)}, ${failures} failure(s)`)
process.exit(failures ? 1 : 0)
