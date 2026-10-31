// R5.3a: generator of Rust code for the ModParser closures of classes A/B (R5-ARCH §4).
// Run from the pob-rust root (Node 24):  node tools/modparser-gen/gen.ts
// Reads data/mod-parser.json `fns` + the upstream Lua (READ-ONLY checkout), parses each closure body with a Lua SUBSET
// grammar (README.md) and writes crates/pob-parser/src/closures_gen.rs (formatted with rustfmt) and
// KNOWLEDGE/drafts/R5-manual-closures.md (every body outside the grammar, with the reason -- never an approximate port).
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const UP = 'C:/pob-work/project_15/PathOfBuilding/src/'
const OUT_RS = `${ROOT}/crates/pob-parser/src/closures_gen.rs`
const OUT_MD = `${ROOT}/KNOWLEDGE/drafts/R5-manual-closures.md`
const J = JSON.parse(readFileSync(`${ROOT}/data/mod-parser.json`, 'utf8'))
const GLOBALS = JSON.parse(readFileSync(`${ROOT}/data/globals.json`, 'utf8'))
const FNS: Record<string, { src: string; from: number; to: number; sha: string }> = J.fns

// Hand-ported in helpers.rs and registered in closures.rs HELPERS (never generated).
const HAND = new Set([
  'Modules/ModTools.lua:33', 'Modules/ModParser.lua:13', 'Modules/ModParser.lua:18', 'Modules/ModParser.lua:19',
  'Modules/ModParser.lua:2012', 'Modules/ModParser.lua:2018', 'Modules/ModParser.lua:2047', 'Modules/ModParser.lua:2055',
  'Modules/ModParser.lua:2073', 'Modules/ModParser.lua:6455', 'Modules/ModParser.lua:6457',
])

class Reject extends Error {}
const reject = (why: string): never => { throw new Reject(why) }

// ---------------------------------------------------------------------------------------------------------------
// 1. references: how each definition is called (table) and what its upvalues are bound to
// ---------------------------------------------------------------------------------------------------------------
type Kind = 'special' | 'value' | 'jewel' | 'up'
const kinds = new Map<string, Set<Kind>>()
const owners = new Map<string, Set<string>>() // def -> defs whose up holds it
const ups = new Map<string, Map<string, Set<string>>>() // def -> upvalue name -> bindings
const refCount = new Map<string, number>()
const isRef = (v: any) => v && typeof v === 'object' && typeof v.fn === 'string'
function binding(u: any): string {
  if (isRef(u)) return `fn:${u.fn}`
  if (u && typeof u === 'object' && u.cfn) return 'cfn'
  if (u && typeof u === 'object' && typeof u.named === 'string') return `named:${u.named}`
  return 'lua'
}
function addRef(ref: any, kind: Kind, owner?: string) {
  const id = ref.fn as string
  ;(kinds.get(id) ?? kinds.set(id, new Set()).get(id)!).add(kind)
  if (kind !== 'up') refCount.set(id, (refCount.get(id) ?? 0) + 1)
  if (owner) (owners.get(id) ?? owners.set(id, new Set()).get(id)!).add(owner)
  const m = ups.get(id) ?? ups.set(id, new Map()).get(id)!
  for (const [name, u] of Object.entries(ref.up ?? {})) {
    ;(m.get(name) ?? m.set(name, new Set()).get(name)!).add(binding(u))
    if (isRef(u)) addRef(u, 'up', id)
  }
}
const TABLE_KIND: Record<string, Kind> = { specialModList: 'special', modTagList: 'value', preFlagList: 'value', jewelFuncList: 'jewel' }
for (const [tn, tb] of Object.entries<any>(J.tables)) {
  for (const v of Object.values<any>(tb)) {
    const ref = isRef(v) ? v : isRef(v?.func) ? v.func : null
    if (ref) {
      if (!TABLE_KIND[tn]) throw new Error(`closure in ${tn}`)
      addRef(ref, TABLE_KIND[tn])
    }
  }
}

