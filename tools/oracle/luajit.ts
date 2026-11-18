// Desktop LuaJIT in the official PoB docker image. Origin: pob-ts tools/oracle/luajit.ts at pob-ts commit
// 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into pob-rust (R-INDEPENDENT-UPDATE A1) and owned here since.
// Trimmed to what pob-rust uses: the image name, the docker check and the timeless-jewel LUT cache (LUTs come from the
// upstream checkout itself, tools/oracle/jewel-luts.ts; the wasm-oracle snapshot path of pob-ts is not part of it).
import { execFileSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { findRequiredJewelLuts, unpackJewelData } from './jewel-luts.ts'
import { REPO_ROOT, type Roots } from './paths.ts'

export const LUAJIT_IMAGE = 'ghcr.io/pathofbuildingcommunity/pathofbuilding-tests:latest'

/** `docker info` succeeds (execFileSync, stdio ignored)? */
export function dockerAvailable(): boolean {
  try {
    execFileSync('docker', ['info'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

const LUT_CACHE_DIR = resolve(REPO_ROOT, 'tools/.scratch/luts')

/** LUT .bin files for the timeless jewels named in `xml` -> tools/.scratch/luts/<stem>.bin (gitignored). Always creates and returns the dir. */
export function ensureJewelLuts(xml: string, roots: Roots): string {
  mkdirSync(LUT_CACHE_DIR, { recursive: true })
  for (const { fileStem } of findRequiredJewelLuts(xml)) {
    writeFileSync(resolve(LUT_CACHE_DIR, `${fileStem}.bin`), unpackJewelData(roots.pobRoot, fileStem).bytes)
  }
  return LUT_CACHE_DIR
}
