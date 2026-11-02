// R5.1b: survey of the 1263 closure bodies in data/mod-parser.json `fns` (static text analysis, no Lua parser).
// Run from the pob-rust root (Node 24):  node tools/modparser-survey/survey.ts
// Reads the upstream Lua (READ-ONLY) and writes KNOWLEDGE/drafts/R5-closure-survey.md.
import { POB } from '../upstream.ts'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const UP = `${POB}/src/` // tools/upstream.json (R-UP)
const J = JSON.parse(readFileSync(`${ROOT}/data/mod-parser.json`, 'utf8'))
const fns = J.fns
const files = {}
const lines = (src) => (files[src] ??= readFileSync(UP + src, 'utf8').split(/\r?\n/))

// ---- tokenizer (strings, comments, names, numbers, symbols) ----
function lex(s) {
  const t = []
  let i = 0
  const n = s.length
  while (i < n) {
    const c = s[i]
    if (/\s/.test(c)) { i++; continue }
    if (c === '-' && s[i + 1] === '-') {
      const m = /^--\[(=*)\[/.exec(s.slice(i, i + 40))
      if (m) { const e = s.indexOf(']' + m[1] + ']', i); i = e < 0 ? n : e + m[1].length + 2 } else { while (i < n && s[i] !== '\n') i++ }
      continue
    }
    if (c === '"' || c === "'") {
      let j = i + 1
      while (j < n && s[j] !== c) { if (s[j] === '\\') j++; j++ }
      t.push({ o: i, k: 's', v: s.slice(i, j + 1) }); i = j + 1; continue
    }
    const lm = c === '[' ? /^\[(=*)\[/.exec(s.slice(i, i + 40)) : null
    if (lm) { const e = s.indexOf(']' + lm[1] + ']', i); const j = e < 0 ? n : e + lm[1].length + 2; t.push({ o: i, k: 's', v: s.slice(i, j) }); i = j; continue }
    let m
    if ((m = /^(0[xX][0-9a-fA-F]+|\d+\.?\d*(?:[eE][+-]?\d+)?|\.\d+)/.exec(s.slice(i, i + 40)))) { t.push({ o: i, k: 'n', v: m[0] }); i += m[0].length; continue }
    if ((m = /^[A-Za-z_][A-Za-z_0-9]*/.exec(s.slice(i, i + 80)))) { t.push({ o: i, k: 'w', v: m[0] }); i += m[0].length; continue }
    const op = ['...', '..', '==', '~=', '<=', '>=', '::'].find((o) => s.startsWith(o, i))
    t.push({ o: i, k: 'p', v: op ?? c }); i += (op ?? c).length
  }
  return t
}
const KW = new Set(['function', 'if', 'then', 'else', 'elseif', 'end', 'for', 'while', 'do', 'repeat', 'until', 'return', 'local', 'and', 'or', 'not', 'nil', 'true', 'false', 'in', 'break'])
const isKw = (x) => x.k === 'w' && KW.has(x.v)

// ---- references: which fns are referenced (directly = table value / func; via `up` = upvalue) ----
const direct = {} // fn -> {table: count}
const viaUp = {} // fn -> count
const upNames = {} // fn -> Set(upvalue names)
const odd = []
const endMismatch = [] // fn refs at unexpected positions
const tableTotals = {}
const isFnRef = (v) => v && typeof v === 'object' && typeof v.fn === 'string'
function walkUp(v, owner) { // v is the `up` object of a reference to `owner`
  for (const [name, x] of Object.entries(v)) {
    ;(upNames[owner] ??= new Set()).add(name)
    if (isFnRef(x)) { viaUp[x.fn] = (viaUp[x.fn] ?? 0) + 1; if (x.up) walkUp(x.up, x.fn) } else if (x && typeof x === 'object') scanAny(x, `up:${owner}`)
  }
}
function scanAny(v, where) {
  if (!v || typeof v !== 'object') return
  if (isFnRef(v)) { odd.push(`${where}: ${v.fn}`); console.error("odd", where, v.fn); if (v.up) walkUp(v.up, v.fn); return }
  for (const x of Object.values(v)) scanAny(x, where)
}
for (const [tn, tb] of Object.entries(J.tables)) {
  tableTotals[tn] = Object.keys(tb).length
  for (const [key, v] of Object.entries(tb)) {
    const ref = isFnRef(v) ? v : isFnRef(v?.func) ? v.func : null
    if (ref) {
      const d = (direct[ref.fn] ??= {}); d[tn] = (d[tn] ?? 0) + 1
      if (ref.up) walkUp(ref.up, ref.fn)
    } else scanAny(v, `${tn}[${key}]`)
  }
}

// ---- body extraction + analysis ----
const HELPERS = new Set(['getSimpleConv', 'explodeFunc', 'grantedExtraSkill', 'triggerExtraSkill', 'extraSupport', 'gemIdLookup', 'getPerStat', 'getThreshold', 'dealNoNonDamageType', 'appendMod', 'firstToUpper'])
const MODTABLES = new Set(['skillIds', 'damageTypeList', 'dmgTypes', 'conquerorList', 'specialModList', 'oldList', 'resourceTypes', 'suffixTypes', 'penTypes', 'regenTypes', 'degenTypes', 'costTypes', 'baseCostTypes', 'flagTypes', 'modNameList', 'modFlagList', 'modTagList', 'formList', 'preFlagList', 'skillNameList', 'preSkillNameList'])
const STRFN = new Set(['lower', 'upper', 'gsub', 'match', 'find', 'format', 'sub', 'gmatch', 'rep', 'len', 'byte', 'char', 'reverse'])
const REALG = new Set(['copyTable', 'isValueInArray', 'modLib', 'bnot', 'print', 'error', 'assert', 'pcall', 'ModStore', 'common', 'launch', 'main', 'data', 'new', 'wipeTable', 'tostring', 'next', 'setmetatable', 'getmetatable', 'rawget'])
const CMP = new Set(['==', '~=', '<', '>', '<=', '>='])
const ARITH = new Set(['+', '*', '/', '%', '^'])

function extract(key) {
  const f = fns[key]
  const L = lines(f.src).slice(f.from - 1, f.to)
  const toks = lex(L.join('\n'))
  const fi = toks.findIndex((x) => x.k === 'w' && x.v === 'function')
  let depth = 0, end = -1
  for (let i = fi; i < toks.length; i++) {
    const x = toks[i]
    if (x.k !== 'w') continue
    if (x.v === 'function' || x.v === 'if' || x.v === 'do') depth++
    else if (x.v === 'end' && --depth === 0) { end = i; break }
  }
  if (fi < 0 || end < 0) return null
  let p = fi + 1; const params = []
  const named = toks[p].v !== '('
  while (toks[p].v !== '(') p++
  if (toks[p].v === '(') { p++; while (toks[p].v !== ')') { if (toks[p].v !== ',') params.push(toks[p].v); p++ } p++ }
  const src = L.join('\n'); const endLine = f.from + (src.slice(0, toks[end].o).match(/\n/g)?.length ?? 0)
  if (endLine !== f.to) endMismatch.push(key)
  return { named, params, body: toks.slice(p, end), text: toks.slice(fi, end + 1).map((x) => x.v).join(' ') }
}

function analyse(key, ex) {
  const { params, body } = ex
  const ups = upNames[key] ?? new Set()
  const F = new Set() // features
  const C = new Set() // reasons for class C
  const locals = new Set(params)
  const shape = []
  let brace = 0, paren = 0, fnCount = 0, retTop = 0
  for (let i = 0; i < body.length; i++) { // declarations first
    const x = body[i]
    if (x.v === 'local' && body[i + 1]) { if (body[i + 1].v === 'function') locals.add(body[i + 2].v); else for (let j = i + 1; j < body.length && body[j].k === 'w' && !isKw(body[j]); j += 2) { locals.add(body[j].v); if (body[j + 1]?.v !== ',') break } }
    if (x.v === 'for') for (let j = i + 1; body[j].v !== 'in' && body[j].v !== '='; j++) if (body[j].k === 'w') locals.add(body[j].v)
    if (x.v === 'function' && body[i + 1]?.v === '(') for (let j = i + 2; body[j].v !== ')'; j++) if (body[j].k === 'w') locals.add(body[j].v)
  }
  const pidx = new Map(params.map((p, i) => [p, i + 1])); const uidx = new Map()
  for (let i = 0; i < body.length; i++) {
    const x = body[i], pv = body[i - 1]?.v, nv = body[i + 1]?.v
    if (x.v === '{') brace++; else if (x.v === '}') brace--; else if (x.v === '(') paren++; else if (x.v === ')') paren--
    if (x.k === 's') { shape.push('S'); if (pv === '..' || nv === '..') F.add('concat'); continue }
    if (x.k === 'n') { shape.push('N'); continue }
    if (x.k === 'p') {
      shape.push(x.v)
      if (x.v === '..') { F.add('str:concat'); C.add('string-op') }
      if (CMP.has(x.v)) F.add('compare')
      if (ARITH.has(x.v) || (x.v === '-' && body[i - 1] && !['(', ',', '{', '=', 'return'].includes(pv))) F.add('arith')
      if (x.v === '-' && F.has('arith') === false) F.add('arith:neg')
      if (x.v === '#') F.add('len-op')
      if (x.v === '...') F.add('varargs')
      if (x.v === '=' && brace === 0 && paren === 0 && pv !== 'local') F.add('assign')
      continue
    }
    const w = x.v
    if (isKw(x)) {
      shape.push(w)
      if (w === 'if' || w === 'elseif') F.add('if')
      if (w === 'for' || w === 'while' || w === 'repeat') { F.add('loop'); C.add('loop') }
      if (w === 'and' || w === 'or' || w === 'not') F.add('and/or/not')
      if (w === 'local') F.add('local')
      if (w === 'function') fnCount++
      if (w === 'return' && brace === 0) retTop++
      continue
    }
    if (pv === '.' || pv === ':') { // member name
      shape.push(w)
      if (pv === ':' && !(body[i - 2]?.v === 'out')) { F.add('str:method'); C.add('string-op') }
      if (pv === ':' && body[i - 2]?.v === 'out') F.add('out-method')
      if (STRFN.has(w)) { F.add('str:' + w); C.add('string-op') }
      if (body[i - 2]?.v === 'math') F.add('arith:math')
      if (body[i - 2]?.v === 'data') { /* handled at base name */ }
      continue
    }
    if (nv === '=' && brace > 0 && (pv === '{' || pv === ',' || pv === ';')) { shape.push(w); if (w === 'type') F.add('tag-table'); continue } // field key
    // free/bound name
    if (pidx.has(w) && !(w === 'data' && params.includes('data') === false)) { shape.push('p' + pidx.get(w)); if (w === 'data') F.add('data:param'); continue }
    if (locals.has(w)) { shape.push('L'); continue }
    if (ups.has(w)) { if (!uidx.has(w)) uidx.set(w, uidx.size + 1); shape.push('u' + uidx.get(w)); F.add('upvalue'); continue }
    shape.push(w)
    if (w === 'mod' || w === 'flag') F.add('call:' + w)
    else if (w === 'tonumber') F.add('tonumber')
    else if (w === 'ModFlag' || w === 'KeywordFlag' || w === 'SkillType') F.add('flagconst:' + w)
    else if (w === 'bor' || w === 'band') F.add('bitop')
    else if (w === 'math' || w === 'm_huge') { if (w === 'math') F.add('arith:math') }
    else if (w === 'data') { F.add('data:global'); C.add('data-lookup') }
    else if (w === 't_insert' || (w === 'table' && nv === '.')) F.add('t_insert')
    else if (w === 'unpack') F.add('unpack')
    else if (w === 'select' || w === 'type') { F.add('builtin:' + w) }
    else if (w === 'parseMod') { F.add('call:parseMod'); C.add('recursion-parseMod') }
    else if (HELPERS.has(w)) { F.add('helper:' + w); C.add('helper:' + w) }
    else if (MODTABLES.has(w)) { F.add('module-table:' + w); C.add('module-table') }
    else if (w === 'string') { F.add('str:string.'); C.add('string-op') }
    else if (w === 'ipairs' || w === 'pairs') F.add('iter')
    else if (REALG.has(w)) { F.add('global:' + w); C.add('unknown:' + w) }
    else { F.add('enclosing-scope-name'); if (!ex.named) F.add('enclosing:' + w) }
  }
  const visitor = params.includes('node') && params.includes('out') || (fnCount > 1 && body.some((x, i) => x.v === 'function' && body[i + 2]?.v === 'node'))
  if (visitor) F.add('visitor')
  if (fnCount > 1 && !visitor) { F.add('nested-fn'); C.add('nested-fn') }
  if (F.has('builtin:type') || F.has('builtin:select')) C.add('builtin')
  if (F.has('len-op')) C.add('len')
  // returns-* features
  const b = body.map((x) => x.v)
  if (b[0] === 'return' && b[1] === '{' && b[b.length - 1] === '}') {
    let d = 0, e = 1, ok = true, keyed = false, parts = []
    for (let i = 1; i < b.length; i++) { const v = b[i]; if ('{(['.includes(v)) d++; else if ('})]'.includes(v)) d--; if (d === 0) { ok = i === b.length - 1 } }
    d = 0; let cur = []
    for (let i = 2; i < b.length - 1; i++) { const v = b[i]; if ('{(['.includes(v)) d++; if ('})]'.includes(v)) d--; if (d === 0 && v === ',') { parts.push(cur); cur = [] } else cur.push(v) }
    if (cur.length) parts.push(cur)
    if (ok && parts.length && parts.every((q) => (q[0] === 'mod' || q[0] === 'flag') && q[1] === '(')) F.add('returns-table-of-mod-calls')
    else if (ok && parts.some((q) => q[1] === '=')) F.add('returns-prop-table')
  } else if (b[0] === 'return' && (b[1] === 'mod' || b[1] === 'flag') && retTop === 1) F.add('returns-single-call')
  if (ex.multiline) F.add('multiline')
  // classes
  const imperative = F.has('local') || F.has('assign') || F.has('t_insert') || F.has('unpack') || retTop > 1 || F.has('iter')
  const condy = F.has('if') || F.has('and/or/not') || F.has('compare') || imperative
  const single = !condy && !visitor && !C.size
  const cls = C.size ? 'C' : (F.has('visitor') ? 'B' : single ? 'A' : 'B')
  const relaxedC = [...C].filter((r) => !(r.startsWith('helper:') || r === 'string-op' && F.has('str:concat') && ![...F].some((f) => /^str:(method|lower|upper|gsub|match|find|format|sub|gmatch|string)/.test(f))))
  const cls2 = cls === 'C' && relaxedC.length === 0 ? 'B*' : cls // C only by helper calls / plain `..` concatenation
  return { cls, cls2, F, C, shape: shape.join(' '), visitor, params }
}

// ---- run ----
const R = []
let bad = 0
for (const key of Object.keys(fns)) {
  const f = fns[key]
  const ex = extract(key)
  if (!ex) { bad++; console.error('extract failed', key); continue }
  ex.multiline = f.to > f.from
  const a = analyse(key, ex)
  const dr = direct[key] ?? {}
  const nd = Object.values(dr).reduce((s, x) => s + x, 0)
  R.push({ key, f, ex, a, dr, nd, nup: viaUp[key] ?? 0 })
}
const totalRecords = R.reduce((s, r) => s + r.nd, 0)
const tabEntriesWithFn = Object.values(J.tables).reduce((s, tb) => s + Object.values(tb).filter((v) => isFnRef(v) || isFnRef(v?.func)).length, 0)
const pct = (a, b) => (b ? ((100 * a) / b).toFixed(1) + '%' : '-')
const row = (...c) => '| ' + c.join(' | ') + ' |'
const out = []
const P = (s = '') => out.push(s)
const CLS = ['A', 'B', 'C']

P('# R5.1b: огляд форм 1 263 тіл замикань ModParser')
P()
P('Згенеровано `node tools/modparser-survey/survey.ts` (статичний аналіз тексту Lua; власний токенізатор, без залежностей). Висновки для R5-ARCH §1.3 переносить оркестратор.')
P()
P('## 0. Методика і застереження')
P('- Тіло = токени від першого `function` на рядку `from` до парного `end` (баланс `function/if/do … end`); перевірка: рядок кінця = `to`.')
P('- **Запис** = ключ таблиці ModParser, значення якого — `{fn,up}` або `{func:{fn,up},type}` (прямі посилання). Посилання лише через `up` (замикання-upvalue) рахуються окремо як «через up».')
P('- **A**: одне `return` (таблиця `mod()/flag()`, таблиця-властивості, один `mod()`), константи/параметри/upvalue, арифметика, `tonumber`, `math.*`, теги-таблиці, `ModFlag/KeywordFlag/SkillType`, `bor/band`. Жодних умов/локальних/присвоєнь.')
P('- **B**: A + `if/elseif`, `and/or/not`, порівняння, `local`, присвоєння, `t_insert`, `unpack`, кілька `return`; а також «visitor»-замикання радіусних самоцвітів (`function(node,out,data)` без циклів; `data` там — параметр, не глобальний `data.*`).')
P('- **C**: цикли; глобальний `data.*`; `parseMod`; хелпери з логікою (`getSimpleConv`, `explodeFunc`, `grantedExtraSkill`, `triggerExtraSkill`, `extraSupport`, `gemIdLookup`, `getPerStat`, `getThreshold`, `dealNoNonDamageType`, `appendMod`, `firstToUpper`); рядкові операції (`..`, `:method`, `gsub/match/lower/…`); таблиці модуля; невідомі глобали; вкладені не-visitor функції; `type()/select()`; `#`.')
P('- **B\\*** (чутливість): C, де *єдині* причини — виклик хелпера з переліку та/або проста конкатенація `..` (тобто хелпер портується один раз, а виклик механічний). Це підмножина C, рахується окремо; класи A/B/C — строгі.')
P(`- Іменовані локальні функції-хелпери (function name(...), самі визначення хелперів, не замикання-значення): ${R.filter((x) => x.ex.named).length}; прямих записів у них: ${R.filter((x) => x.ex.named).reduce((s, x) => s + x.nd, 0)}. Визначень без прямих записів (лише через up або нікуди): ${R.filter((x) => x.nd === 0).length}, з них посилань через up є: ${R.filter((x) => x.nd === 0 && x.nup > 0).length}.`)
P(`- Збігів рядка кінця з to: ${R.length - endMismatch.length}/${R.length}.`)
P(`- Технічно: невдалих витягувань тіла: ${bad}; записів у таблицях із прямим fn: ${tabEntriesWithFn}; сума прямих посилань за визначеннями: ${totalRecords}; посилання fn у нетипових позиціях: ${odd.length}.`)
P()

const stat = (rs) => ({ d: rs.length, r: rs.reduce((s, x) => s + x.nd, 0), u: rs.reduce((s, x) => s + x.nup, 0) })
P('## 1. Класи (визначення / записи)')
P(row('клас', 'визначень', '% визн.', 'записів (прямих)', '% записів', 'посилань через up'))
P(row('---', '---', '---', '---', '---', '---'))
let sd = 0, sr = 0
for (const c of CLS) { const s = stat(R.filter((x) => x.a.cls === c)); sd += s.d; sr += s.r; P(row(c, s.d, pct(s.d, R.length), s.r, pct(s.r, totalRecords), s.u)) }
P(row('**разом**', sd, '', sr, '', ''))
{ const s = stat(R.filter((x) => x.a.cls2 === 'B*')); P(); P(`Чутливість: B\\* (C лише через виклик хелпера/конкатенацію) = ${s.d} визначень, ${s.r} записів. Механічні A+B+B\\*: ${stat(R.filter((x) => x.a.cls !== 'C' || x.a.cls2 === 'B*')).d} визначень (${pct(stat(R.filter((x) => x.a.cls !== 'C' || x.a.cls2 === 'B*')).d, R.length)}), ${stat(R.filter((x) => x.a.cls !== 'C' || x.a.cls2 === 'B*')).r} записів (${pct(stat(R.filter((x) => x.a.cls !== 'C' || x.a.cls2 === 'B*')).r, totalRecords)}). Строго A+B: ${pct(stat(R.filter((x) => x.a.cls !== 'C')).d, R.length)} визн., ${pct(stat(R.filter((x) => x.a.cls !== 'C')).r, totalRecords)} записів.`) }
P()
P('### 1a. Без visitor-замикань (visitor вважати C)')
{ const nv = R.filter((x) => !x.a.visitor); const v = R.filter((x) => x.a.visitor)
  P(`visitor-замикань: ${v.length} визначень, ${stat(v).r} записів (класи: ${CLS.map((c) => c + '=' + v.filter((x) => x.a.cls === c).length).join(', ')}).`)
  const ab = nv.filter((x) => x.a.cls !== 'C'); P(`Без visitor: A+B = ${ab.length}/${nv.length} визначень (${pct(ab.length, nv.length)}), ${stat(ab).r}/${stat(nv).r} записів (${pct(stat(ab).r, stat(nv).r)}).`) }
P()
P('## 2. По таблицях (записи → клас)')
const tn = Object.keys(J.tables).filter((t) => R.some((x) => x.dr[t]))
P(row('таблиця', 'ключів усього', 'записів із fn', 'A', 'B', 'C', '(з них B*)'))
P(row(...Array(7).fill('---')))
for (const t of tn) {
  const cnt = (f) => R.filter(f).reduce((s, x) => s + (x.dr[t] ?? 0), 0)
  P(row(t, tableTotals[t], cnt(() => true), ...CLS.map((c) => cnt((x) => x.a.cls === c)), cnt((x) => x.a.cls2 === 'B*')))
}
P()
P('## 3. По ознаках (визначень / записів)')
const feat = new Map()
for (const r of R) for (const f of r.a.F) { const e = feat.get(f) ?? { d: 0, r: 0 }; e.d++; e.r += r.nd; feat.set(f, e) }
P(row('ознака', 'визначень', 'записів'))
P(row('---', '---', '---'))
for (const [f, e] of [...feat].sort((a, b) => b[1].d - a[1].d)) P(row('`' + f + '`', e.d, e.r))
P()
P('### 3a. Причини класу C')
const rs = new Map()
for (const r of R.filter((x) => x.a.cls === 'C')) for (const c of r.a.C) { const e = rs.get(c) ?? { d: 0, r: 0 }; e.d++; e.r += r.nd; rs.set(c, e) }
P(row('причина', 'визначень', 'записів'))
P(row('---', '---', '---'))
for (const [f, e] of [...rs].sort((a, b) => b[1].d - a[1].d)) P(row('`' + f + '`', e.d, e.r))
P()
P('## 4. Топ-20 форм (нормалізований шаблон: рядки→S, числа→N, параметри→pK, upvalue→uK, локальні→L)')
const sh = new Map()
for (const r of R) { const k = r.ex.params.map((_, i) => 'p' + (i + 1)).join(',') + ' => ' + r.a.shape; const e = sh.get(k) ?? { d: 0, r: 0, c: r.a.cls, ex: r.key }; e.d++; e.r += r.nd; sh.set(k, e) }
P(`Різних форм: ${sh.size}.`)
P()
P(row('#', 'визначень', 'записів', 'клас', 'приклад', 'форма'))
P(row(...Array(6).fill('---')))
;[...sh].sort((a, b) => b[1].d - a[1].d || b[1].r - a[1].r).slice(0, 20).forEach(([k, e], i) => P(row(i + 1, e.d, e.r, e.c, `${e.ex.replace('Modules/', '')}-${fns[e.ex].to}`, '`' + k.replace(/\|/g, '\\|').slice(0, 150) + '`')))
{ const top = [...sh].sort((a, b) => b[1].d - a[1].d).slice(0, 20); P(); P(`Топ-20 покривають ${top.reduce((s, [, e]) => s + e.d, 0)} визначень (${pct(top.reduce((s, [, e]) => s + e.d, 0), R.length)}).`) }
P()
P('## 5. Приклади (по 3 на клас)')
for (const c of CLS) {
  P(`### Клас ${c}`)
  const pool = R.filter((x) => x.a.cls === c); const pick = [pool[0], pool[Math.floor(pool.length / 2)], pool[pool.length - 1]].filter(Boolean)
  for (const r of pick) { P(`- \`${r.f.src}:${r.f.from}-${r.f.to}\` (записів ${r.nd}): \`${r.ex.text.replace(/`/g, "'").slice(0, 260)}\``) }
  P()
}
P('## 6. Повний перелік класу C')
const cs = R.filter((x) => x.a.cls === 'C')
P(`Всього: ${cs.length} визначень, ${stat(cs).r} записів. Опис = початок тіла (до 110 символів); [B*] = C лише через хелпер/конкатенацію.`)
P()
P(row('src:from-to', 'записів', 'ознаки (причини C)', 'опис'))
P(row(...Array(4).fill('---')))
for (const r of cs) P(row(`${r.f.src.replace('Modules/', '')}:${r.f.from}-${r.f.to}`, r.nd, (r.a.cls2 === 'B*' ? '[B*] ' : '') + [...r.a.C].join(', ').replace(/\|/g, '/'), '`' + r.ex.text.replace(/\|/g, '/').replace(/`/g, "'").slice(0, 110) + '`'))
P()
P('## 7. Залежності: визначення, на які посилаються лише через upvalue (портувати один раз)')
P('Замикання класів A/B, що викликають такі функції як upvalue (`mod` = ModTools createMod, `flag`, `getThreshold`, …), самі лишаються механічними; ціна — одноразовий порт цих залежностей.')
P()
P(row('src:from-to', 'посилань через up', 'клас', 'опис'))
P(row(...Array(4).fill('---')))
for (const r of R.filter((x) => x.nup > 0).sort((a, b) => b.nup - a.nup).slice(0, 25)) P(row(`${r.f.src.replace('Modules/', '')}:${r.f.from}-${r.f.to}`, r.nup, r.a.cls, '`' + r.ex.text.replace(/[|]/g, '/').replace(/`/g, "'").slice(0, 90) + '`'))
P()
mkdirSync(`${ROOT}/KNOWLEDGE/drafts`, { recursive: true })
writeFileSync(`${ROOT}/KNOWLEDGE/drafts/R5-closure-survey.md`, out.join('\n') + '\n')
console.log(`defs=${R.length} bad=${bad} records=${totalRecords} tableEntriesWithFn=${tabEntriesWithFn} odd=${odd.length}`)
for (const c of CLS) { const s = stat(R.filter((x) => x.a.cls === c)); console.log(c, s.d, s.r, s.u) }
console.log('B*', stat(R.filter((x) => x.a.cls2 === 'B*')))
