// R1.5: deterministic grammar generator for the Lua-pattern fuzz corpus (fixture B) and the bit.* corpus (fixture C).
// Strings are "byte strings" (every char code < 256). mulberry32 seed 0x1a7e5e11 (fuzz) / 0x1a7e5e12 (bit).
import { mulberry32 } from 'file:///C:/pob-work/pob-ts/tools/oracle/fuzz.ts'

export type Op = 'find' | 'findp' | 'match' | 'gmatch' | 'gsub_s' | 'gsub_t' | 'gsub_f'
export const OPS: Op[] = ['find', 'findp', 'match', 'gmatch', 'gsub_s', 'gsub_t', 'gsub_f']
/** extras: find/findp/match: [init|null]; gmatch: []; gsub_s: [repl, n|null]; gsub_t: [n|null]; gsub_f: [mode, n|null] */
export interface FuzzCase { op: Op; subject: string; pattern: string; extra: (string | number | null)[] }

const SPECIALS = '.%-+*?[]^$()'
const CLASSES = 'acdglpsuwx'
const lit = (c: string): string => (SPECIALS.includes(c) ? '%' + c : c)
const utf8 = (s: string): string => Buffer.from(s, 'utf8').toString('latin1')

export function genFuzzCases(modLines: string[], total = 21000): FuzzCase[] {
  const rnd = mulberry32(0x1a7e5e11)
  const pick = <T>(xs: readonly T[]): T => xs[Math.floor(rnd() * xs.length)]!
  const int = (lo: number, hi: number): number => lo + Math.floor(rnd() * (hi - lo + 1))
  const ALPHA = 'ab01 -.%()[]'
  const short = modLines.filter((l) => l.length <= 40)

  function subject(): string {
    const r = rnd()
    if (r < 0.06) return ''
    if (r < 0.62) return Array.from({ length: int(1, 12) }, () => pick([...ALPHA])).join('')
    if (r < 0.78) return pick(['abc', 'aaa', 'a1b2c3', 'ab ab', '(a)(b)', '[x]', 'a.b', '100%', 'hello world', '  x  ', '0123456789'])
    if (r < 0.92) return pick(short)
    return pick(['aé', utf8('aéb'), utf8('—a-'), utf8('é'), utf8('x—y é'), utf8('1é2'), '\xe9a', 'a\xff', utf8('é') + 'b01'])
  }

  function setAtom(): string {
    const items: string[] = []
    const neg = rnd() < 0.25
    const n = int(1, 4)
    if (rnd() < 0.15) items.push(']')
    for (let i = 0; i < n; i += 1) {
      const r = rnd()
      if (r < 0.3) items.push(lit(pick([...'ab01 .'])))
      else if (r < 0.55) items.push(pick(['a-z', '0-9', 'a-b', '0-1', 'A-Z', 'b-a']))
      else if (r < 0.85) items.push('%' + (rnd() < 0.3 ? pick([...CLASSES]).toUpperCase() : pick([...CLASSES])))
      else items.push(pick(['%]', '%-', '%^', '%.']))
    }
    let s = items.join('')
    if (rnd() < 0.15) s = '-' + s
    else if (rnd() < 0.15) s += '-'
    return '[' + (neg ? '^' : '') + s + ']'
  }
  function single(): string {
    const r = rnd()
    if (r < 0.4) return lit(pick([...'ab01 -.%()[]']))
    if (r < 0.62) return '%' + (rnd() < 0.3 ? pick([...CLASSES]).toUpperCase() : pick([...CLASSES]))
    if (r < 0.72) return '.'
    return setAtom()
  }

  // quantifier budget shared per pattern (<= 4, <= 3 for long subjects)
  function randomPattern(qMax: number): string {
    let q = 0
    let opened = 0
    const closed: number[] = []
    const quant = (a: string): string => (q < qMax && rnd() < 0.35 ? (q += 1, a + pick(['*', '+', '-', '?'])) : a)
    function seq(count: number, depth: number): string {
      let out = ''
      for (let i = 0; i < count; i += 1) {
        const r = rnd()
        if (r < 0.1 && depth < 2) { const k = ++opened; out += '(' + seq(int(1, 2), depth + 1) + ')'; closed.push(k) }
        else if (r < 0.13) { ++opened; closed.push(opened); out += '()' }
        else if (r < 0.18 && closed.length > 0) out += '%' + Math.min(3, pick(closed))
        else if (r < 0.22) out += '%b' + pick(['()', '[]', 'ab', '01', '..'])
        else if (r < 0.26) out += '%f' + pick(['[%w]', '[^%s]', '[%a]', '[^%w]', '[%s]', '[%d]'])
        else if (r < 0.28) out += pick(['^', '$'])
        else out += quant(single())
      }
      return out
    }
    return (rnd() < 0.2 ? '^' : '') + seq(int(1, 5), 0) + (rnd() < 0.2 ? '$' : '')
  }

  function derivedPattern(s: string, qMax: number): string {
    if (s.length === 0) return pick(['', 'a*', '^', '$', '()', '.-', '%s*'])
    const start = int(0, s.length - 1)
    const len = int(1, Math.min(6, s.length - start))
    const sub = s.slice(start, start + len)
    let q = 0
    let out = start === 0 && rnd() < 0.3 ? '^' : ''
    const capAt = rnd() < 0.4 ? int(0, len - 1) : -1
    const capEnd = capAt >= 0 ? int(capAt, len - 1) : -1
    for (let i = 0; i < len; i += 1) {
      const c = sub[i]!
      if (i === capAt) out += '('
      const r = rnd()
      let a: string
      if (r < 0.5) a = lit(c)
      else if (r < 0.7) a = /[a-z]/.test(c) ? pick(['%a', '%l', '%w', '%A']) : /\d/.test(c) ? pick(['%d', '%w', '%x']) : c === ' ' ? '%s' : /[!-/:-@[-`{-~]/.test(c) ? pick(['%p', '%W']) : '.'
      else if (r < 0.8) a = '.'
      else a = '[' + (rnd() < 0.5 ? '' : 'xy') + lit(c) + (rnd() < 0.5 ? 'a-b' : '') + ']'
      if (q < qMax && rnd() < 0.25) { q += 1; a += pick(['*', '+', '-', '?']) }
      out += a
      if (i === capEnd) out += ')'
    }
    if (start + len === s.length && rnd() < 0.3) out += '$'
    return out
  }

  const BROKEN = ['[abc', '(a', 'a%', '%b', '%4', ')', '%f', '[a-', '(a(b)', '%b(', 'a%1', '[%']
  function pattern(s: string, plain: boolean): string {
    if (plain) return rnd() < 0.7 ? s.slice(int(0, Math.max(0, s.length - 1)), int(0, s.length)) : Array.from({ length: int(0, 3) }, () => pick([...ALPHA])).join('')
    const qMax = s.length > 30 ? 3 : 4
    const r = rnd()
    let p: string
    if (r < 0.04) p = pick(['', 'a*', 'b*', '()', '^', '$', 'x-', '.-', '%s*', '[a-z]*', '(a*)', '%d*', '.*'])
    else p = rnd() < 0.5 ? derivedPattern(s, qMax) : randomPattern(qMax)
    if (rnd() < 0.05) p = rnd() < 0.5 ? p + pick(BROKEN) : pick(BROKEN)
    return p
  }

  const INITS = ['nil', 1, 2, 0, -1, -3, 'len', 'len+1', 'len+2', 100] as const
  const initFor = (s: string): number | null => {
    const v = pick(INITS)
    return v === 'nil' ? null : v === 'len' ? s.length : v === 'len+1' ? s.length + 1 : v === 'len+2' ? s.length + 2 : v
  }
  const REPLS = ['X', '', '%0', '%1', '%2', '%3', '%%', '%x', '%a', '[%0]', '<%1>%1', '%', '%0%0', 'a%1b']
  const NS = [null, 0, 1, 2]

  const cases: FuzzCase[] = []
  for (let i = 0; i < total; i += 1) {
    const op = OPS[i % OPS.length]!
    const s = subject()
    const p = pattern(s, op === 'findp')
    const extra: (string | number | null)[] =
      op === 'find' || op === 'findp' || op === 'match' ? [initFor(s)]
        : op === 'gmatch' ? []
          : op === 'gsub_s' ? [pick(REPLS), pick(NS)]
            : op === 'gsub_t' ? [pick(NS)]
              : [pick(['upper', 'nil', 'false']), pick(NS)]
    cases.push({ op, subject: s, pattern: p, extra })
  }
  return cases
}

// ---- bit.* corpus --------------------------------------------------------------------

export type BitOp = 'band' | 'bor' | 'bxor' | 'bnot' | 'tobit' | 'lshift' | 'rshift' | 'arshift'
export const BIT_OPS: BitOp[] = ['band', 'bor', 'bxor', 'bnot', 'tobit', 'lshift', 'rshift', 'arshift']
export const BIT_UNARY: BitOp[] = ['bnot', 'tobit']

/** Input values as numbers (NaN/Infinity allowed); `inputs` distinct-ish values, used for `a`, and paired with a shifted copy as `b`. */
export function genBitInputs(count = 1600): number[] {
  const rnd = mulberry32(0x1a7e5e12)
  const int = (lo: number, hi: number): number => lo + Math.floor(rnd() * (hi - lo + 1))
  const P53 = 2 ** 53
  const xs: number[] = [
    0, -0, 1, -1, 2, 31, 32, 33, 63, 64, 0x40000000, 0x7fffffff, 0x80000000, 0xffffffff, 0x100000000, 2 ** 31, 2 ** 32 - 1, 2 ** 40 + 3,
    2 ** 52, 2 ** 52 + 1, P53, -P53, P53 - 1, -(P53 - 1), -0x80000000, -0x80000001, -0x40000000, 1.5, 2.5, -1.5, -2.5, 0.5, -0.5, 3.5, 4294967295.5,
    0.49999999999999994, 2147483647.5, -2147483648.5, 1e300, -1e300, 1e10, NaN, Infinity, -Infinity, 5e-324, 2 ** 51 + 0.5, 2 ** 31 + 0.5,
  ]
  while (xs.length < count) {
    const r = rnd()
    if (r < 0.25) xs.push(int(-0x80000000, 0x7fffffff))
    else if (r < 0.45) xs.push(int(0, 0xffffffff))
    else if (r < 0.6) xs.push(int(-P53, P53))
    else if (r < 0.75) xs.push(int(-1000000, 1000000) + pick4(rnd))
    else if (r < 0.85) xs.push(int(0, 70))
    else xs.push((rnd() - 0.5) * 2 ** int(0, 62))
  }
  return xs
}
const pick4 = (rnd: () => number): number => [0, 0.5, 0.25, 0.75, 0.999999][Math.floor(rnd() * 5)]!
