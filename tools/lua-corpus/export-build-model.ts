// R3.2: snapshot pob-ts `loadBuildXml` (src/build/load.ts, checked against the oracle in pob-ts P4.1/P4.6) for the 6 goldens
// into crates/pob-build/tests/fixtures/<build>.model.json -- the reference the Rust `BuildModel` must equal field for field.
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-build-model.ts
// Canonical JSON: object keys sorted (UTF-16 order, = byte order for these ASCII keys), `undefined` dropped (as JSON.stringify),
// one-space indent, trailing newline -- two runs are byte-identical.
import '../upstream.ts'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadBuildXml } from 'file:///C:/pob-work/pob-ts/src/build/load.ts'
import { GOLDEN_BUILDS, resolveRoots } from 'file:///C:/pob-work/pob-ts/tools/oracle/paths.ts'

const HERE = dirname(fileURLToPath(import.meta.url))
const ROOT = resolve(HERE, '../..')
const OUT_DIR = resolve(ROOT, 'crates/pob-build/tests/fixtures')

function canon(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(canon)
  if (v !== null && typeof v === 'object') {
    const out: Record<string, unknown> = {}
    for (const k of Object.keys(v).sort()) {
      const x = (v as Record<string, unknown>)[k]
      if (x !== undefined) out[k] = canon(x)
    }
    return out
  }
  if (typeof v === 'number' && !Number.isFinite(v)) throw new Error(`non-finite number ${v} has no JSON form`)
  return v
}

const roots = resolveRoots()
mkdirSync(OUT_DIR, { recursive: true })
for (const golden of GOLDEN_BUILDS) {
  const xml = readFileSync(golden.xmlPath(roots), 'utf8')
  const model = loadBuildXml(xml)
  const text = JSON.stringify(canon(model), null, 1) + '\n'
  const out = resolve(OUT_DIR, `${golden.name}.model.json`)
  writeFileSync(out, text)
  console.log(`${golden.name}: ${text.length} bytes -> ${out}`)
}
