// R-UP2: Lua line numbers that reach RUNTIME text (error messages, `writers`, ids) must follow the upstream they port: LuaJIT prints the
// line of the file it runs. This tool maps every such number from the old upstream to the new one through the exact line map of
// `git diff -U0 <old> <new>` and reports (default) or rewrites (`--write`) them. Comments are NOT touched (they cite the upstream of
// the day the code was ported).
//
//   node tools/upstream-linemap.ts --old <sha> [--new <sha>] [--write]
//
// What counts as runtime (outside `//` comments, in crates/*/src):
//   A. string literals `Modules|Classes|Data/<path>.lua:<n>[-<m>]`;
//   B. integer arguments passed for a `line: u32` / `site: u32` parameter. The Lua file of such a function: a path literal or a
//      `const X: &str = "<path>"` its body uses, else (transitively) the line-parameter function it hands `line` to. Names are resolved
//      in the same .rs file first, then crate-wide when unique.
// A line inside a changed hunk has no image, and a function whose file is not resolved stays as it is: both are REPORTED, never
// guessed. Registries re-keyed by body sha (ModParser closures: modparser-gen/rekey.ts; skill funcs: rekey-skill-funcs.ts) are skipped.
import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const ROOT = join(import.meta.dirname, '..')
const cfg = JSON.parse(readFileSync(join(ROOT, 'tools/upstream.json'), 'utf8'))
const arg = (k: string): string | undefined => { const i = process.argv.indexOf(k); return i >= 0 ? process.argv[i + 1] : undefined }
const OLD = arg('--old') ?? (() => { throw new Error('--old <sha> is required') })()
const NEW = arg('--new') ?? cfg.pobSha
const WRITE = process.argv.includes('--write')
const POB = cfg.pobRoot as string

type LineMap = (n: number) => number | null
const maps = new Map<string, LineMap>()
function lineMap(lua: string): LineMap {
  const hit = maps.get(lua)
  if (hit) return hit
  const diff = execFileSync('git', ['-C', POB, 'diff', '-U0', OLD, NEW, '--', `src/${lua}`], { encoding: 'utf8', maxBuffer: 1 << 28 })
  const hunks = [...diff.matchAll(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/gm)].map((m) => ({
    a: Number(m[1]), b: m[2] === undefined ? 1 : Number(m[2]), d: m[4] === undefined ? 1 : Number(m[4]),
  }))
  const f: LineMap = (n) => {
    let shift = 0
    for (const h of hunks) {
      // b = 0: a pure insertion AFTER old line a
      const end = h.b === 0 ? h.a : h.a + h.b - 1
      if (h.b > 0 && n >= h.a && n <= end) return null
      if (n > end) shift += h.d - h.b
    }
    return n + shift
  }
  maps.set(lua, f)
  return f
}

function rsFiles(dir: string, out: string[] = []): string[] {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e)
    if (statSync(p).isDirectory()) rsFiles(p, out)
    else if (p.endsWith('.rs')) out.push(p)
  }
  return out
}
const files = readdirSync(join(ROOT, 'crates')).flatMap((c) => {
  try { return rsFiles(join(ROOT, 'crates', c, 'src')) } catch { return [] }
})

// the code part of a line: up to a `//` that is not inside a string literal
function codeOf(line: string): string {
  let inStr = false
  for (let i = 0; i < line.length; i++) {
    const ch = line[i]
    if (ch === '\\' && inStr) { i++; continue }
    if (ch === '"') inStr = !inStr
    if (!inStr && ch === '/' && line[i + 1] === '/') return line.slice(0, i)
  }
  return line
}
// the top-level arguments of a call whose `(` was just consumed; `end` = index of the closing `)` (or -1)
function topArgs(rest: string): { args: string[]; end: number } {
  let depth = 0, cur = '', inStr = false
  const args: string[] = []
  for (let k = 0; k < rest.length; k++) {
    const ch = rest[k]!
    if (inStr) { cur += ch; if (ch === '\\') { cur += rest[++k] ?? '' } else if (ch === '"') inStr = false; continue }
    if (ch === '"') inStr = true
    if (ch === '(' || ch === '[' || ch === '{') depth++
    if (ch === ')' || ch === ']' || ch === '}') { if (depth === 0) { args.push(cur); return { args, end: k } } depth-- }
    if (ch === ',' && depth === 0) { args.push(cur); cur = ''; continue }
    cur += ch
  }
  args.push(cur)
  return { args, end: -1 }
}

