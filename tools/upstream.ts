// The ONE place TS tools (probes in tools/lua-corpus, make-goldens, oracle adapter) learn which upstream PoB they
// target: tools/upstream.json (R-UP). Env vars override: POB_ROOT, POB_GOLDENS_DIR.
// The LuaJIT harness is ours (tools/oracle, R-INDEPENDENT-UPDATE A1) and needs only POB; since A3/A4 no TS tool runs pob-ts or
// pob-engine-wasm code, so this module exports no root of theirs (`pobTsRoot` / `wasmMirror` stay in upstream.json only for the
// data converter, tools/sync-data.ps1 and tools/upstream-mirror.ps1, until it is ours too).
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
