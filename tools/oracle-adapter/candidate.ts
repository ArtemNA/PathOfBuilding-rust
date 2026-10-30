// pob-rust candidate for the pob-ts oracle CLI: spawns `pob-cli calc - --json` and passes its output through untouched
// (numbers are NOT normalised here: -0 and exact doubles must survive, O2/O8). Types are local on purpose.
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

type Stats = Record<string, number | string | boolean>
interface OracleCase {
  mutations: unknown[]
}
interface CandidateResult {
  stats: Stats
  meta?: Record<string, unknown>
}

const root = dirname(dirname(dirname(fileURLToPath(import.meta.url))))

function binPath(): string {
  const bin = process.env.POB_RUST_CLI || join(root, 'target', 'release', 'pob-cli.exe')
  if (!existsSync(bin)) throw new Error(`pob-rust: binary not found: ${bin} (build it: cargo build --release -p pob-cli)`)
  return bin
}

export const candidate = {
  name: 'pob-rust',
  complete: false,
  async run(c: OracleCase, xml: string): Promise<CandidateResult> {
    if (c.mutations.length > 0) throw new Error('pob-rust: mutations are not supported yet (R0)')
    const r = spawnSync(binPath(), ['calc', '-', '--json'], { input: xml, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
    if (r.error) throw new Error(`pob-rust: cannot run pob-cli: ${r.error.message}`)
    if (r.status !== 0) throw new Error(`pob-rust: pob-cli exited with ${r.status}: ${r.stderr}`)
    const out = JSON.parse(r.stdout) as { stats: Stats; meta?: Record<string, unknown>; unimplemented?: string[] }
    console.error(`pob-rust unimplemented: ${out.unimplemented?.length ?? 0}`)
    return { stats: out.stats, ...(out.meta ? { meta: out.meta } : {}) }
  },
}