const PATH = String.raw`(?:Modules|Classes|Data)(?:/[A-Za-z0-9_]+)+\.lua`
// a bare `CalcTriggers.lua:{line}` template (no directory) names the Modules/ or Classes/ file of that name
const BARE = /"[^"\n]*?\b([A-Z][A-Za-z]+\.lua):\{/g
const bareToPath = (name: string): string | undefined =>
  ['Modules', 'Classes'].map((d) => `${d}/${name}`).find((p) => { try { return statSync(join(POB, 'src', p)).isFile() } catch { return false } })
const LIT = new RegExp(`(${PATH}):(\\d+)(?:-(\\d+))?`, 'g')
// generated from the NEW upstream by tools/config-apply-gen/gen.ts: its numbers are new already
const SKIP_FILES = [/pob-build[\\/]src[\\/]config[\\/]apply_gen\.rs$/, /pob-parser[\\/]src[\\/]closures/, /pob-calc[\\/]src[\\/]skill_funcs[\\/]mod\.rs$/]

type Fn = { file: string; name: string; idx: number; body: string; lua?: string | null }
const fnsByName = new Map<string, Fn[]>()
const constsByName = new Map<string, Set<string>>()
const fileConsts = new Map<string, Map<string, string>>()
const texts = new Map<string, string>()
for (const file of files) {
  if (SKIP_FILES.some((r) => r.test(file))) continue
  const text = readFileSync(file, 'utf8')
  texts.set(file, text)
  const fc = new Map<string, string>()
  for (const m of text.matchAll(new RegExp(`const ([A-Z_0-9]+): &str = "(?:\\./)?(${PATH})"`, 'g'))) {
    fc.set(m[1]!, m[2]!)
    if (!constsByName.has(m[1]!)) constsByName.set(m[1]!, new Set())
    constsByName.get(m[1]!)!.add(m[2]!)
  }
  fileConsts.set(file, fc)
  for (const m of text.matchAll(/^([ \t]*)(?:pub(?:\([a-z]+\))? )?fn ([a-z_0-9]+)(?:<[^>]*>)?\(([^)]*)\)/gm)) {
    const params = m[3]!.split(',').map((s) => s.trim()).filter(Boolean)
    // methods: `&self` is not an argument at the call site
    const shown = params.filter((p) => !/^&?(mut )?self$|^&'[a-z_]+ (mut )?self$/.test(p))
    const idx = shown.findIndex((p) => /^(line|site): u32$/.test(p))
    if (idx < 0) continue
    const end = text.indexOf(`\n${m[1]}}`, m.index!)
    const body = (end < 0 ? text.slice(m.index!) : text.slice(m.index!, end)).split('\n').map(codeOf).join('\n')
    const f: Fn = { file, name: m[2]!, idx, body }
    if (!fnsByName.has(f.name)) fnsByName.set(f.name, [])
    fnsByName.get(f.name)!.push(f)
  }
}
// the same file, else the only one in the same directory (module), else the only one in the same crate, else the only one
const dirOf = (f: string): string => f.replace(/[\\/][^\\/]+$/, '')
const crateOf = (f: string): string => f.replace(/^(.*?[\\/]crates[\\/][^\\/]+).*$/, '$1')
function pick<T extends { file: string }>(xs: T[] | undefined, file: string): T | undefined {
  if (!xs) return undefined
  for (const near of [(x: T) => x.file === file, (x: T) => dirOf(x.file) === dirOf(file), (x: T) => crateOf(x.file) === crateOf(file), () => true]) {
    const c = xs.filter(near)
    if (c.length === 1) return c[0]
    if (c.length > 1) return undefined
  }
  return undefined
}
const constLua = (name: string, file: string): string | undefined =>
  fileConsts.get(file)?.get(name) ?? (constsByName.get(name)?.size === 1 ? [...constsByName.get(name)!][0] : undefined)

const all = [...fnsByName.values()].flat()
for (const f of all) {
  const found = new Set([...f.body.matchAll(new RegExp(`(${PATH})`, 'g'))].map((m) => m[1]!))
  for (const m of f.body.matchAll(BARE)) { const p = bareToPath(m[1]!); if (p) found.add(p) }
  for (const m of f.body.matchAll(/\{([A-Z_0-9]+)\}|\b([A-Z][A-Z_0-9]+)\b/g)) {
    const lua = constLua(m[1] ?? m[2]!, f.file)
    if (lua) found.add(lua)
  }
  f.lua = found.size === 1 ? [...found][0]! : found.size > 1 ? null : undefined
}
for (let pass = 0; pass < 8; pass++) {
  for (const f of all) {
    if (f.lua !== undefined) continue
    for (const m of f.body.matchAll(/\b([a-z_0-9]+)\(/g)) {
      if (m[1] === f.name) continue
      const g = pick(fnsByName.get(m[1]!), f.file)
      if (!g?.lua) continue
      const a = topArgs(f.body.slice(m.index! + m[0].length)).args[g.idx]?.trim()
      if (a === 'line' || a === 'site') { f.lua = g.lua; break }
    }
  }
}

// last resort, marked `B(file)` in the report: a function still unresolved whose .rs file's code names exactly one Lua file
const byFile = new Set<Fn>()
for (const f of all) {
  if (f.lua) continue
  const code = texts.get(f.file)!.split('\n').map(codeOf).join('\n')
  const found = new Set([...code.matchAll(new RegExp(`(${PATH})`, 'g'))].map((m) => m[1]!))
  for (const m of code.matchAll(BARE)) { const p = bareToPath(m[1]!); if (p) found.add(p) }
  if (found.size === 1) { f.lua = [...found][0]!; byFile.add(f) }
}

// lines this working tree already changed against HEAD: their numbers may be NEW already (code ported for this update) --
// reported with `*` and never rewritten
const touched = new Map<string, Set<number>>()
for (const m of execFileSync('git', ['-C', ROOT, 'diff', '-U0', 'HEAD', '--', 'crates'], { encoding: 'utf8', maxBuffer: 1 << 28 })
  .matchAll(/^\+\+\+ b\/(.+)$|^@@ -\S+ \+(\d+)(?:,(\d+))? @@/gm)) {
  if (m[1]) { touched.set(m[1], touched.get(m[1]) ?? new Set()); (touched as any).cur = m[1]; continue }
  const set = touched.get((touched as any).cur)!
  const s = Number(m[2]), n = m[3] === undefined ? 1 : Number(m[3])
  for (let k = s; k < s + n; k++) set.add(k)
}
type Row = { file: string; line: number; kind: string; lua: string; from: string; to: string | null }
const rows: Row[] = []
for (const [file, text] of texts) {
  const rel = file.slice(ROOT.length + 1).replaceAll('\\', '/')
  const lines = text.split('\n')
  let changed = false
  const isTouched = (ln: number): boolean => touched.get(rel)?.has(ln) ?? false
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i]!
    const code = codeOf(raw)
    const star = isTouched(i + 1) ? '*' : ''
    // A
    let next = code.replace(LIT, (whole, lua: string, a: string, b: string | undefined) => {
      const f = lineMap(lua)
      const na = f(Number(a)), nb = b === undefined ? undefined : f(Number(b))
      if (na === null || nb === null) { rows.push({ file: rel, line: i + 1, kind: 'A' + star, lua, from: whole, to: null }); return whole }
      const to = `${lua}:${na}${nb === undefined ? '' : `-${nb}`}`
      if (to !== whole) rows.push({ file: rel, line: i + 1, kind: 'A' + star, lua, from: whole, to })
      return star ? whole : to
    })
    // C: `"{CONST}:<n>"` where CONST names a Lua path, and a bare `"CalcTriggers.lua:<n>"`
    next = next.replace(/\{([A-Z_0-9]+)\}:(\d+)(?:-(\d+))?/g, (whole, c: string, a: string, b: string | undefined) => {
      const lua = constLua(c, file)
      if (!lua) return whole
      const f = lineMap(lua)
      const na = f(Number(a)), nb = b === undefined ? undefined : f(Number(b))
      if (na === null || nb === null) { rows.push({ file: rel, line: i + 1, kind: 'C' + star, lua, from: whole, to: null }); return whole }
      const to = `{${c}}:${na}${nb === undefined ? '' : `-${nb}`}`
      if (to !== whole) rows.push({ file: rel, line: i + 1, kind: 'C' + star, lua, from: whole, to })
      return star ? whole : to
    })
    next = next.replace(/(?<![/A-Za-z])([A-Z][A-Za-z]+\.lua):(\d+)(?:-(\d+))?/g, (whole, name: string, a: string, b: string | undefined) => {
      const lua = bareToPath(name)
      if (!lua) return whole
      const f = lineMap(lua)
      const na = f(Number(a)), nb = b === undefined ? undefined : f(Number(b))
      if (na === null || nb === null) { rows.push({ file: rel, line: i + 1, kind: 'C' + star, lua, from: whole, to: null }); return whole }
      const to = `${name}:${na}${nb === undefined ? '' : `-${nb}`}`
      if (to !== whole) rows.push({ file: rel, line: i + 1, kind: 'C' + star, lua, from: whole, to })
      return star ? whole : to
    })
    if (next !== code) { lines[i] = next + raw.slice(code.length); changed = true }
  }
  // B: the calls of line-parameter functions over the whole file (rustfmt splits long calls over lines); comments are blanked
  let body = lines.join('\n')
  const masked = body.split('\n').map((l) => { const c = codeOf(l); return c + ' '.repeat(l.length - c.length) }).join('\n')
  const lineAt = (off: number): number => masked.slice(0, off).split('\n').length
  const edits: { at: number; len: number; text: string }[] = []
  for (const m of masked.matchAll(/\b([a-z_0-9]+)\(/g)) {
    const before = masked.slice(Math.max(0, m.index! - 3), m.index!)
    const g = /fn $/.test(before) ? undefined : pick(fnsByName.get(m[1]!), file)
    if (!g) continue
    const open = m.index! + m[0].length
    const { args, end } = topArgs(masked.slice(open))
    const a = args[g.idx]
    if (end < 0 || a === undefined || !/^\s*\d+\s*$/.test(a)) continue
    const n = Number(a.trim())
    const ln = lineAt(open + args.slice(0, g.idx).reduce((s, x) => s + x.length + 1, 0) + a.indexOf(String(n)))
    if (!g.lua) { rows.push({ file: rel, line: ln, kind: 'B?', lua: '?', from: `${g.name}(..${n}..)`, to: null }); continue }
    const to = lineMap(g.lua)(n)
    if (to === null) { rows.push({ file: rel, line: ln, kind: 'B', lua: g.lua, from: `${g.name}(..${n}..)`, to: null }); continue }
    if (to === n) continue
    rows.push({ file: rel, line: ln, kind: (byFile.has(g) ? 'B(file)' : 'B') + (isTouched(ln) ? '*' : ''), lua: g.lua, from: `${g.name}(..${n}..)`, to: `${g.name}(..${to}..)` })
    if (isTouched(ln)) continue
    const argStart = open + args.slice(0, g.idx).reduce((s, x) => s + x.length + 1, 0)
    edits.push({ at: argStart + a.indexOf(String(n)), len: String(n).length, text: String(to) })
  }
  for (const e of edits.sort((x, y) => y.at - x.at)) body = body.slice(0, e.at) + e.text + body.slice(e.at + e.len)
  if (edits.length) changed = true
  if (WRITE && changed) writeFileSync(file, body)
}
console.log(`(* = a line this working tree already changed against HEAD: left as it is; check that its number is new already)`)
const unm = rows.filter((r) => r.to === null)
for (const r of rows) console.log(`${r.kind}\t${r.file}:${r.line}\t[${r.lua}] ${r.from} -> ${r.to ?? 'UNMAPPED (changed upstream line / unresolved file)'}`)
console.log(`${rows.length - unm.length} mapped, ${unm.length} to check by hand (${OLD.slice(0, 9)} -> ${String(NEW).slice(0, 9)})${WRITE ? ', written' : ', dry run'}`)
