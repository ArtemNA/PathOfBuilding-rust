// The numeric corpora of R1.1 (tostring / string.format / tonumber / min-max-mod-modf-round-floor), deterministic. Origin: pob-ts
// test/lua/corpus.ts (`encodeNum` :129-136, `tostringCorpus` :175-202, `FmtCase` + `formatCorpus` :218-261, `tonumberCorpus` :315-344, `MathCase` + `mathCorpus` :371-389, SEED :62) at pob-ts commit
// 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into pob-rust (R-INDEPENDENT-UPDATE A4) and owned here since. Read by
// tools/lua-corpus/export-numeric.ts and perf-numeric.ts.
import { formatG } from './format-g.ts'
import { mulberry32 } from './fuzz.ts'

const SEED = 0x9e3779b9

/** A double as a token Lua's `dec()` turns back into the very same double (sign of zero and NaN included). */
export function encodeNum(v: number): string {
  if (Number.isNaN(v)) return formatG(v, 17) // "nan" / "-nan" by sign bit
  if (v === Infinity) return 'inf'
  if (v === -Infinity) return '-inf'
  if (Object.is(v, -0)) return '-0.0' // "-0" would come back as a Lua 5.4 integer and lose the sign
  return formatG(v, 17)
}

/** ≥ 3000 doubles: mantissa/exponent sweep, integers, powers of 2 and 10, 14-15 digit .5 ties, specials. */
export function tostringCorpus(): number[] {
  const rnd = mulberry32(SEED)
  const vs: number[] = [
    0, -0, Infinity, -Infinity, NaN, -NaN, Number.MAX_VALUE, Number.MIN_VALUE, -Number.MIN_VALUE,
    Number.EPSILON, Number.MIN_SAFE_INTEGER, Number.MAX_SAFE_INTEGER, 2 ** 53, 2 ** 53 + 2,
    1 / 3, 2 / 3, 0.1 + 0.2, 1e-320, -1e-320, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308,
    0.5, 2.5, 1.5, -2.5, 8.5, 4.5, 0.125, 1234567890122.5, 1234567890123.5, 12345678901234.5,
    12345678901235.5, 1e13 + 0.5, 123456789012345, 1234567890123456, 999999999999999, 99999999999999.99,
  ]
  for (let e = -20; e <= 20; e += 1) {
    vs.push(10 ** e, -(10 ** e), 10 ** e / 3, 9.999999999999999 * 10 ** e)
  }
  for (let k = -60; k <= 60; k += 1) vs.push(2 ** k, -(2 ** k), 2 ** k + 2 ** (k - 40))
  // .5 ties at 13..16 significant digits -- exactly the width where %.14g must round half-even
  for (let d = 12; d <= 15; d += 1) {
    for (let i = 0; i < 60; i += 1) {
      const base = Math.floor(rnd() * 10 ** d)
      vs.push(base + 0.5, -(base + 0.5), base + 0.25, base + 0.75)
    }
  }
  for (let i = 0; i < 900; i += 1) vs.push(Math.floor(rnd() * 1e16) * (rnd() < 0.5 ? -1 : 1))
  while (vs.length < 3200) {
    const e = Math.floor(rnd() * 41) - 20
    vs.push((rnd() < 0.5 ? -1 : 1) * rnd() * 10 ** e)
  }
  return vs
}

type FmtArg = { kind: 'n'; v: number } | { kind: 's'; v: string } | { kind: 'b'; v: boolean } | { kind: 'z' }
export interface FmtCase { fmt: string; arg: FmtArg }

const F_NUM = ['%.0f', '%.1f', '%.2f', '%.3f', '%f', '%5.2f', '%-9.3f', '%08.2f', '%+.2f']
const F_G = ['%g', '%.3g', '%.10g', '%.14g', '%12.4g', '%-12.4g']
const F_D = ['%d', '%i', '%-6d', '%03d', '%+d', '%8d', '%025d']
const F_X = ['%x', '%X', '%08x', '%-8x']
const F_S = ['%s', '%.3s', '%8s', '%-8s']

