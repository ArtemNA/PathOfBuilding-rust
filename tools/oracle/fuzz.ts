// Deterministic PRNG of the probe corpora. Origin: pob-ts tools/oracle/fuzz.ts (mulberry32 only) at pob-ts commit
// 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into pob-rust (R-INDEPENDENT-UPDATE A1) and owned here since.

/** mulberry32: deterministic 32-bit PRNG (public domain). Returns a generator -> [0, 1). */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0
  return function (): number {
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}
