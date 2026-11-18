// Timeless/Abyss jewel LUTs for desktop LuaJIT. HeadlessWrapper's Inflate is a stub, so the probes mount ready .bin files and
// shim io.open/NewFileSearch (tools/lua-corpus/export-timeless.ts JEWEL_SHIM). The bytes are the zlib stream of the upstream checkout
// itself: DataJewelFileLoader.lua concatenates <stem>.zip.part0, part1, ... (or a single <stem>.zip) verbatim and Inflate()s them;
// every such file is an RFC1950 zlib stream (0x78da), not a PK-ZIP archive.
// Origin: pob-engine-wasm src/{timelessJewelLuts,nodeJewelData}.mjs as pob-ts tools/oracle/luajit.ts used them (pob-ts commit
// 70911c1b629cfe6887795cb40356c6d298c1922e), rewritten once into pob-rust (R-INDEPENDENT-UPDATE A1) and owned here since.
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { inflateSync } from 'node:zlib'

/**
 * Jewel title -> LUT file stem. Modules/Data.lua names the eleven LUT types, Classes/PassiveSpec.lua maps conqueror ids to them,
 * Classes/TreeTab.lua supplies the Abyss jewel titles; DataJewelFileLoader removes spaces from the type name to form the stem.
 */
export const TIMELESS_JEWEL_LUTS: readonly { itemNames: readonly string[]; fileStem: string }[] = Object.freeze([
  { itemNames: ['Glorious Vanity'], fileStem: 'GloriousVanity' },
  { itemNames: ['Lethal Pride'], fileStem: 'LethalPride' },
  { itemNames: ['Brutal Restraint'], fileStem: 'BrutalRestraint' },
  { itemNames: ['Militant Faith'], fileStem: 'MilitantFaith' },
  { itemNames: ['Elegant Hubris'], fileStem: 'ElegantHubris' },
  { itemNames: ['Heroic Tragedy'], fileStem: 'HeroicTragedy' },
  { itemNames: ['Festering Vengeance', 'Abyss Tecrod'], fileStem: 'AbyssTecrod' },
  { itemNames: ['Extinguishing Grasp', 'Abyss Ulaman'], fileStem: 'AbyssUlaman' },
  { itemNames: ['Baleful Dominion', 'Abyss Kurgal'], fileStem: 'AbyssKurgal' },
  { itemNames: ['Destructive Aspiration', 'Abyss Amanamu'], fileStem: 'AbyssAmanamu' },
  { itemNames: ['Reclaimed Malevolence', 'Abyss Zorath'], fileStem: 'AbyssZorath' },
])

/** Each LUT the XML names, at most once, in the order above. */
export function findRequiredJewelLuts(xml: string): { itemNames: readonly string[]; fileStem: string }[] {
  return TIMELESS_JEWEL_LUTS.filter(({ itemNames }) => itemNames.some((name) => xml.includes(name)))
}

/** The compressed parts of a LUT in `<pobRoot>/src/Data/TimelessJewelData`, in part order. */
export function findCompressedJewelParts(pobRoot: string, fileStem: string): string[] {
  const dir = join(pobRoot, 'src', 'Data', 'TimelessJewelData')
  const single = join(dir, `${fileStem}.zip`)
  if (existsSync(single)) return [single]
  const prefix = `${fileStem}.zip.part`
  const numbered = readdirSync(dir)
    .filter((n) => n.startsWith(prefix) && /^\d+$/.test(n.slice(prefix.length)))
    .map((n) => ({ number: Number(n.slice(prefix.length)), path: join(dir, n) }))
    .sort((a, b) => a.number - b.number)
  numbered.forEach((e, i) => {
    if (e.number !== i) throw new Error(`${fileStem}: split zlib stream is missing .part${i}`)
  })
  return numbered.map((e) => e.path)
}

/** The inflated LUT bytes (what LuaJIT's DataJewelFileLoader would read) plus where they came from. */
export function unpackJewelData(pobRoot: string, fileStem: string): { bytes: Buffer; compressedBytes: number; compressedSha256: string; partCount: number } {
  const parts = findCompressedJewelParts(pobRoot, fileStem)
  if (parts.length === 0) throw new Error(`${fileStem}: no .zip or .zip.part0 source found`)
  const compressed = Buffer.concat(parts.map((p) => readFileSync(p)))
  let bytes: Buffer
  try {
    bytes = inflateSync(compressed)
  } catch (error) {
    throw new Error(`${fileStem}: node:zlib could not inflate ${parts.length} source part(s): ${(error as Error).message}`, { cause: error })
  }
  return {
    bytes,
    compressedBytes: compressed.length,
    compressedSha256: createHash('sha256').update(compressed).digest('hex'),
    partCount: parts.length,
  }
}
