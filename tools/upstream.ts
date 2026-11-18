// The ONE place TS tools (probes in tools/lua-corpus, make-goldens, oracle adapter) learn which upstream PoB they
// target: tools/upstream.json (R-UP). Env vars override: POB_ROOT, POB_GOLDENS_DIR.
// Everything an update runs is ours or upstream's (R-INDEPENDENT-UPDATE): the LuaJIT harness (tools/oracle), the data converter
// (tools/data-convert), the goldens (tools/make-goldens.ts on LuaJIT). No pob-ts, no pob-engine-wasm, no WASM mirror.
// Importing this module also sets process.env.POB_ROOT when unset (child processes see the same config).
import { readFileSync } from 'node:fs'
import { isAbsolute, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const REPO = fileURLToPath(new URL('../', import.meta.url)).replace(/\\/g, '/').replace(/\/$/, '')
export const CFG = JSON.parse(readFileSync(`${REPO}/tools/upstream.json`, 'utf8')) as {
  pobRoot: string
  pobSha: string
  pobVersion: string
  goldensDir: string
}
const abs = (p: string) => (isAbsolute(p) ? p : resolve(REPO, p)).replace(/\\/g, '/')

/** Real upstream checkout: LuaJIT (docker / desktop lua51.dll) and Lua sources read from here. */
export const POB = abs(process.env.POB_ROOT ?? CFG.pobRoot)
export const GOLDENS = abs(process.env.POB_GOLDENS_DIR ?? CFG.goldensDir)
process.env.POB_ROOT ??= POB
