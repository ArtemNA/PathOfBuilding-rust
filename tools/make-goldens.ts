// Oracle goldens for the upstream of tools/upstream.json (R-UP). Twin of pob-ts `npm run oracle:goldens`
// (tools/oracle/make-goldens.ts), which writes into pob-ts (READ-ONLY for us) and reads POB_ROOT for the wasm boot.
// Here: the same pob-ts snapshot code (only imported/executed), the wasm oracle boots the Lua-5.4-loadable mirror
// (tools/upstream-mirror.ps1; `..=` desugared), golden XMLs come from the REAL upstream / pob-engine-wasm fixtures, and
// the canonical snapshots go to `goldensDir` of tools/upstream.json (builds/corpus/goldens). Strictly sequential (W12).
//
//   node tools/make-goldens.ts [--only <name>] [--verify]
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { CFG, POB, POB_WASM, ORACLE, POBTS, GOLDENS } from './upstream.ts'

const P = `file:///${POBTS}`
const { loadCase, serializeSnapshot } = await import(`${P}/tools/oracle/format.ts`)
const { GOLDEN_BUILDS } = await import(`${P}/tools/oracle/paths.ts`)
const { snapshotCase } = await import(`${P}/tools/oracle/snapshot.ts`)

const arg = (n: string) => { const i = process.argv.indexOf(n); return i >= 0 ? process.argv[i + 1] : undefined }
const only = arg('--only')
const verify = process.argv.includes('--verify')
mkdirSync(GOLDENS, { recursive: true })
let failures = 0
for (const b of GOLDEN_BUILDS as { name: string; xmlPath: (r: { pobRoot: string; oracleRoot: string }) => string }[]) {
  if (only && b.name !== only) continue
  const t = Date.now()
  try {
    const xml = readFileSync(b.xmlPath({ pobRoot: POB, oracleRoot: ORACLE }), 'utf8')
    const snap = await snapshotCase(loadCase(b.name, xml), xml, { roots: { pobRoot: POB_WASM, oracleRoot: ORACLE } })
    // the wasm boots the mirror, whose HEAD is the same sha; record the real upstream root for the reader
    const text = serializeSnapshot(snap)
    const out = join(GOLDENS, `${b.name}.json`)
    if (verify) {
      const same = readFileSync(out, 'utf8') === text
      if (!same) failures++
      console.log(`[${b.name}] ${same ? 'verified' : 'MISMATCH'} ${Date.now() - t} ms`)
    } else {
      writeFileSync(out, text)
      console.log(`[${b.name}] keys ${Object.keys(snap.stats).length} written ${Date.now() - t} ms`)
    }
  } catch (e) {
    failures++
    console.error(`[${b.name}] FAILED: ${e instanceof Error ? e.message : String(e)}`)
  }
}
console.log(`pobSha ${CFG.pobSha.slice(0, 9)}, ${failures} failure(s)`)
process.exit(failures ? 1 : 0)