/** ≥ 600 (fmt, arg) pairs across %d %f %g %x %s on ties, randoms, non-finites and strings. */
export function formatCorpus(): FmtCase[] {
  const rnd = mulberry32(SEED ^ 0x5bf03635)
  const cs: FmtCase[] = []
  const nums = [
    0, -0, 2.5, 3.5, -2.5, 0.5, -0.4, 0.05, 2.675, 1 / 3, 3.99, -41.53, 3.14159, 1e15, 1e14, 1e20,
    -1e20, 2 ** 53, 123456789012345, 1234.5678, 0.0001234, 1e-7, 255, 4294967295, 4294967296, 42, -42,
    Infinity, -Infinity, NaN, -NaN, Number.MAX_VALUE, 5e-324,
  ]
  for (let i = 0; i < 70; i += 1) nums.push((rnd() < 0.5 ? -1 : 1) * rnd() * 10 ** (Math.floor(rnd() * 25) - 8))
  for (let i = 0; i < 40; i += 1) nums.push(Math.floor(rnd() * 1e13) + 0.5)
  for (const v of nums) {
    cs.push({ fmt: F_NUM[Math.floor(rnd() * F_NUM.length)]!, arg: { kind: 'n', v } })
    cs.push({ fmt: F_G[Math.floor(rnd() * F_G.length)]!, arg: { kind: 'n', v } })
    cs.push({ fmt: F_D[Math.floor(rnd() * F_D.length)]!, arg: { kind: 'n', v } })
    cs.push({ fmt: F_S[Math.floor(rnd() * F_S.length)]!, arg: { kind: 'n', v } })
  }
  for (const f of [...F_NUM, ...F_G, ...F_D, ...F_S]) {
    for (const v of [0, -0, 2.5, -2.5, 1e15, NaN, -NaN, Infinity, -Infinity, 1 / 3]) {
      cs.push({ fmt: f, arg: { kind: 'n', v } })
    }
  }
  for (const f of F_X) for (const v of [0, 1, 15, 255, 255.7, 4294967295, 4294967296, 2 ** 53, 1e15]) {
    cs.push({ fmt: f, arg: { kind: 'n', v } })
  }
  for (const f of F_S) {
    for (const s of ['', 'a', 'abc', 'abcdef', 'Skill:Arc', '100%', 'Adds 8 Passive Skills']) {
      cs.push({ fmt: f, arg: { kind: 's', v: s } })
    }
    cs.push({ fmt: f, arg: { kind: 'b', v: true } })
    cs.push({ fmt: f, arg: { kind: 'b', v: false } })
    cs.push({ fmt: f, arg: { kind: 'z' } })
  }
  return cs
}

/** ≥ 300 strings: valid, invalid and boundary forms (bytes are hex-encoded so NUL/whitespace survive). */
export function tonumberCorpus(): string[] {
  const rnd = mulberry32(SEED ^ 0x7f4a7c15)
  const ss = [
    '', ' ', '  ', '\t', '\n', '5abc', 'abc', 'inf', '-inf', 'nan', 'infinity', '0x', '1e', '--5', '5 5',
    '1,5', '.', '-.', '+.', '0b101', '1_000', '5\0', '\x005', '0x1p', '0x.p2', '0xg', 'e5', '1e+', '1e-',
    '1e308', '1e309', '-1e309', '1e999', '-1e999', '1e-999', '1e-308', '1e-320', '0x1p4', '0X1P-2',
    '.5', '5.', '-0', '-0.0', '0', '+5', '  5  ', '\t\n\v\f\r 5 \t', '00005', '5e-0', '+.5', '0x0p0',
    '0x10', '0xA.8', '0x.8', '0x8p-1', '0x1.8p1', '0x1.p2', '  0x10  ', '0xff', '0XFF', '0x7fffffff',
    '1', '-1', '3.14159', '2.675', '1/2', '1 ', ' 1', '1.', '.1', '1e0', '1E0', '1e00', '9007199254740993',
  ]
  const digits = '0123456789'
  while (ss.length < 340) {
    const r = rnd()
    if (r < 0.35) {
      let s = ''
      for (let i = 0, n = 1 + Math.floor(rnd() * 12); i < n; i += 1) s += digits.charAt(Math.floor(rnd() * 10))
      ss.push((rnd() < 0.3 ? '-' : '') + s + (rnd() < 0.5 ? '.' + s.slice(0, 3) : ''))
    } else if (r < 0.6) {
      ss.push(`${rnd() < 0.5 ? '-' : ''}${(rnd() * 10).toFixed(4)}e${rnd() < 0.5 ? '-' : '+'}${Math.floor(rnd() * 320)}`)
    } else if (r < 0.8) {
      let h = ''
      for (let i = 0, n = 1 + Math.floor(rnd() * 12); i < n; i += 1) h += '0123456789abcdef'.charAt(Math.floor(rnd() * 16))
      ss.push('0x' + h)
    } else {
      ss.push(String.fromCharCode(33 + Math.floor(rnd() * 90)) + Math.floor(rnd() * 1000))
    }
  }
  return ss
}

export interface MathCase { a: number; b: number; dec: number | undefined }

/** ≥ 500 (a, b, dec) triples over NaN/inf/-0/integers/fractions. */
export function mathCorpus(): MathCase[] {
  const rnd = mulberry32(SEED ^ 0x1b873593)
  const pool = [
    0, -0, 1, -1, 3, -3, 5, -5, 6, -6, 0.5, -0.5, 2.5, -2.5, 3.7, -3.7, 1 / 3, 0.1, 0.03, 2.675,
    1.99999, 12345.6789, 1e15, -1e15, 2 ** 53, 1e300, NaN, -NaN, Infinity, -Infinity, 5e-324, 1e-320,
  ]
  const decs = [undefined, undefined, undefined, -2, -1, 0, 1, 2, 3, 4, 6]
  const cs: MathCase[] = []
  for (const a of pool) for (const b of pool.slice(0, 14)) cs.push({ a, b, dec: decs[cs.length % decs.length] })
  while (cs.length < 560) {
    const a = (rnd() < 0.5 ? -1 : 1) * rnd() * 10 ** (Math.floor(rnd() * 18) - 6)
    const b = (rnd() < 0.5 ? -1 : 1) * rnd() * 10 ** (Math.floor(rnd() * 10) - 3)
    cs.push({ a, b, dec: decs[Math.floor(rnd() * decs.length)] })
  }
  return cs
}
