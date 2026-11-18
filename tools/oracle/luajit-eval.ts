// `luajitEval`: run a Lua chunk in desktop LuaJIT (the official PoB docker image) after the HeadlessWrapper boot
// (tools/oracle/lua/luajit-eval.lua) and hand back its `return` value, decoded from dkjson. Origin: pob-ts
// tools/oracle/luajit-eval.ts at pob-ts commit 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into pob-rust
// (R-INDEPENDENT-UPDATE A1) and owned here since. The Lua side is mounted from this repo's tools/oracle/lua.
import { execFileSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { resolve } from 'node:path'
import { LUAJIT_IMAGE } from './luajit.ts'
import { REPO_ROOT, type Roots } from './paths.ts'

export const LUA_DIR = resolve(REPO_ROOT, 'tools/oracle/lua')
const LUA_PATH_ENV = './?.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua;;'

export interface LuajitEvalOptions {
  roots: Roots
  image?: string
}

/**
 * One `docker run` per call (~5 s: image start + PoB boot), so batch work into one chunk. The chunk's `return` value comes
 * back through dkjson (numbers at `%.14g`, an empty table as `[]`, nan/inf unrepresentable -- transport them as strings).
 */
export function luajitEval<T = unknown>(code: string, opts: LuajitEvalOptions): T {
  const image = opts.image ?? LUAJIT_IMAGE
  const tmpDir = mkdtempSync(resolve(tmpdir(), 'pob-luajit-eval-'))
  try {
    writeFileSync(resolve(tmpDir, 'code.lua'), code)
    let stdout: string
    try {
      stdout = execFileSync(
        'docker',
        [
          'run', '--rm',
          '-v', `${opts.roots.pobRoot}:/workdir:ro`, '-v', `${LUA_DIR}:/bench:ro`, '-v', `${tmpDir}:/code:ro`,
          '-w', '/workdir/src', '-e', `LUA_PATH=${LUA_PATH_ENV}`, image,
          'luajit', '/bench/luajit-eval.lua', '/code/code.lua',
        ],
        { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
      )
    } catch (err) {
      const e = err as { stderr?: string; message?: string }
      throw new Error(`luajit eval failed: ${e.stderr ?? e.message ?? String(err)}`)
    }
    const lastLine = stdout.trim().split('\n').pop()
    if (!lastLine) throw new Error('luajit eval: empty stdout')
    const parsed = JSON.parse(lastLine) as { result?: unknown }
    return parsed.result as T
  } finally {
    rmSync(tmpDir, { recursive: true, force: true })
  }
}