// ---------------------------------------------------------------------------------------------------------------
// 2. lexer
// ---------------------------------------------------------------------------------------------------------------
type Tok = { k: 's' | 'n' | 'w' | 'p' | 'eof'; v: string; num?: number; line: number }
const KW = new Set(['and', 'break', 'do', 'else', 'elseif', 'end', 'false', 'for', 'function', 'if', 'in', 'local', 'nil', 'not', 'or', 'repeat', 'return', 'then', 'true', 'until', 'while'])
function lex(s: string, line0: number): Tok[] {
  const t: Tok[] = []
  let i = 0
  let line = line0
  const n = s.length
  while (i < n) {
    const c = s[i]
    if (c === '\n') { line++; i++; continue }
    if (/\s/.test(c)) { i++; continue }
    if (c === '-' && s[i + 1] === '-') {
      const m = /^--\[(=*)\[/.exec(s.slice(i, i + 40))
      if (m) {
        const e = s.indexOf(']' + m[1] + ']', i)
        const j = e < 0 ? n : e + m[1].length + 2
        line += (s.slice(i, j).match(/\n/g) ?? []).length
        i = j
      } else while (i < n && s[i] !== '\n') i++
      continue
    }
    if (c === '"' || c === "'") {
      let j = i + 1
      let out = ''
      while (j < n && s[j] !== c) {
        if (s[j] === '\n') reject('unterminated string')
        if (s[j] === '\\') {
          const e = s[j + 1]
          const simple: Record<string, string> = { n: '\n', t: '\t', r: '\r', a: '\x07', b: '\b', f: '\f', v: '\v', '\\': '\\', '"': '"', "'": "'" }
          if (e in simple) { out += simple[e]; j += 2; continue }
          const d = /^\d{1,3}/.exec(s.slice(j + 1, j + 4))
          if (d) {
            const code = Number(d[0])
            if (code > 127) reject('string escape of a non-ASCII byte')
            out += String.fromCharCode(code)
            j += 1 + d[0].length
            continue
          }
          reject(`string escape \\${e}`)
        }
        out += s[j]
        j++
      }
      t.push({ k: 's', v: out, line })
      i = j + 1
      continue
    }
    if (c === '[' && /^\[=*\[/.test(s.slice(i, i + 40))) reject('long string')
    let m
    if ((m = /^(0[xX][0-9a-fA-F]+|\d+\.?\d*(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?)/.exec(s.slice(i, i + 60)))) {
      t.push({ k: 'n', v: m[0], num: Number(m[0]), line })
      i += m[0].length
      continue
    }
    if ((m = /^[A-Za-z_][A-Za-z_0-9]*/.exec(s.slice(i, i + 100)))) { t.push({ k: 'w', v: m[0], line }); i += m[0].length; continue }
    const op = ['...', '..', '==', '~=', '<=', '>='].find((o) => s.startsWith(o, i))
    t.push({ k: 'p', v: op ?? c, line })
    i += (op ?? c).length
  }
  t.push({ k: 'eof', v: '<eof>', line })
  return t
}

// ---------------------------------------------------------------------------------------------------------------
// 3. parser (Lua 5.1 subset; lparser.c priorities)
// ---------------------------------------------------------------------------------------------------------------
type Ex =
  | { t: 'nil' } | { t: 'true' } | { t: 'false' } | { t: 'num'; v: number } | { t: 'str'; v: string } | { t: 'vararg' }
  | { t: 'name'; n: string }
  | { t: 'index'; o: Ex; k: Ex }
  | { t: 'call'; f: Ex; args: Ex[] }
  | { t: 'mcall'; o: Ex; m: string; args: Ex[] }
  | { t: 'table'; items: ({ pos: Ex } | { key: string; e: Ex })[] }
  | { t: 'bin'; op: string; a: Ex; b: Ex }
  | { t: 'un'; op: string; e: Ex }
  | { t: 'paren'; e: Ex }
type St =
  | { t: 'local'; names: string[]; exprs: Ex[] }
  | { t: 'assign'; target: Ex; e: Ex }
  | { t: 'if'; arms: { c: Ex; b: St[] }[]; els: St[] | null }
  | { t: 'callst'; e: Ex }
  | { t: 'return'; exprs: Ex[] }
type Fn = { params: string[]; vararg: boolean; body: St[]; endLine: number }

const BIN: Record<string, [number, number]> = {
  '+': [6, 6], '-': [6, 6], '*': [7, 7], '/': [7, 7], '%': [7, 7], '^': [10, 9], '..': [5, 4],
  '==': [3, 3], '~=': [3, 3], '<': [3, 3], '<=': [3, 3], '>': [3, 3], '>=': [3, 3], and: [2, 2], or: [1, 1],
}
const UNARY_PRIORITY = 8

class Parser {
  i = 0
  t: Tok[]
  constructor(t: Tok[]) { this.t = t }
  get cur() { return this.t[this.i] }
  is(v: string) { const c = this.cur; return (c.k === 'p' || c.k === 'w') && c.v === v }
  accept(v: string) { if (this.is(v)) { this.i++; return true } return false }
  expect(v: string) { if (!this.accept(v)) reject(`expected '${v}' near '${this.cur.v}'`) }
  name(): string {
    const c = this.cur
    if (c.k !== 'w' || KW.has(c.v)) reject(`expected a name near '${c.v}'`)
    this.i++
    return c.v
  }
  funcAt(): Fn {
    // `function [name[.name][:name]] (params) block end`
    this.expect('function')
    if (this.cur.k === 'w') { this.name(); while (this.accept('.')) this.name(); if (this.is(':')) reject('method definition') }
    this.expect('(')
    const params: string[] = []
    let vararg = false
    if (!this.is(')')) {
      do {
        if (this.accept('...')) { vararg = true; break }
        params.push(this.name())
      } while (this.accept(','))
    }
    this.expect(')')
    const body = this.block()
    const endLine = this.cur.line
    this.expect('end')
    return { params, vararg, body, endLine }
  }
  blockEnd() { return this.is('end') || this.is('else') || this.is('elseif') || this.cur.k === 'eof' }
  block(): St[] {
    const out: St[] = []
    while (!this.blockEnd()) {
      if (this.is('return')) {
        this.i++
        const exprs = this.blockEnd() || this.is(';') ? [] : this.exprList()
        this.accept(';')
        if (!this.blockEnd()) reject("'return' not at the end of a block")
        out.push({ t: 'return', exprs })
        break
      }
      out.push(this.stat())
      this.accept(';')
    }
    return out
  }
  stat(): St {
    const c = this.cur
    if (c.k === 'w') {
      switch (c.v) {
        case 'local': {
          this.i++
          if (this.is('function')) reject('local function')
          const names = [this.name()]
          while (this.accept(',')) names.push(this.name())
          const exprs = this.accept('=') ? this.exprList() : []
          return { t: 'local', names, exprs }
        }
        case 'if': {
          this.i++
          const arms: { c: Ex; b: St[] }[] = []
          const c0 = this.expr()
          this.expect('then')
          arms.push({ c: c0, b: this.block() })
          let els: St[] | null = null
          for (;;) {
            if (this.accept('elseif')) { const ci = this.expr(); this.expect('then'); arms.push({ c: ci, b: this.block() }) }
            else if (this.accept('else')) { els = this.block(); this.expect('end'); break }
            else { this.expect('end'); break }
          }
          return { t: 'if', arms, els }
        }
        case 'for': case 'while': case 'repeat': case 'do': case 'function': case 'break': case 'goto':
          reject(`statement '${c.v}'`)
      }
    }
    const e = this.suffixed()
    if (this.is('=') || this.is(',')) {
      if (this.is(',')) reject('multiple assignment')
      this.i++
      if (e.t !== 'name' && e.t !== 'index') reject('assignment target')
      const rhs = this.exprList()
      if (rhs.length !== 1) reject('assignment of several values')
      return { t: 'assign', target: e, e: rhs[0] }
    }
    if (e.t !== 'call' && e.t !== 'mcall') reject('expression statement')
    return { t: 'callst', e }
  }
  exprList(): Ex[] {
    const l = [this.expr()]
    while (this.accept(',')) l.push(this.expr())
    return l
  }
  expr(limit = 0): Ex {
    let e: Ex
    if (this.is('not') || this.is('-') || this.is('#')) {
      const op = this.cur.v
      this.i++
      e = { t: 'un', op, e: this.expr(UNARY_PRIORITY) }
    } else e = this.simple()
    for (;;) {
      const c = this.cur
      const op = (c.k === 'p' || c.k === 'w') && BIN[c.v] ? c.v : null
      if (!op || BIN[op][0] <= limit) break
      this.i++
      const b = this.expr(BIN[op][1])
      e = { t: 'bin', op, a: e, b }
    }
    return e
  }
  simple(): Ex {
    const c = this.cur
    if (c.k === 'n') { this.i++; return { t: 'num', v: c.num! } }
    if (c.k === 's') { this.i++; return { t: 'str', v: c.v } }
    if (c.k === 'w' && c.v === 'nil') { this.i++; return { t: 'nil' } }
    if (c.k === 'w' && c.v === 'true') { this.i++; return { t: 'true' } }
    if (c.k === 'w' && c.v === 'false') { this.i++; return { t: 'false' } }
    if (this.is('...')) { this.i++; return { t: 'vararg' } }
    if (this.is('{')) return this.table()
    if (this.is('function')) reject('nested function')
    return this.suffixed()
  }
  primary(): Ex {
    if (this.accept('(')) { const e = this.expr(); this.expect(')'); return { t: 'paren', e } }
    return { t: 'name', n: this.name() }
  }
  suffixed(): Ex {
    let e = this.primary()
    for (;;) {
      if (this.accept('.')) e = { t: 'index', o: e, k: { t: 'str', v: this.name() } }
      else if (this.accept('[')) { const k = this.expr(); this.expect(']'); e = { t: 'index', o: e, k } }
      else if (this.accept(':')) { const m = this.name(); e = { t: 'mcall', o: e, m, args: this.args() } }
      else if (this.is('(') || this.is('{') || this.cur.k === 's') e = { t: 'call', f: e, args: this.args() }
      else return e
    }
  }
  args(): Ex[] {
    if (this.cur.k === 's') { const v = this.cur.v; this.i++; return [{ t: 'str', v }] }
    if (this.is('{')) return [this.table()]
    this.expect('(')
    if (this.accept(')')) return []
    const l = this.exprList()
    this.expect(')')
    return l
  }
  table(): Ex {
    this.expect('{')
    const items: ({ pos: Ex } | { key: string; e: Ex })[] = []
    while (!this.is('}')) {
      if (this.is('[')) {
        this.i++
        const k = this.expr()
        this.expect(']')
        this.expect('=')
        if (k.t !== 'str') reject('table key that is not a string literal')
        items.push({ key: k.v, e: this.expr() })
      } else if (this.cur.k === 'w' && !KW.has(this.cur.v) && this.t[this.i + 1].v === '=' && this.t[this.i + 1].k === 'p') {
        const key = this.name()
        this.expect('=')
        items.push({ key, e: this.expr() })
      } else items.push({ pos: this.expr() })
      if (!this.accept(',') && !this.accept(';')) break
    }
    this.expect('}')
    const keys = items.filter((x) => 'key' in x).map((x: any) => x.key)
    if (new Set(keys).size !== keys.length) reject('duplicate key in a table constructor')
    return { t: 'table', items }
  }
}

// ---------------------------------------------------------------------------------------------------------------
// 4. code generation (every Lua value is a pob_mods::Value; errors are ParseError via `?`)
// ---------------------------------------------------------------------------------------------------------------
const MOD = 'fn:Modules/ModTools.lua:33'
const HELPER_CALLS: Record<string, { rust: string; arity: number; d: boolean }> = {
  'fn:Modules/ModParser.lua:13': { rust: 'first_to_upper_v', arity: 1, d: false },
  'fn:Modules/ModParser.lua:2018': { rust: 'deal_no_non_damage_type', arity: 2, d: false },
  'fn:Modules/ModParser.lua:2047': { rust: 'granted_extra_skill', arity: 3, d: true },
  'fn:Modules/ModParser.lua:2055': { rust: 'trigger_extra_skill', arity: 3, d: true },
  'fn:Modules/ModParser.lua:2073': { rust: 'extra_support', arity: 4, d: true },
}
const NAMED: Record<string, string> = { gemIdLookup: 'gem_id_lookup', conquerorList: 'conqueror_list', dmgTypes: 'dmg_types' }
const FLAG_GLOBALS: Record<string, string> = { ModFlag: 'mod_flag', KeywordFlag: 'keyword_flag', SkillType: 'skill_type' }

const rustStr = (s: string) => '"' + [...s].map((ch) => {
  if (ch === '\\') return '\\\\'
  if (ch === '"') return '\\"'
  const c = ch.codePointAt(0)!
  if (c < 0x20 || c === 0x7f) return `\\u{${c.toString(16)}}`
  return ch
}).join('') + '"'
function rustNum(n: number): string {
  if (!Number.isFinite(n)) reject('non-finite literal')
  let s = String(n).replace('e+', 'e')
  if (!/[.e]/.test(s)) s += '.0'
  return `${s}_f64`
}

type Var = { rust: string; decl: number }
type Gen = {
  id: string
  ups: Map<string, string> // name -> single binding
  scopes: Map<string, Var>[]
  params: Map<string, number>
  vararg: boolean
  nparams: number
  decls: { lua: string; assigned: boolean; reads: number }[]
  usesD: boolean
  usesF: boolean
  usesA: boolean
  calls: Set<string> // generated helper defs this body calls
  kind: 'special' | 'value'
}

function lookupVar(g: Gen, n: string): Var | null {
  for (let i = g.scopes.length - 1; i >= 0; i--) { const v = g.scopes[i].get(n); if (v) return v }
  return null
}
const what = (s: string) => rustStr(s)
function whatOf(g: Gen, e: Ex): string {
  if (e.t === 'name') {
    if (lookupVar(g, e.n)) return what(`local '${e.n}'`)
    if (g.ups.has(e.n)) return what(`upvalue '${e.n}'`)
    return what(`global '${e.n}'`)
  }
  if (e.t === 'index' && e.k.t === 'str') return what(`field '${e.k.v}'`)
  return '""'
}

// A name read as a value (locals/params/Lua upvalues).
function nameValue(g: Gen, n: string): string {
  const v = lookupVar(g, n)
  if (v) { g.decls[v.decl].reads++; return `${v.rust}.clone()` }
  const b = g.ups.get(n)
  if (b === 'lua') { g.usesF = true; return `upv(f, ${rustStr(n)})?` }
  if (b) reject(`upvalue '${n}' (${b}) used as a value`)
  reject(`global '${n}'`)
  return ''
}
// `&value` of an expression without a needless clone when it is a plain variable.
function ref(g: Gen, e: Ex): string {
  if (e.t === 'name') {
    const v = lookupVar(g, e.n)
    if (v) { g.decls[v.decl].reads++; return `&${v.rust}` }
  }
  return `&${ex(g, e)}`
}

// Does the expression produce several values in a multi position (call results, `...`)?
function isMulti(g: Gen, e: Ex): boolean {
  if (e.t === 'vararg') return true
  if (e.t === 'mcall' && e.m === 'gsub') return true
  if (e.t === 'call' && e.f.t === 'index' && e.f.o.t === 'name' && e.f.o.n === 'string' && e.f.k.t === 'str' && e.f.k.v === 'gsub' && !lookupVar(g, 'string')) return true
  return false
}
// Rust Vec<Value> of a multi expression.
function multi(g: Gen, e: Ex): string {
  if (e.t === 'vararg') { if (!g.vararg) reject("'...' outside a vararg function"); g.usesA = true; return `rest(a, ${g.nparams})` }
  return `{ let (s, n) = ${gsubParts(g, e)}; vec![s, n] }`
}
// Rust Vec<Value> of an expression list (Lua adjusts the last expression to all its values).
function list(g: Gen, es: Ex[]): string {
  if (es.length === 0) return 'Vec::new()'
  const last = es[es.length - 1]
  if (isMulti(g, last)) {
    const head = es.slice(0, -1).map((e) => ex(g, e))
    if (head.length === 0) return multi(g, last)
    return `{ let mut v = vec![${head.join(', ')}]; v.extend(${multi(g, last)}); v }`
  }
  return `vec![${es.map((e) => ex(g, e)).join(', ')}]`
}

// `&[...]` of an expression list (a slice argument).
function listSlice(g: Gen, es: Ex[]): string {
  if (es.length > 0 && isMulti(g, es[es.length - 1])) return `&${list(g, es)}`
  return `&[${es.map((e) => ex(g, e)).join(', ')}]`
}

// gsub: a Rust expr of (Value, Value) -- the string and the count
function gsubParts(g: Gen, e: Ex): string {
  let recv: string
  let args: Ex[]
  if (e.t === 'mcall') { recv = `self_str(${ref(g, e.o)}, ${whatOf(g, e.o)})?`; args = e.args }
  else if (e.t === 'call') { if (e.args.length < 1) reject('string.gsub without arguments'); recv = `check_str(${ref(g, e.args[0])}, 1, "gsub")?`; args = e.args.slice(1) }
  else return reject('gsub form')
  if (args.length !== 2) reject('gsub with other than (pattern, repl)')
  if (args[0].t !== 'str' || args[0].v.includes('()')) reject('gsub pattern that is not a literal without position captures')
  const pat = `check_str(${ref(g, args[0])}, 2, "gsub")?`
  const r = args[1]
  let repl: string
  if (r.t === 'index' && r.o.t === 'name' && r.o.n === 'string' && !lookupVar(g, 'string') && !g.ups.has('string') && r.k.t === 'str' && r.k.v === 'upper') repl = 'GsubRepl::Upper'
  else if (r.t === 'str') repl = `GsubRepl::Str(${rustStr(r.v)})`
  else return reject('gsub replacement that is not string.upper or a string literal')
  return `gsub_v(&${recv}, &${pat}, ${repl})?`
}

function ex(g: Gen, e: Ex): string {
  switch (e.t) {
    case 'nil': return 'Value::Nil'
    case 'true': return 'Value::Bool(true)'
    case 'false': return 'Value::Bool(false)'
    case 'num': return `Value::Num(${rustNum(e.v)})`
    case 'str': return `Value::str(${rustStr(e.v)})`
    case 'vararg': if (!g.vararg) reject("'...' outside a vararg function"); g.usesA = true; return `arg(a, ${g.nparams})`
    case 'paren': return ex(g, e.e)
    case 'name': return nameValue(g, e.n)
    case 'index': return index(g, e)
    case 'call': return call(g, e)
    case 'mcall': return mcall(g, e)
    case 'table': return table(g, e)
    case 'un': {
      if (e.op === 'not') return `Value::Bool(!${cond(g, e.e)})`
      if (e.op === '-') return `neg_v(${ref(g, e.e)}, ${whatOf(g, e.e)})?`
      return `len_v(${ref(g, e.e)}, ${whatOf(g, e.e)})?`
    }
    case 'bin': {
      const ARITH: Record<string, string> = { '+': 'Add', '-': 'Sub', '*': 'Mul', '/': 'Div', '%': 'Mod', '^': 'Pow' }
      if (ARITH[e.op]) return `arith(Arith::${ARITH[e.op]}, ${ref(g, e.a)}, ${ref(g, e.b)}, ${whatOf(g, e.a)}, ${whatOf(g, e.b)})?`
      if (e.op === '..') return `concat_v(${ref(g, e.a)}, ${ref(g, e.b)}, ${whatOf(g, e.a)}, ${whatOf(g, e.b)})?`
      if (['==', '~=', '<', '<=', '>', '>='].includes(e.op)) return `Value::Bool(${cond(g, e)})`
      if (e.op === 'and') return `{ let t = ${ex(g, e.a)}; if t.truthy() { ${ex(g, e.b)} } else { t } }`
      if (e.op === 'or') return `{ let t = ${ex(g, e.a)}; if t.truthy() { t } else { ${ex(g, e.b)} } }`
      return reject(`operator ${e.op}`)
    }
  }
}

// A Rust bool: Lua truthiness of the expression.
function cond(g: Gen, e: Ex): string {
  if (e.t === 'paren') return cond(g, e.e)
  if (e.t === 'true') return 'true'
  if (e.t === 'false' || e.t === 'nil') return 'false'
  if (e.t === 'un' && e.op === 'not') return `!${cond(g, e.e)}`
  if (e.t === 'bin') {
    if (e.op === 'and') return `(${cond(g, e.a)} && ${cond(g, e.b)})`
    if (e.op === 'or') return `(${cond(g, e.a)} || ${cond(g, e.b)})`
    if (e.op === '==') return `lua_eq(${ref(g, e.a)}, ${ref(g, e.b)})`
    if (e.op === '~=') return `!lua_eq(${ref(g, e.a)}, ${ref(g, e.b)})`
    if (e.op === '<') return `lt_v(${ref(g, e.a)}, ${ref(g, e.b)})?`
    if (e.op === '<=') return `le_v(${ref(g, e.a)}, ${ref(g, e.b)})?`
    // `a > b` is `b < a` with a evaluated first (LuaJIT swaps the operands of the comparison)
    if (e.op === '>') return `{ let x = ${ex(g, e.a)}; let y = ${ex(g, e.b)}; lt_v(&y, &x)? }`
    if (e.op === '>=') return `{ let x = ${ex(g, e.a)}; let y = ${ex(g, e.b)}; le_v(&y, &x)? }`
  }
  if (e.t === 'name') {
    const v = lookupVar(g, e.n)
    if (v) { g.decls[v.decl].reads++; return `${v.rust}.truthy()` }
  }
  return `${ex(g, e)}.truthy()`
}

function index(g: Gen, e: Ex & { t: 'index' }): string {
  const o = e.o
  if (o.t === 'name' && !lookupVar(g, o.n)) {
    const b = g.ups.get(o.n)
    if (!b && FLAG_GLOBALS[o.n]) {
      if (e.k.t === 'str') return `global_flag(GlobalFlags::${o.n}, ${rustStr(e.k.v)})`
      return `global_flag_v(GlobalFlags::${o.n}, ${ref(g, e.k)})`
    }
    if (b?.startsWith('named:')) {
      const t = b.slice(6)
      if (!NAMED[t]) reject(`named table ${t}`)
      g.usesD = true
      return `named_${NAMED[t]}(d, ${ref(g, e.k)})?`
    }
    if (!b && (o.n === 'string' || o.n === 'math')) reject(`${o.n}.* as a value`)
  }
  return `index_v(${ref(g, o)}, ${ref(g, e.k)}, ${whatOf(g, o)})?`
}

// Static type of a vararg of createMod for its heuristic (ModTools.lua:37-49).
function staticType(g: Gen, e: Ex): 'nil' | 'num' | 'str' | 'table' | '?' {
  switch (e.t) {
    case 'nil': return 'nil'
    case 'num': return 'num'
    case 'str': return 'str'
    case 'table': return 'table'
    case 'index':
      if (e.o.t === 'name' && FLAG_GLOBALS[e.o.n] && !lookupVar(g, e.o.n) && !g.ups.has(e.o.n) && e.k.t === 'str') {
        return typeof GLOBALS[e.o.n]?.[e.k.v] === 'number' ? 'num' : 'nil'
      }
      return '?'
    case 'call': return e.f.t === 'name' && g.ups.get(e.f.n) === 'cfn' && e.f.n === 'bor' ? 'num' : '?'
  }
  return '?'
}
// `mod(n, t, v, ...)` with a literal nil in the tag part of `...` and a value after it builds a tag after a hole (L9
// orphan): the static ModParser entries carry those beside the mods (RawParse.orphan_tags), a closure result cannot.
function holeCheck(g: Gen, va: Ex[]) {
  const ty = va.map((x) => staticType(g, x))
  let start = 0
  for (const [slot, want] of [[0, 'str'], [1, 'num'], [2, 'num']] as const) {
    if (slot >= ty.length) break
    if (ty[slot] === want) start = slot + 1
    else if (ty[slot] === '?') { start = -1; break }
  }
  const from = start < 0 ? 0 : start
  for (let k = from; k < ty.length - 1; k++) {
    if (ty[k] === 'nil') reject('mod()/flag() with a literal nil before a later argument in the tag part (L9 orphan tag; a closure result cannot carry RawParse.orphan_tags)')
  }
}

function call(g: Gen, e: Ex & { t: 'call' }): string {
  const f = e.f
  if (f.t === 'name' && !lookupVar(g, f.n)) {
    const b = g.ups.get(f.n)
    if (b === MOD) { holeCheck(g, e.args.slice(3)); return `call_mod(${list(g, e.args)})?` }
    if (b === 'fn:Modules/ModParser.lua:2012') { holeCheck(g, e.args.slice(1)); return `call_flag(${list(g, e.args)})?` }
    if (b && HELPER_CALLS[b]) {
      const h = HELPER_CALLS[b]
      if (h.d) g.usesD = true
      const dArg = h.d ? 'd, ' : ''
      if (e.args.length <= h.arity && !(e.args.length > 0 && isMulti(g, e.args[e.args.length - 1]))) {
        const as = e.args.map((x) => ref(g, x))
        while (as.length < h.arity) as.push('&Value::Nil')
        return `${h.rust}(${dArg}${as.join(', ')})?`
      }
      const n = Array.from({ length: h.arity }, (_, i) => `&arg(&v, ${i})`).join(', ')
      return `{ let v = ${list(g, e.args)}; ${h.rust}(${dArg}${n})? }`
    }
    if (b === 'cfn' && f.n === 'bor') return `bor_v(${listSlice(g, e.args)})?`
    if (b?.startsWith('fn:')) {
      const target = b.slice(3)
      g.calls.add(target)
      g.usesD = true
      g.usesF = true
      return `${fnName(target)}(d, up_fn(f, ${rustStr(f.n)})?, ${listSlice(g, e.args)})?`
    }
    if (b) reject(`call of upvalue '${f.n}' (${b})`)
    if (f.n === 'tonumber') {
      if (e.args.length !== 1 || isMulti(g, e.args[0])) reject('tonumber with other than one argument')
      return `tonumber(${ref(g, e.args[0])})`
    }
    reject(`call of global '${f.n}'`)
  }
  if (f.t === 'index' && f.o.t === 'name' && !lookupVar(g, f.o.n) && !g.ups.has(f.o.n) && f.k.t === 'str') {
    const lib = `${f.o.n}.${f.k.v}`
    if (lib === 'math.floor') {
      if (e.args.length !== 1 || isMulti(g, e.args[0])) reject('math.floor arity')
      return `math_floor(${ref(g, e.args[0])})?`
    }
    if (lib === 'math.min' || lib === 'math.max') return `math_${f.k.v}(${listSlice(g, e.args)})?`
    if (lib === 'string.gsub') return `${gsubParts(g, e)}.0`
    reject(`call of ${lib}`)
  }
  return reject('call of a non-name')
}

function mcall(g: Gen, e: Ex & { t: 'mcall' }): string {
  switch (e.m) {
    case 'gsub': return `${gsubParts(g, e)}.0`
    case 'lower': case 'upper':
      if (e.args.length) reject(`:${e.m} with arguments`)
      return `Value::str(&pob_lua::string::${e.m}(&self_str(${ref(g, e.o)}, ${whatOf(g, e.o)})?))`
    case 'sub': {
      if (e.args.length < 1 || e.args.length > 2 || isMulti(g, e.args[e.args.length - 1])) reject(':sub arity')
      const s = `self_str(${ref(g, e.o)}, ${whatOf(g, e.o)})?`
      const i = ref(g, e.args[0])
      const j = e.args.length === 2 ? ref(g, e.args[1]) : '&Value::Nil'
      return `str_sub(&${s}, ${i}, ${j})?`
    }
  }
  return reject(`method :${e.m}`)
}

function table(g: Gen, e: Ex & { t: 'table' }): string {
  const items = e.items
  if (items.length === 0) return 'Value::table(Table::default())'
  if (items.every((x) => 'key' in x)) {
    return `tbl([${items.map((x: any) => `(${rustStr(x.key)}, ${ex(g, x.e)})`).join(', ')}])`
  }
  if (items.every((x) => 'pos' in x)) return `seq(${list(g, items.map((x: any) => x.pos))})`
  // mixed: Lua evaluates the fields in source order
  const parts: string[] = []
  items.forEach((x: any, i) => {
    if ('key' in x) parts.push(`t.key(${rustStr(x.key)}, ${ex(g, x.e)});`)
    else if (i === items.length - 1 && isMulti(g, x.pos)) parts.push(`t.extend(${multi(g, x.pos)});`)
    else parts.push(`t.push(${ex(g, x.pos)});`)
  })
  return `{ let mut t = TableBuilder::default(); ${parts.join(' ')} t.build() }`
}

function declare(g: Gen, lua: string): Var {
  const decl = g.decls.length
  g.decls.push({ lua, assigned: false, reads: 0 })
  const v = { rust: `@@V${decl}@@`, decl }
  g.scopes[g.scopes.length - 1].set(lua, v)
  return v
}

// Reads of local `n` anywhere in `st` other than as `n[k] = v` targets / `return n` (alias check for in-place writes).
function aliasReads(body: St[], n: string): number {
  let count = 0
  const walkE = (e: Ex | undefined): void => {
    if (!e) return
    switch (e.t) {
      case 'name': if (e.n === n) count++; return
      case 'index': walkE(e.o); walkE(e.k); return
      case 'call': walkE(e.f); e.args.forEach(walkE); return
      case 'mcall': walkE(e.o); e.args.forEach(walkE); return
      case 'table': e.items.forEach((x: any) => walkE('pos' in x ? x.pos : x.e)); return
      case 'bin': walkE(e.a); walkE(e.b); return
      case 'un': case 'paren': walkE(e.e); return
    }
  }
  const walkS = (s: St): void => {
    switch (s.t) {
      case 'local': s.exprs.forEach(walkE); return
      case 'assign':
        if (s.target.t === 'index' && s.target.o.t === 'name' && s.target.o.n === n) { walkE(s.target.k) } else walkE(s.target)
        walkE(s.e)
        return
      case 'if': s.arms.forEach((a) => { walkE(a.c); a.b.forEach(walkS) }); s.els?.forEach(walkS); return
      case 'callst': walkE(s.e); return
      case 'return': if (!(s.exprs.length === 1 && s.exprs[0].t === 'name' && s.exprs[0].n === n)) s.exprs.forEach(walkE); return
    }
  }
  body.forEach(walkS)
  return count
}

function stmts(g: Gen, body: St[], tail: boolean, fnBody: St[]): string {
  g.scopes.push(new Map())
  const out: string[] = []
  body.forEach((s, i) => {
    const last = i === body.length - 1
    switch (s.t) {
      case 'local': {
        let vals: string[]
        if (s.names.length === 1 && s.exprs.length <= 1 && !(s.exprs.length === 1 && isMulti(g, s.exprs[0]))) {
          vals = [s.exprs.length ? ex(g, s.exprs[0]) : 'Value::Nil']
          const v = declare(g, s.names[0])
          out.push(`let @@M${v.decl}@@${v.rust} = ${vals[0]};`)
        } else {
          const l = list(g, s.exprs)
          const vs = s.names.map((n) => declare(g, n))
          out.push(`let lv = ${l};`)
          vs.forEach((v, k) => out.push(`let @@M${v.decl}@@${v.rust} = arg(&lv, ${k});`))
        }
        break
      }
      case 'assign': {
        if (s.target.t === 'name') {
          const v = lookupVar(g, s.target.n)
          if (!v) reject(`assignment to non-local '${s.target.n}'`)
          const val = ex(g, s.e)
          g.decls[v!.decl].assigned = true
          out.push(`${v!.rust} = ${val};`)
        } else if (s.target.t === 'index' && s.target.o.t === 'name' && lookupVar(g, s.target.o.n)) {
          // `t[k] = v` on a local table: only when nothing else can see the table (the write is then a key check)
          const n = s.target.o.n
          if (aliasReads(fnBody, n) > 0) reject(`write into local table '${n}' that is read elsewhere`)
          const k = ex(g, s.target.k)
          const val = ex(g, s.e)
          out.push(`let tk = ${k}; let _ = ${val}; table_key_check(&tk)?;`)
        } else reject('assignment target')
        break
      }
      case 'callst': reject('call statement'); break
      case 'if': {
        const arms = s.arms.map((a, k) => `${k ? ' else if ' : 'if '}${cond(g, a.c)} { ${stmts(g, a.b, tail && last && s.els !== null, fnBody)} }`).join('')
        const els = s.els ? ` else { ${stmts(g, s.els, tail && last, fnBody)} }` : ''
        out.push(arms + els + (tail && last && s.els ? '' : ''))
        if (tail && last && s.els === null) out.push(defaultRet(g))
        break
      }
      case 'return': out.push(tail && last ? retExpr(g, s.exprs) : `return ${retExpr(g, s.exprs)};`); break
    }
  })
  if (tail && (body.length === 0 || (body[body.length - 1].t !== 'return' && body[body.length - 1].t !== 'if'))) out.push(defaultRet(g))
  g.scopes.pop()
  return out.join('\n')
}
const defaultRet = (g: Gen) => (g.kind === 'special' ? 'Ok((Value::Nil, Value::Nil))' : 'Ok(Value::Nil)')
function retExpr(g: Gen, es: Ex[]): string {
  if (g.kind === 'special') {
    if (es.length === 0) return 'Ok((Value::Nil, Value::Nil))'
    if (es.length === 1 && !isMulti(g, es[0])) return `Ok((${ex(g, es[0])}, Value::Nil))`
    if (es.length === 2 && !isMulti(g, es[1])) return `Ok((${ex(g, es[0])}, ${ex(g, es[1])}))`
    return `Ok(ret2(${list(g, es)}))`
  }
  if (es.length === 0) return 'Ok(Value::Nil)'
  if (es.length === 1 && !isMulti(g, es[0])) return `Ok(${ex(g, es[0])})`
  return `Ok(arg(${listSlice(g, es)}, 0))`
}

const fnName = (id: string) => {
  const [src, from] = id.split(':')
  return (src === 'Modules/ModParser.lua' ? 'c' : src.replace(/\W/g, '_') + '_') + from
}

type Out = { id: string; to: number; sha: string; kind: 'special' | 'value' | 'helper'; code: string; calls: Set<string> }
function generate(id: string, fn: Fn, kind: 'special' | 'value' | 'helper', upBind: Map<string, string>): Out {
  const g: Gen = {
    id, ups: upBind, scopes: [new Map()], params: new Map(), vararg: fn.vararg, nparams: fn.params.length, decls: [],
    usesD: false, usesF: false, usesA: false, calls: new Set(), kind: kind === 'special' ? 'special' : 'value',
  }
  const paramVars = fn.params.map((p) => declare(g, p)) // later duplicates (`_`) shadow earlier ones, as in Lua
  const body = stmts(g, fn.body, true, fn.body)
  const pre: string[] = []
  paramVars.forEach((v, i) => {
    const d = g.decls[v.decl]
    if (d.reads > 0 || d.assigned) { g.usesA = true; pre.push(`let @@M${v.decl}@@${v.rust} = arg(a, ${i});`) }
  })
  let code = [...pre, body].join('\n')
  code = code.replace(/@@M(\d+)@@/g, (_, n) => (g.decls[+n].assigned ? 'mut ' : ''))
  code = code.replace(/@@V(\d+)@@/g, (_, n) => {
    const d = g.decls[+n]
    const base = `v${n}_${d.lua.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`).replace(/^_+/, '').replace(/__+/g, '_') || 'u'}`
    return d.reads > 0 || d.assigned ? base : `_${base}`
  })
  const ret = kind === 'special' ? 'Ret2' : 'Value'
  const sig = `fn ${fnName(id)}(${g.usesD ? 'd' : '_d'}: &ModParserData, ${g.usesF ? 'f' : '_f'}: &FnRef, ${g.usesA ? 'a' : '_a'}: &[Value]) -> Result<${ret}, ParseError>`
  const f = FNS[id]
  return { id, to: f.to, sha: f.sha, kind, code: `/// \`${id}-${f.to}\`\n${sig} {\n${code}\n}\n`, calls: g.calls }
}

// ---------------------------------------------------------------------------------------------------------------
// 5. driver
// ---------------------------------------------------------------------------------------------------------------
const files: Record<string, string[]> = {}
const srcLines = (src: string) => (files[src] ??= readFileSync(UP + src, 'utf8').replace(/\r\n/g, '\n').split('\n'))

// Top-level `local` declarations of `src` before line `before` (the last one of a name wins), as upvalue bindings.
function moduleScope(src: string, before: number): Map<string, string> {
  const m = new Map<string, string>()
  srcLines(src).slice(0, before - 1).forEach((l, i) => {
    const line = i + 1
    let r
    if ((r = /^local function ([A-Za-z_]\w*)\s*\(/.exec(l)) || (r = /^local ([A-Za-z_]\w*)\s*=\s*function\s*\(/.exec(l))) {
      m.set(r[1], FNS[`${src}:${line}`] ? `fn:${src}:${line}` : 'unknown')
    } else if ((r = /^local ([A-Za-z_]\w*)\s*=\s*(.*?)\s*$/.exec(l))) {
      const rhs = r[2]
      if (rhs === 'modLib.createMod') m.set(r[1], MOD)
      else if (rhs === 'bit.bor' && r[1] === 'bor') m.set(r[1], 'cfn')
      else if (rhs.startsWith('{') && NAMED[r[1]]) m.set(r[1], `named:${r[1]}`)
      else m.set(r[1], 'unknown')
    } else if ((r = /^local ([A-Za-z_]\w*)\s*$/.exec(l))) m.set(r[1], 'unknown')
  })
  return m
}

const manual: { id: string; to: number; refs: number; kind: string; why: string }[] = []
const generated = new Map<string, Out>()
const kindLabel = (id: string) => [...(kinds.get(id) ?? [])].join('+')

for (const id of Object.keys(FNS)) {
  if (HAND.has(id)) continue
  const f = FNS[id]
  const lines = srcLines(f.src).slice(f.from - 1, f.to)
  const text = lines.join('\n')
  const sha = createHash('sha256').update(text).digest('hex')
  if (sha !== f.sha) throw new Error(`${id}: sha of the checkout ${sha} != fns ${f.sha} (data drift -- regenerate data first)`)
  const ks = kinds.get(id) ?? new Set<Kind>()
  const fail = (why: string) => manual.push({ id, to: f.to, refs: refCount.get(id) ?? 0, kind: kindLabel(id), why })
  try {
    if (ks.size !== 1) reject(`referenced as ${kindLabel(id) || 'nothing'}`)
    const kind = [...ks][0]
    if (kind === 'jewel') reject('jewelFuncList visitor/factory: the jewel interface is R5.4 (parse.rs:249-256 stops on it)')
    if (kind === 'up' && [...(owners.get(id) ?? [])].every((o) => kinds.get(o)?.has('jewel'))) reject('upvalue of jewelFuncList closures only (R5.4)')
    const upBind = new Map<string, string>()
    for (const [name, bs] of ups.get(id) ?? []) {
      if (bs.size !== 1) reject(`upvalue '${name}' bound differently across references: ${[...bs].join(', ')}`)
      upBind.set(name, [...bs][0])
    }
    // a function reached only through another closure's upvalue carries no `up` in the data (the dumper records one
    // level): its free names are the module locals declared above it (ModParser.lua top level)
    if (kind === 'up') for (const [name, b] of moduleScope(f.src, f.from)) if (!upBind.has(name)) upBind.set(name, b)
    const toks = lex(text, f.from)
    const fi = toks.findIndex((x) => x.k === 'w' && x.v === 'function')
    if (fi < 0 || toks[fi].line !== f.from) reject('no `function` on the first line')
    const p = new Parser(toks)
    p.i = fi
    const fn = p.funcAt()
    if (fn.endLine !== f.to) reject(`body ends on line ${fn.endLine}, fns says ${f.to}`)
    generated.set(id, generate(id, fn, kind === 'up' ? 'helper' : kind, upBind))
  } catch (e) {
    if (e instanceof Reject) fail(e.message)
    else throw e
  }
}

// a generated body that calls a definition that is not generated is manual too (to a fixed point)
for (let changed = true; changed;) {
  changed = false
  for (const [id, o] of generated) {
    const bad = [...o.calls].find((c) => !generated.has(c))
    if (bad) {
      generated.delete(id)
      manual.push({ id, to: FNS[id].to, refs: refCount.get(id) ?? 0, kind: kindLabel(id), why: `calls ${bad}, which is not generated` })
      changed = true
    }
  }
}
// a generated helper nobody generated calls is dead weight: still registered (drift) but say so
const calledHelpers = new Set([...generated.values()].flatMap((o) => [...o.calls]))
for (const [id, o] of generated) {
  if (o.kind === 'helper' && !calledHelpers.has(id)) {
    generated.delete(id)
    manual.push({ id, to: FNS[id].to, refs: refCount.get(id) ?? 0, kind: kindLabel(id), why: 'helper only called from manual closures' })
  }
}

// ---- closures_gen.rs ----
const order = Object.keys(FNS).filter((id) => generated.has(id))
const KIND_RS = { special: 'Special', value: 'Value', helper: 'Helper' }
let rs = `// @generated by tools/modparser-gen (node tools/modparser-gen/gen.ts) -- do not edit.
//! The ModParser closures of classes A/B (R5-ARCH §4), generated from the Lua bodies of \`data/mod-parser.json\` \`fns\`
//! (sha256 of each body in \`GENERATED\`; drift test \`tests/generated.rs\`). Grammar and semantics: tools/modparser-gen/README.md.
//! Bodies outside the grammar are listed in KNOWLEDGE/drafts/R5-manual-closures.md (hand port, R5.3b).

use pob_data::mod_parser::{FnRef, ModParserData};
use pob_mods::{Table, Value};

use crate::ParseError;
use crate::closures::{ClosureFn, Registered, Ret2};
use crate::helpers::*;

/// Every generated definition (${order.length}), in \`fns\` order.
pub static GENERATED: [Registered; ${order.length}] = [
${order.map((id) => { const o = generated.get(id)!; return `    Registered { id: ${rustStr(id)}, to: ${o.to}, sha: ${rustStr(o.sha)}, f: ClosureFn::${KIND_RS[o.kind]}${o.kind === 'helper' ? '' : `(${fnName(id)})`} },` }).join('\n')}
];

${order.map((id) => generated.get(id)!.code).join('\n')}`
writeFileSync(OUT_RS, rs)
execFileSync('rustfmt', ['--edition', '2024', '--config-path', `${ROOT}/rustfmt.toml`, OUT_RS], { stdio: 'inherit' })

// ---- R5-manual-closures.md ----
manual.sort((a, b) => (FNS[a.id].src === FNS[b.id].src ? FNS[a.id].from - FNS[b.id].from : FNS[a.id].src < FNS[b.id].src ? -1 : 1))
const byWhy = new Map<string, number>()
for (const m of manual) {
  const k = m.why.replace(/'[^']*'/g, "'…'").replace(/Modules\/\S+/g, '<def>').replace(/line \d+/g, 'line N').replace(/near .*/, 'near …')
  byWhy.set(k, (byWhy.get(k) ?? 0) + 1)
}
const genRefs = order.reduce((s, id) => s + (refCount.get(id) ?? 0), 0)
const manRefs = manual.reduce((s, m) => s + m.refs, 0)
const md = `# R5.3a: closures the generator does not take (hand port, R5.3b)

Generated by \`node tools/modparser-gen/gen.ts\` -- do not edit by hand. Definitions in \`fns\`: ${Object.keys(FNS).length}; hand-ported helpers
(\`closures.rs\` HELPERS): ${HAND.size}; **generated: ${order.length}** (${genRefs} direct table references); **manual: ${manual.length}** (${manRefs} direct table references).
A body goes here when ANY construct is outside the grammar (tools/modparser-gen/README.md) -- never an approximate translation.

## Reasons (normalised)
| reason | definitions |
| --- | --- |
${[...byWhy].sort((a, b) => b[1] - a[1]).map(([k, n]) => `| ${k.replace(/\|/g, '\\|')} | ${n} |`).join('\n')}

## List
| id | to | referenced as | direct refs | reason |
| --- | --- | --- | --- | --- |
${manual.map((m) => `| \`${m.id}\` | ${m.to} | ${m.kind || '-'} | ${m.refs} | ${m.why.replace(/\|/g, '\\|')} |`).join('\n')}
`
writeFileSync(OUT_MD, md)
console.log(`generated ${order.length} (${genRefs} refs), manual ${manual.length} (${manRefs} refs), hand ${HAND.size}`)
