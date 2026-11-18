// C `printf("%.<p>g", n)` done exactly (exact BigInt fraction, half-even on exact binary ties; `toPrecision`/`toFixed` round
// half-up on the decimal shorthand and are wrong here, MINEFIELD L1/L2). Origin: pob-ts src/lua/string.ts:1-111 (`formatG` and its
// helpers) at pob-ts commit 70911c1b629cfe6887795cb40356c6d298c1922e, copied once into pob-rust (R-INDEPENDENT-UPDATE A4) and owned
// here since: the probes format their Lua inputs and %.17g reads with it. NaN prints as "nan" (LuaJIT lj_strfmt, no sign).

const DV = new DataView(new ArrayBuffer(8))

/** Exact `n = ±mant * 2^exp2` (mant = 0 only for ±0; subnormals get exp2 = -1074). */
function bitsOf(n: number): { neg: boolean; mant: bigint; exp2: number } {
  DV.setFloat64(0, n)
  const hi = DV.getUint32(0)
  const lo = DV.getUint32(4)
  const neg = (hi & 0x80000000) !== 0
  const biased = (hi >>> 20) & 0x7ff
  const frac = (BigInt(hi & 0xfffff) << 32n) | BigInt(lo)
  if (biased === 0) return { neg, mant: frac, exp2: -1074 }
  return { neg, mant: frac | (1n << 52n), exp2: biased - 1075 }
}

const POW10: bigint[] = [1n]
function pow10(e: number): bigint {
  while (POW10.length <= e) POW10.push(POW10[POW10.length - 1]! * 10n)
  return POW10[e]!
}

/** |n| as the exact fraction num/den. */
function fraction(mant: bigint, exp2: number): { num: bigint; den: bigint } {
  if (exp2 >= 0) return { num: mant << BigInt(exp2), den: 1n }
  return { num: mant, den: 1n << BigInt(-exp2) }
}

/** sign(num/den - 10^e). */
function cmpPow10(num: bigint, den: bigint, e: number): number {
  const a = e < 0 ? num * pow10(-e) : num
  const b = e >= 0 ? den * pow10(e) : den
  return a < b ? -1 : a > b ? 1 : 0
}

/** Decimal exponent X with 10^X <= |n| < 10^(X+1) (log10 estimate, corrected exactly). */
function decExp(num: bigint, den: bigint, n: number): number {
  let x = Math.floor(Math.log10(Math.abs(n)))
  if (!Number.isFinite(x)) x = 0
  while (cmpPow10(num, den, x + 1) >= 0) x += 1
  while (cmpPow10(num, den, x) < 0) x -= 1
  return x
}

/** round_half_even(num / (den * 10^k)) -- the printf rounding rule, exact on binary ties. */
function scaledRound(num: bigint, den: bigint, k: number): bigint {
  const n = k < 0 ? num * pow10(-k) : num
  const d = k >= 0 ? den * pow10(k) : den
  const q = n / d
  const twice = (n % d) * 2n
  if (twice > d) return q + 1n
  if (twice < d) return q
  return q % 2n === 0n ? q : q + 1n
}

/**
 * LuaJIT's own number formatter (lj_strfmt) prints every NaN as "nan" -- no sign, whatever the
 * sign bit says, and no "+" for the `+` flag (LuaJIT docker, P3.6: tostring(-(0/0)) = "nan",
 * ("%+.2f"):format(0/0) = "nan"). C printf, and therefore the wasm oracle, would say "-nan"/"+nan".
 */
function nonFinite(n: number): string | undefined {
  if (Number.isNaN(n)) return 'nan'
  if (n === Infinity) return 'inf'
  if (n === -Infinity) return '-inf'
  return undefined
}

/**
 * C `printf("%.<precision>g", n)`: round to `precision` significant digits (half-even on an exact
 * tie), then exponential when `X < -4 || X >= precision` else fixed, trailing zeros and a bare
 * decimal point removed, exponent at least two digits. `precision < 1` means 1 (C).
 * Oracle wasm, 2026-09-03: `%.14g` of 12345678901234.5 = "12345678901234" and of 12345678901235.5 =
 * "12345678901236" (both exact ties, rounded to even), of 999999999999999 = "1e+15", of -0.0 = "-0".
 */
export function formatG(n: number, precision: number): string {
  const nf = nonFinite(n)
  if (nf !== undefined) return nf
  const p = precision < 1 ? 1 : precision
  const { neg, mant, exp2 } = bitsOf(n)
  const sign = neg ? '-' : ''
  if (mant === 0n) return sign + '0'
  const { num, den } = fraction(mant, exp2)
  let x = decExp(num, den, n)
  let digits = scaledRound(num, den, x - p + 1)
  if (digits >= pow10(p)) {
    digits = digits / 10n
    x += 1
  }
  const ds = digits.toString()
  if (x < -4 || x >= p) {
    const frac = ds.slice(1).replace(/0+$/, '')
    const e = Math.abs(x).toString().padStart(2, '0')
    return sign + ds.charAt(0) + (frac === '' ? '' : '.' + frac) + 'e' + (x < 0 ? '-' : '+') + e
  }
  if (x >= 0) {
    const frac = ds.slice(x + 1).replace(/0+$/, '')
    return sign + ds.slice(0, x + 1) + (frac === '' ? '' : '.' + frac)
  }
  const frac = ('0'.repeat(-x - 1) + ds).replace(/0+$/, '')
  return sign + '0' + (frac === '' ? '' : '.' + frac)
}
