// The ONE place TS tools (probes in tools/lua-corpus, make-goldens, oracle adapter) learn which upstream PoB they
// target: tools/upstream.json (R-UP). Env vars override: POB_ROOT, POB_WASM_ROOT, POB_ORACLE, POB_TS_ROOT, POB_GOLDENS_DIR.
// Importing this module also sets process.env.POB_ROOT / POB_ORACLE when unset, so pob-ts' own `resolveRoots()`
// (tools/oracle/paths.ts, read at call time) follows the same config.
import { readFileSync } from 'node:fs'
import { isAbsolute, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const REPO = fileURLToPath(new URL('../', import.meta.url)).replace(/\\/g, '/').replace(/\/$/, '')
export const CFG = JSON.parse(readFileSync(`${REPO}/tools/upstream.json`, 'utf8')) as {
  pobRoot: string
  pobSha: string
  pobVersion: string
  wasmMirror: string
  oracleRoot: string
  pobTsRoot: string
  goldensDir: string
}
const abs = (p: string) => (isAbsolute(p) ? p : resolve(REPO, p)).replace(/\\/g, '/')

/** Real upstream checkout: LuaJIT (docker / desktop lua51.dll) and Lua sources read from here. */
export const POB = abs(process.env.POB_ROOT ?? CFG.pobRoot)
/** Lua-5.4-loadable mirror (tools/upstream-mirror.ps1) for WASM consumers (pob-ts converter, pob-engine-wasm). */
export const POB_WASM = abs(process.env.POB_WASM_ROOT ?? CFG.wasmMirror)
export const ORACLE = abs(process.env.POB_ORACLE ?? CFG.oracleRoot)
export const POBTS = abs(process.env.POB_TS_ROOT ?? CFG.pobTsRoot)
export const GOLDENS = abs(process.env.POB_GOLDENS_DIR ?? CFG.goldensDir)
process.env.POB_ROOT ??= POB
process.env.POB_ORACLE ??= ORACLE
