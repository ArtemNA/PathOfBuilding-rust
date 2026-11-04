// R3.5b: generator of Rust code for the `apply` closures of Modules/ConfigOptions.lua that R3.5 did not hand-port.
// Run from the pob-rust root (Node 24):  node tools/config-apply-gen/gen.ts
//
// Inputs: data/config-options.json (`vars[].apply` = {src, from, to, sha}), the upstream Lua (READ-ONLY checkout), and
// crates/pob-build/src/config/apply.rs (the hand-ported entries `e("<var>", ...)` -- never generated).
// Output: crates/pob-build/src/config/apply_gen.rs (one statement per line; rustfmt-skipped by its `mod` declaration), deterministic.
//
// The sha256 of every body is recomputed from the checkout (lines from..to, LF line ends) and must equal the json
// (otherwise abort: data drift). A body outside the grammar below is NOT translated approximately: the generator lists it
// and aborts unless it is one of the known hand-ported bodies (MANUAL_OK: the 8 `mapAffixDropDownFunction` vars, `enemyIsBoss`
// and `presetBossSkills`, which read `data.*` and `build.configTab.*`).
//
// Grammar (Lua 5.1 subset):
//   funcbody := 'function' '(' Name {',' Name} ')' block 'end'
//   stat     := 'local' Name '=' exp | Name '=' exp | 'if' exp 'then' block {'elseif' exp 'then' block} ['else' block] 'end'
//             | P ':' 'NewMod' '(' explist ')'          -- P = the 2nd or 3rd parameter (modList / enemyModList)
//             | build.configTab.varControls[String]:SetPlaceholder(exp, false)
//   exp      := nil | true | false | Number | String | Name | '(' exp ')' | tablecons | exp binop exp | unop exp
//             | m_min(a, b) | m_max(a, b) | type(x) | modLib.createMod(explist) | SkillType.Name
// Semantics (helpers in apply.rs, section "runtime of the generated closures"):
//   * NewMod's source (the 1st vararg) is a literal or a `..`, which is a string or raises;
//   * arithmetic coerces numeric strings and raises otherwise (L3/L18); `..` formats numbers with %.14g; `<`/`<=` compare
//     numbers or strings, raise across types; `a > b` is `b < a` (LuaJIT VM order, MINEFIELD-RUST R4);
//   * `and`/`or`/`not`/`if` use Lua truthiness (L4); every table constructor is a fresh table (R4);
//   * NewMod/createMod: vararg decoded as ModTools.lua:33-67 (a string 1st = source; tables = tags); order of mods = order
//     of the calls.
import { POB } from '../upstream.ts'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const UP = `${POB}/src/` // tools/upstream.json (R-UP)
const OUT_RS = `${ROOT}/crates/pob-build/src/config/apply_gen.rs`
const J = JSON.parse(readFileSync(`${ROOT}/data/config-options.json`, 'utf8'))
const HAND_RS = readFileSync(`${ROOT}/crates/pob-build/src/config/apply.rs`, 'utf8')
const HAND = new Set([...HAND_RS.matchAll(/\be\("(\w+)", \d+, \d+, /g)].map((m) => m[1]!))
const MANUAL_OK = new Set(['enemyIsBoss', 'presetBossSkills', 'MapPrefix1', 'MapPrefix2', 'MapPrefix3', 'MapPrefix4', 'MapSuffix1', 'MapSuffix2', 'MapSuffix3', 'MapSuffix4'])

class Reject extends Error {}
const reject = (why: string): never => { throw new Reject(why) }

// ------------------------------------------------------------------------------------------------ tokenizer
type Tok = { t: 'name' | 'num' | 'str' | 'op' | 'eof'; v: string; line: number; n?: number }
const KEYWORDS = new Set(['and', 'break', 'do', 'else', 'elseif', 'end', 'false', 'for', 'function', 'if', 'in', 'local', 'nil', 'not', 'or', 'repeat', 'return', 'then', 'true', 'until', 'while'])
function tokenize(src: string, line0: number): Tok[] {
  const out: Tok[] = []
  let i = 0
  let line = line0
  while (i < src.length) {
    const c = src[i]!
    if (c === '\n') { line++; i++; continue }
    if (/\s/.test(c)) { i++; continue }
    if (src.startsWith('--', i)) {
      const lb = /^--\[(=*)\[/.exec(src.slice(i))
      if (lb) {
        const close = `]${lb[1]}]`
        const e = src.indexOf(close, i)
        if (e < 0) reject('unterminated long comment')
        for (let k = i; k < e; k++) if (src[k] === '\n') line++
        i = e + close.length
      } else {
        while (i < src.length && src[i] !== '\n') i++
      }
      continue
    }
    if (/[A-Za-z_]/.test(c)) {
      const m = /^[A-Za-z_]\w*/.exec(src.slice(i))![0]
      out.push({ t: KEYWORDS.has(m) ? 'op' : 'name', v: m, line })
      i += m.length
      continue
    }
    if (/[0-9]/.test(c) || (c === '.' && /[0-9]/.test(src[i + 1] ?? ''))) {
      const m = /^(0[xX][0-9a-fA-F]+|[0-9]*\.?[0-9]+([eE][+-]?[0-9]+)?)/.exec(src.slice(i))![0]
      out.push({ t: 'num', v: m, line, n: Number(m) })
      i += m.length
      continue
    }
    if (c === '"' || c === "'") {
      let s = ''
      let k = i + 1
      for (;;) {
        const d = src[k]
        if (d === undefined || d === '\n') reject('unterminated string')
        if (d === c) break
        if (d === '\\') {
          const e = src[k + 1]!
          const map: Record<string, string> = { n: '\n', t: '\t', '\\': '\\', '"': '"', "'": "'" }
          if (!(e in map)) reject(`string escape \\${e}`)
          s += map[e]
          k += 2
          continue
        }
        s += d
        k++
      }
      out.push({ t: 'str', v: s, line })
      i = k + 1
      continue
    }
    const ops = ['...', '..', '==', '~=', '<=', '>=', '<', '>', '=', '+', '-', '*', '/', '%', '^', '#', '(', ')', '{', '}', '[', ']', ';', ':', ',', '.']
    const op = ops.find((o) => src.startsWith(o, i))
    if (!op) reject(`unexpected character ${JSON.stringify(c)}`)
    out.push({ t: 'op', v: op!, line })
    i += op!.length
  }
  out.push({ t: 'eof', v: '<eof>', line })
  return out
}

// ------------------------------------------------------------------------------------------------ AST
type Exp =
  | { k: 'nil'; line: number } | { k: 'bool'; v: boolean; line: number } | { k: 'num'; v: number; line: number } | { k: 'str'; v: string; line: number }
  | { k: 'name'; v: string; line: number }
  | { k: 'bin'; op: string; a: Exp; b: Exp; line: number } | { k: 'un'; op: string; a: Exp; line: number }
  | { k: 'call'; f: 'm_min' | 'm_max' | 'type' | 'createMod'; args: Exp[]; line: number }
  | { k: 'skilltype'; v: string; line: number }
  | { k: 'table'; fields: { key: string | null; v: Exp }[]; line: number }
type Stat =
  | { k: 'local'; name: string; e: Exp } | { k: 'assign'; name: string; e: Exp }
  | { k: 'if'; arms: { cond: Exp | null; body: Stat[] }[] }
  | { k: 'newmod'; list: string; args: Exp[]; line: number }
  | { k: 'setph'; v: string; e: Exp }

class Parser {
  p = 0
  toks: Tok[]
  constructor(toks: Tok[]) { this.toks = toks }
  peek(o = 0): Tok { return this.toks[this.p + o]! }
  next(): Tok { return this.toks[this.p++]! }
  is(v: string): boolean { const t = this.peek(); return t.t === 'op' && t.v === v }
  expect(v: string): Tok { const t = this.next(); if (!(t.t === 'op' && t.v === v)) reject(`expected '${v}' at line ${t.line}, got '${t.v}'`); return t }
  name(): string { const t = this.next(); if (t.t !== 'name') reject(`expected a name at line ${t.line}, got '${t.v}'`); return t.v }

  funcbody(): { params: string[]; body: Stat[] } {
    this.expect('function')
    this.expect('(')
    const params: string[] = []
    if (!this.is(')')) { params.push(this.name()); while (this.is(',')) { this.next(); params.push(this.name()) } }
    this.expect(')')
    const body = this.block()
    this.expect('end')
    return { params, body }
  }
  block(): Stat[] {
    const out: Stat[] = []
    for (;;) {
      const t = this.peek()
      if (t.t === 'eof' || (t.t === 'op' && ['end', 'else', 'elseif'].includes(t.v))) return out
      if (this.is(';')) { this.next(); continue }
      out.push(this.stat())
    }
  }
  stat(): Stat {
    const t = this.peek()
    if (t.t === 'op' && t.v === 'local') {
      this.next()
      if (this.is('function')) reject('local function')
      const name = this.name()
      this.expect('=')
      return { k: 'local', name, e: this.exp() }
    }
    if (t.t === 'op' && t.v === 'if') {
      this.next()
      const arms: { cond: Exp | null; body: Stat[] }[] = []
      const cond = this.exp()
      this.expect('then')
      arms.push({ cond, body: this.block() })
      for (;;) {
        if (this.is('elseif')) { this.next(); const c = this.exp(); this.expect('then'); arms.push({ cond: c, body: this.block() }); continue }
        if (this.is('else')) { this.next(); arms.push({ cond: null, body: this.block() }) }
        break
      }
      this.expect('end')
      return { k: 'if', arms }
    }
    if (t.t === 'name' && t.v === 'build') {
      this.next()
      for (const f of ['configTab', 'varControls']) { this.expect('.'); if (this.name() !== f) reject(`build... at line ${t.line}`) }
      this.expect('[')
      const v = this.next()
      if (v.t !== 'str') reject(`varControls[non-string] at line ${t.line}`)
      this.expect(']')
      this.expect(':')
      if (this.name() !== 'SetPlaceholder') reject(`varControls method at line ${t.line}`)
      this.expect('(')
      const args = this.explist(')')
      const notify = args[1]
      if (args.length !== 2 || notify?.k !== 'bool' || notify.v) reject(`SetPlaceholder with a notify at line ${t.line}`)
      return { k: 'setph', v: v.v, e: args[0]! }
    }
    if (t.t === 'name') {
      const n1 = this.peek(1)
      if (n1.t === 'op' && n1.v === '=') { this.next(); this.next(); return { k: 'assign', name: t.v, e: this.exp() } }
      if (n1.t === 'op' && n1.v === ':') {
        this.next(); this.next()
        const m = this.name()
        if (m !== 'NewMod') reject(`method call :${m}`)
        this.expect('(')
        const args = this.explist(')')
        return { k: 'newmod', list: t.v, args, line: t.line }
      }
    }
    return reject(`statement at line ${t.line} starting with '${t.v}'`)
  }
  explist(close: string): Exp[] {
    const args: Exp[] = []
    if (!this.is(close)) { args.push(this.exp()); while (this.is(',')) { this.next(); args.push(this.exp()) } }
    this.expect(close)
    return args
  }
  // lparser.c priorities: {left, right}
  static PRI: Record<string, [number, number]> = {
    '+': [6, 6], '-': [6, 6], '*': [7, 7], '/': [7, 7], '%': [7, 7], '^': [10, 9], '..': [5, 4],
    '==': [3, 3], '~=': [3, 3], '<': [3, 3], '<=': [3, 3], '>': [3, 3], '>=': [3, 3], 'and': [2, 2], 'or': [1, 1],
  }
  exp(limit = 0): Exp {
    let a: Exp
    const t = this.peek()
    if (t.t === 'op' && (t.v === 'not' || t.v === '-' || t.v === '#')) {
      this.next()
      a = { k: 'un', op: t.v, a: this.exp(8), line: t.line }
    } else a = this.simple()
    for (;;) {
      const o = this.peek()
      const pri = o.t === 'op' ? Parser.PRI[o.v] : undefined
      if (!pri || pri[0] <= limit) return a
      this.next()
      const b = this.exp(pri[1])
      a = { k: 'bin', op: o.v, a, b, line: o.line }
    }
  }
  simple(): Exp {
    const t = this.next()
    if (t.t === 'num') return { k: 'num', v: t.n!, line: t.line }
    if (t.t === 'str') return { k: 'str', v: t.v, line: t.line }
    if (t.t === 'op') {
      if (t.v === 'nil') return { k: 'nil', line: t.line }
      if (t.v === 'true' || t.v === 'false') return { k: 'bool', v: t.v === 'true', line: t.line }
      if (t.v === '(') { const e = this.exp(); this.expect(')'); return e }
      if (t.v === '{') return this.table(t.line)
      return reject(`expression at line ${t.line} starting with '${t.v}'`)
    }
    if (t.t !== 'name') return reject(`expression at line ${t.line}`)
    if (t.v === 'm_min' || t.v === 'm_max' || t.v === 'type') {
      this.expect('(')
      return { k: 'call', f: t.v, args: this.explist(')'), line: t.line }
    }
    if (t.v === 'modLib' && this.is('.')) {
      this.next()
      const f = this.name()
      if (f !== 'createMod') reject(`modLib.${f}`)
      this.expect('(')
      return { k: 'call', f: 'createMod', args: this.explist(')'), line: t.line }
    }
    if (t.v === 'SkillType' && this.is('.')) { this.next(); return { k: 'skilltype', v: this.name(), line: t.line } }
    if (this.is('(') || this.is('.') || this.is('[') || this.is(':')) reject(`call/index on '${t.v}' at line ${t.line}`)
    return { k: 'name', v: t.v, line: t.line }
  }
  table(line: number): Exp {
    const fields: { key: string | null; v: Exp }[] = []
    while (!this.is('}')) {
      const t = this.peek()
      const n1 = this.peek(1)
      if (t.t === 'name' && n1.t === 'op' && n1.v === '=') {
        this.next(); this.next()
        if (fields.some((f) => f.key === t.v)) reject(`duplicate key ${t.v}`)
        fields.push({ key: t.v, v: this.exp() })
      } else if (this.is('[')) {
        return reject('[k] = v field')
      } else {
        fields.push({ key: null, v: this.exp() })
      }
      if (this.is(',') || this.is(';')) this.next()
      else break
    }
    this.expect('}')
    return { k: 'table', fields, line }
  }
}

// ------------------------------------------------------------------------------------------------ emitter
const rustStr = (s: string): string => JSON.stringify(s) // ASCII-only bodies; JSON escapes are valid Rust escapes here
const f64 = (n: number): string => {
  if (!Number.isFinite(n)) reject('non-finite literal')
  const s = String(n)
  return /[.e]/.test(s) ? s : `${s}.0`
}
const MODTYPES: Record<string, string> = { BASE: 'Base', INC: 'Inc', MORE: 'More', FLAG: 'Flag', OVERRIDE: 'Override', LIST: 'List', MAX: 'Max', MIN: 'Min' }

type Ctx = { params: string[]; locals: Map<string, string>; valAssigned: boolean }
const isLit = (e: Exp): boolean => e.k === 'num' || e.k === 'str' || e.k === 'bool'

/** A scalar expression as Rust: `{ place: true }` = an `&LV`-typed place, else an owned `LV` expression. */
type Sc = { code: string; place: boolean }
const own = (s: Sc): string => (s.place ? `${s.code.replace(/^&/, "")}.clone()` : s.code)
const ref = (s: Sc): string => (s.place ? s.code : `&${s.code}`)
const opName = (e: Exp, cx: Ctx): string => (e.k === 'name' && (cx.params.includes(e.v) || cx.locals.has(e.v)) ? `local '${e.v}'` : '')

function scalar(e: Exp, cx: Ctx): Sc {
  switch (e.k) {
    case 'num': return { code: `ln(${f64(e.v)})`, place: false }
    case 'str': return { code: `ls(${rustStr(e.v)})`, place: false }
    case 'bool': return { code: `lb(${e.v})`, place: false }
    case 'nil': return reject('nil as a value')
    case 'name': {
      if (e.v === cx.params[0]) return { code: cx.valAssigned ? '&val' : 'v', place: true }
      const l = cx.locals.get(e.v)
      if (l) return { code: `&${l}`, place: true }
      return reject(`free name '${e.v}' (global or other parameter) at line ${e.line}`)
    }
    case 'bin': {
      const ar: Record<string, string> = { '+': 'Add', '-': 'Sub', '*': 'Mul', '/': 'Div', '%': 'Mod', '^': 'Pow' }
      if (ar[e.op]) {
        return { code: `arith(Ar::${ar[e.op]}, ${ref(scalar(e.a, cx))}, ${ref(scalar(e.b, cx))}, ${e.line}, ${rustStr(opName(e.a, cx))}, ${rustStr(opName(e.b, cx))})?`, place: false }
      }
      if (e.op === '..') {
        return { code: `concat(${ref(scalar(e.a, cx))}, ${ref(scalar(e.b, cx))}, ${e.line}, ${rustStr(opName(e.a, cx))}, ${rustStr(opName(e.b, cx))})?`, place: false }
      }
      if (e.op === 'and' || e.op === 'or') {
        const a = scalar(e.a, cx)
        const b = scalar(e.b, cx)
        return e.op === 'and'
          ? { code: `{ let t = ${own(a)}; if truthy(Some(&t)) { ${own(b)} } else { t } }`, place: false }
          : { code: `{ let t = ${own(a)}; if truthy(Some(&t)) { t } else { ${own(b)} } }`, place: false }
      }
      return { code: `lb(${cond(e, cx)})`, place: false }
    }
    case 'un': {
      if (e.op === 'not') return { code: `lb(${cond(e, cx)})`, place: false }
      if (e.op === '-') {
        if (e.a.k === 'num') return { code: `ln(${f64(-e.a.v)})`, place: false }
        return { code: `unm(${ref(scalar(e.a, cx))}, ${e.line}, ${rustStr(opName(e.a, cx))})?`, place: false }
      }
      return reject(`unary ${e.op}`)
    }
    case 'call': {
      if (e.f === 'm_min' || e.f === 'm_max') {
        if (e.args.length !== 2) reject(`${e.f} with ${e.args.length} arguments`)
        return { code: `${e.f === 'm_min' ? 'lmin' : 'lmax'}(${ref(scalar(e.args[0]!, cx))}, ${ref(scalar(e.args[1]!, cx))}, ${e.line})?`, place: false }
      }
      if (e.f === 'type') {
        if (e.args.length !== 1) reject('type with != 1 argument')
        return { code: `ltype(${ref(scalar(e.args[0]!, cx))})`, place: false }
      }
      return reject('createMod as a scalar')
    }
    default: return reject(`${e.k} as a scalar`)
  }
}

/** A Lua condition as a Rust `bool` (truthiness of the value; `and`/`or`/`not` compose on truthiness, L4). */
function cond(e: Exp, cx: Ctx): string {
  if (e.k === 'bin') {
    if (e.op === 'and') return `(${cond(e.a, cx)} && ${cond(e.b, cx)})`
    if (e.op === 'or') return `(${cond(e.a, cx)} || ${cond(e.b, cx)})`
    if (e.op === '==' || e.op === '~=') {
      const a = scalar(e.a, cx)
      const b = scalar(e.b, cx)
      return `${e.op === '~=' ? '!' : ''}leq(${ref(a)}, ${ref(b)})`
    }
    const cmp: Record<string, [string, boolean]> = { '<': ['lt', false], '<=': ['le', false], '>': ['lt', true], '>=': ['le', true] }
    if (cmp[e.op]) {
      const [f, swap] = cmp[e.op]!
      const a = ref(scalar(e.a, cx))
      const b = ref(scalar(e.b, cx))
      return `${f}(${swap ? b : a}, ${swap ? a : b}, ${e.line})?`
    }
  }
  if (e.k === 'un' && e.op === 'not') return `!${cond(e.a, cx)}`
  if (e.k === 'bool') return String(e.v)
  return `truthy(Some(${ref(scalar(e, cx))}))`
}

/** A Lua value stored in a table / mod value, as a Rust `Value`. */
function value(e: Exp, cx: Ctx): string {
  switch (e.k) {
    case 'num': return `Value::Num(${f64(e.v)})`
    case 'str': return `s(${rustStr(e.v)})`
    case 'bool': return `Value::Bool(${e.v})`
    case 'un': if (e.op === '-' && e.a.k === 'num') return `Value::Num(${f64(-e.a.v)})`; break
    case 'skilltype': return `Value::Num(skill_type_num(${rustStr(e.v)}))`
    case 'table': return `Value::table(${tableCons(e, cx)})`
    case 'call': if (e.f === 'createMod') return `cmod(${modArgs(e.args, cx, e.line)})`; break
    case 'bin': if (['==', '~=', '<', '<=', '>', '>='].includes(e.op)) return `Value::Bool(${cond(e, cx)})`; break
    default: break
  }
  return `lval(${ref(scalar(e, cx))})`
}

function tableCons(e: Exp, cx: Ctx): string {
  if (e.k !== 'table') return reject('not a table')
  for (const f of e.fields) if (f.v.k === 'nil') reject('nil table field')
  const keyed = `&[${e.fields.filter((f) => f.key !== null).map((f) => `(${rustStr(f.key!)}, ${value(f.v, cx)})`).join(', ')}]`
  const pos = e.fields.filter((f) => f.key === null)
  // positional fields fill the array part 1..n in order (no trailing call/vararg expansion in this grammar)
  if (pos.length) return `table_arr(vec![${pos.map((f) => value(f.v, cx)).join(', ')}], ${keyed})`
  return `table(${keyed})`
}

function tagCons(e: Exp, cx: Ctx): string {
  if (e.k !== 'table') return reject(`a non-table tag argument (${e.k})`)
  // `{ type = "Condition", var = "X" }` -> the `cond` helper (same fields; tag field order is not observable: formatTag sorts)
  if (e.fields.length === 2 && e.fields[0]!.key === 'type' && e.fields[0]!.v.k === 'str' && e.fields[0]!.v.v === 'Condition'
    && e.fields[1]!.key === 'var' && e.fields[1]!.v.k === 'str') return `cond(${rustStr(e.fields[1]!.v.v)})`
  return `Tag::new(${tableCons(e, cx)})`
}

/** `modLib.createMod(name, type, value, ...)`: the arguments of `nmod`/`cmod`, vararg decoded statically (ModTools.lua:33-67). */
function modArgs(args: Exp[], cx: Ctx, line: number): string {
  if (args.length < 3) reject(`createMod with ${args.length} arguments`)
  const [nameE, typeE, valE, ...rest] = args as [Exp, Exp, Exp, ...Exp[]]
  let name: string
  if (nameE.k === 'str') name = rustStr(nameE.v)
  else name = `&lua_tostring(${ref(scalar(nameE, cx))})`
  if (typeE.k !== 'str' || !MODTYPES[typeE.v]) reject(`mod type ${JSON.stringify(typeE)}`)
  const ty = `ModType::${MODTYPES[(typeE as { v: string }).v]}`
  let source = 'None'
  let i = 0
  if (rest[0] && rest[0].k === 'str') { source = `Some(${rustStr(rest[0].v)})`; i = 1 }
  else if (rest[0] && rest[0].k === 'bin' && rest[0].op === '..') { source = `Some(&lua_tostring(${ref(scalar(rest[0], cx))}))`; i = 1 }
  else if (rest[0] && rest[0].k !== 'table') reject(`non-literal vararg slot 1 at line ${line}`)
  if (rest.slice(i).some((x) => x.k !== 'table')) reject(`flags/keywordFlags or a non-table tag at line ${line}`)
  const tags = rest.slice(i).map((t) => tagCons(t, cx))
  return `${name}, ${ty}, ${value(valE, cx)}, ${source}, vec![${tags.join(', ')}]`
}

/** Drops one pair of parentheses around a whole condition (`if (a || b)` -> `if a || b`). */
function bare(c: string): string {
  if (!c.startsWith('(') || !c.endsWith(')')) return c
  let d = 0
  for (let i = 0; i < c.length; i++) {
    if (c[i] === '(') d++
    else if (c[i] === ')' && --d === 0 && i < c.length - 1) return c
  }
  return c.slice(1, -1)
}

function stats(body: Stat[], cx: Ctx, out: string[], ind: string): void {
  for (const s of body) {
    if (s.k === 'local') {
      if (cx.locals.has(s.name) || cx.params.includes(s.name)) reject(`shadowing local ${s.name}`)
      const rs = `l_${s.name.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`)}`
      out.push(`${ind}let ${rs}: LV = ${own(scalar(s.e, cx))};`)
      cx.locals.set(s.name, rs)
    } else if (s.k === 'assign') {
      if (s.name !== cx.params[0]) reject(`assignment to ${s.name}`)
      out.push(`${ind}val = ${own(scalar(s.e, cx))};`)
    } else if (s.k === 'if') {
      s.arms.forEach((a, i) => {
        if (a.cond === null) out.push(`${ind}} else {`)
        else out.push(`${ind}${i === 0 ? 'if' : '} else if'} ${bare(cond(a.cond, cx))} {`)
        const saved = new Map(cx.locals)
        stats(a.body, cx, out, ind + '    ')
        cx.locals = saved
      })
      out.push(`${ind}}`)
    } else if (s.k === 'setph') {
      out.push(`${ind}c.tab.set_control_placeholder(${rustStr(s.v)}, ${ref(scalar(s.e, cx))});`)
    } else {
      const which = cx.params.indexOf(s.list)
      if (which !== 1 && which !== 2) reject(`NewMod on '${s.list}' (not the modList/enemyModList parameter)`)
      out.push(`${ind}nmod(${which === 1 ? 'c.mod_list' : 'c.enemy_mod_list'}, ${modArgs(s.args, cx, s.line)});`)
    }
  }
}

function assigns(body: Stat[], name: string): boolean {
  return body.some((s) => (s.k === 'assign' && s.name === name) || (s.k === 'if' && s.arms.some((a) => assigns(a.body, name))))
}
function mentions(e: unknown, name: string): boolean {
  if (!e || typeof e !== 'object') return false
  const o = e as Record<string, unknown>
  if (o.k === 'name' && o.v === name) return true
  return Object.values(o).some((x) => (Array.isArray(x) ? x.some((y) => mentions(y, name)) : mentions(x, name)))
}

// ------------------------------------------------------------------------------------------------ main
const src = readFileSync(`${UP}Modules/ConfigOptions.lua`, 'utf8').replace(/\r\n/g, '\n').split('\n')
type Gen = { var: string; from: number; to: number; sha: string; fn: string; code: string }
const gens: Gen[] = []
const rejected: { var: string; why: string }[] = []
let total = 0
for (const v of J.vars) {
  if (!v.apply) continue
  total++
  const a = v.apply as { src: string; from: number; to: number; sha: string }
  if (a.src !== 'Modules/ConfigOptions.lua') throw new Error(`${v.var}: apply in ${a.src}`)
  const text = src.slice(a.from - 1, a.to).join('\n')
  const sha = createHash('sha256').update(text).digest('hex')
  if (sha !== a.sha) throw new Error(`${v.var}: sha of the checkout ${sha} != config-options.json ${a.sha} (data drift -- regenerate data first)`)
  if (HAND.has(v.var)) continue
  try {
    // the body starts at `function` after `apply =` on line `from` (or at the first `function` of the range)
    const m = /apply\s*=\s*function\b/.exec(text)
    const start = m ? m.index + m[0].length - 'function'.length : reject('no `apply = function` on the range (a shared local function)')
    let pre = text.slice(0, start)
    const line0 = a.from + (pre.match(/\n/g)?.length ?? 0)
    pre = text.slice(start)
    const p = new Parser(tokenize(pre, line0))
    const fb = p.funcbody()
    const rest = p.peek()
    if (!(rest.t === 'op' && rest.v === '}')) reject(`after the body: '${rest.v}'`)
    const params = fb.params
    if (params.length < 3 || params.length > 4) reject(`${params.length} parameters`)
    const valAssigned = assigns(fb.body, params[0]!)
    const cx: Ctx = { params, locals: new Map(), valAssigned }
    const lines: string[] = []
    if (valAssigned) lines.push('    let mut val: LV = v.clone();')
    stats(fb.body, cx, lines, '    ')
    const usesVal = valAssigned || mentions(fb.body, params[0]!)
    const usesC = lines.some((l) => l.includes('c.'))
    const fn = `a${a.from}_${v.var.replace(/[A-Z]/g, (c: string) => `_${c.toLowerCase()}`).replace(/^_/, '').replace(/__+/g, '_')}`
    const code = `/// \`ConfigOptions.lua:${a.from}-${a.to}\` \`${v.var}\`\nfn ${fn}(${usesVal ? 'v' : '_'}: &LV, ${usesC ? 'c' : '_'}: &mut ApplyCtx<'_>) -> LuaResult<()> {\n${lines.join('\n')}\n    Ok(())\n}\n`
    gens.push({ var: v.var, from: a.from, to: a.to, sha: a.sha, fn, code })
  } catch (e) {
    if (!(e instanceof Reject)) throw e
    rejected.push({ var: v.var, why: e.message })
  }
}
const unexpected = rejected.filter((r) => !MANUAL_OK.has(r.var))
if (unexpected.length) {
  for (const r of unexpected) console.error(`REJECT ${r.var}: ${r.why}`)
  throw new Error(`${unexpected.length} bodies outside the grammar (not in MANUAL_OK)`)
}
const missingManual = [...MANUAL_OK].filter((x) => !HAND.has(x) && !rejected.some((r) => r.var === x))
if (missingManual.length) throw new Error(`MANUAL_OK vars the generator now takes: ${missingManual.join(', ')}`)
if (rejected.length) console.log(`not generated, hand-port pending in apply.rs: ${rejected.map((r) => r.var).join(', ')}`)

const body = gens.map((g) => g.code).join('\n')
const used = (n: string): boolean => new RegExp(`(^|[^\\w:])${n}\\b`).test(body)
const helpers = ['Ar', 'LV', 'arith', 'cmod', 'concat', 'cond', 'lb', 'le', 'leq', 'lmax', 'lmin', 'ln', 'ls', 'lt', 'lval', 'ltype', 'nmod', 's',
  'skill_type_num', 'table', 'table_arr', 'unm'].filter((n) => n === 'LV' || used(n))
const state = ['lua_tostring', 'truthy'].filter(used)
const mods = ['LuaResult', 'ModType', 'Tag', 'Value'].filter((n) => n === 'LuaResult' || used(n))
const rs = `//! GENERATED by \`tools/config-apply-gen/gen.ts\` from \`data/config-options.json\` + Modules/ConfigOptions.lua -- do not edit.
//! ${gens.length} \`apply\` closures: every var with an \`apply\` that \`apply.rs\` does not hand-port. Grammar and semantics: the header
//! of the generator. Each entry carries \`ConfigOptions.lua:<from>-<to>\` and the sha256 of the body; \`registry_drift\` (test
//! \`config_registry.rs\`) checks all.
${used('Rc') ? '\nuse std::rc::Rc;\n' : ''}
use pob_mods::{${mods.join(', ')}};

use super::apply::{ApplyCtx, ApplyEntry, e, ${helpers.join(', ')}};${state.length ? `
use super::{${state.join(', ')}};` : ''}

/// The generated entries, in ConfigOptions.lua order.
pub static GENERATED: [ApplyEntry; ${gens.length}] = [
${gens.map((g) => `    e(${rustStr(g.var)}, ${g.from}, ${g.to}, ${rustStr(g.sha)}, ${g.fn}),`).join('\n')}
];

${body}`
// Not run through rustfmt (the declaration `#[rustfmt::skip] mod apply_gen;` in mod.rs): one statement per line, as emitted.
writeFileSync(OUT_RS, rs)
console.log(`apply closures: ${total} in config-options.json; hand-ported ${HAND.size}; generated ${gens.length}; manual (map affix) ${rejected.length}`)
