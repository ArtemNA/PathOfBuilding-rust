// Where the LuaJIT harness finds its inputs. Origin: pob-ts tools/oracle/paths.ts + tools/convert/paths.ts at pob-ts commit
// 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into pob-rust (R-INDEPENDENT-UPDATE A1) and owned here since.
// Differences: roots come from tools/upstream.ts (tools/upstream.json + POB_ROOT), never from hard-coded defaults; the
// Lethal Pride golden XML is our copy in builds/corpus/ (was pob-engine-wasm test/fixtures), so no oracle root is needed.
import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { POB, REPO } from '../upstream.ts'

/** pob-rust root (tools/oracle/lua is mounted from here as /bench). */
export const REPO_ROOT = REPO

export interface Roots {
  pobRoot: string
}

/** The upstream checkout of tools/upstream.json (POB_ROOT overrides); it must hold src/Modules/Data.lua. */
export function resolvePobRoot(explicit?: string): string {
  const root = resolve(explicit ?? POB)
  if (!existsSync(resolve(root, 'src/Modules/Data.lua'))) {
    throw new Error(`PathOfBuilding checkout not found at '${root}' (expected src/Modules/Data.lua); see tools/upstream.json / POB_ROOT`)
  }
  return root
}

export function resolveRoots(explicit: Partial<Roots> = {}): Roots {
  return { pobRoot: resolvePobRoot(explicit.pobRoot) }
}

/** Upstream's own reference builds (spec/System/TestBuilds_spec.lua). */
export const UPSTREAM_TEST_BUILDS_REL = 'spec/TestBuilds/3.13'

export interface GoldenBuild {
  readonly name: string
  /** 'upstream' = PathOfBuilding/spec/TestBuilds/3.13; 'corpus' = builds/corpus of this repo. */
  readonly source: 'upstream' | 'corpus'
  readonly xmlPath: (roots: Roots) => string
}

const upstream = (name: string): GoldenBuild => ({
  name,
  source: 'upstream',
  xmlPath: (r) => resolve(r.pobRoot, UPSTREAM_TEST_BUILDS_REL, `${name}.xml`),
})

/** The golden set, in name order: five upstream test builds + the deterministic timeless-jewel build (Lethal Pride at node 61834). */
export const GOLDEN_BUILDS: readonly GoldenBuild[] = [
  upstream('Dual Savior'),
  upstream('Dual Wield Cospris CoC'),
  {
    name: 'Dual Wield Cospris CoC - Lethal Pride',
    source: 'corpus',
    xmlPath: () => resolve(REPO_ROOT, 'builds/corpus', 'Dual Wield Cospris CoC - Lethal Pride.xml'),
  },
  upstream('Generals Perforate Zerker'),
  upstream('Mirage Archer Toxic Rain'),
  upstream('OccVortex'),
]

/** Path of a golden build's XML by name (throws for an unknown name). */
export function goldenXmlPath(name: string, roots: Roots = resolveRoots()): string {
  const b = GOLDEN_BUILDS.find((x) => x.name === name)
  if (!b) throw new Error(`unknown golden build '${name}'`)
  return b.xmlPath(roots)
}
