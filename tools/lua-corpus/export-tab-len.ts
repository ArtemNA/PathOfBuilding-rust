// `#t` of a LuaJIT 2.1 table built from `{}` by `t[k] = true` / `t[k] = nil` with small integer keys, answered by the DESKTOP LuaJIT
// (PoB runtime/lua51.dll via tools/luajit-win) -> crates/pob-tree/tests/fixtures/lj-tab-len.json. What the 2.69.0 set code reads
// (`#self.itemSets + 1`, `#self.skillSets + 1`, `#self.configSets + 1`: ItemsTab.lua:1686, SkillsTab.lua:1470/1495, ConfigTab.lua:1285/1312)
// after loads, copies and deletes; the gate is crates/pob-tree/tests/lj_tab_len.rs (`LjTable::len`).
// Run from the pob-rust root (Node 24):  node tools/lua-corpus/export-tab-len.ts
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import '../upstream.ts'
const { resolveRoots } = await import('../oracle/paths.ts')

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const POB_RUNTIME = join(resolveRoots().pobRoot, 'runtime')
const TOOL = join(ROOT, 'tools/luajit-win')
const FIXTURE_OUT = join(ROOT, 'crates/pob-tree/tests/fixtures/lj-tab-len.json')

// A deterministic generator (mulberry32): the corpus is the same on every run.
let seed = 0x7ab1e5
const rnd = (): number => {
  seed = (seed + 0x6d2b79f5) | 0
  let t = seed
  t = Math.imul(t ^ (t >>> 15), t | 1)
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}
const int = (n: number): number => Math.floor(rnd() * n)

// An op: +k = `t[k] = true`, -k = `t[k] = nil`.
const seqs: number[][] = []
// contiguous loads 1..n, then deletes and re-adds (the sets of a build)
for (let n = 0; n <= 40; n++) {
  const s: number[] = []
  for (let k = 1; k <= n; k++) s.push(k)
  seqs.push([...s])
  for (let r = 0; r < 6 && n > 0; r++) {
    const t = [...s]
    for (let d = 0; d <= int(4); d++) t.push(-(1 + int(n)))
    if (rnd() < 0.5) t.push(1 + int(n + 3))
    seqs.push(t)
  }
}
// loads in another order (ids of a file, `NewItemSet(id)`), with gaps
for (let i = 0; i < 1500; i++) {
  const len = 1 + int(14)
  const s: number[] = []
  for (let j = 0; j < len; j++) s.push(rnd() < 0.78 ? 1 + int(18) : -(1 + int(18)))
  seqs.push(s)
}

const lua = [
  'local seqs = {',
  ...seqs.map((s) => `{${s.join(',')}},`),
  '}',
  'local out = { jit.version }',
  'for _, s in ipairs(seqs) do',
  '  local t = {}',
  '  for _, op in ipairs(s) do',
  '    if op > 0 then t[op] = true else t[-op] = nil end',
  '  end',
  '  out[#out + 1] = tostring(#t)',
  'end',
  'return table.concat(out, "\\n")',
].join('\n')

const tmp = mkdtempSync(join(tmpdir(), 'pob-tablen-'))
try {
  writeFileSync(join(tmp, 'tablen.lua'), lua)
  execFileSync('cargo', ['build', '-q', '--release', '--manifest-path', join(TOOL, 'Cargo.toml')], { stdio: 'inherit' })
  const exe = join(TOOL, 'target/release/luajit-win.exe')
  const out = execFileSync(exe, [join(tmp, 'tablen.lua')], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, PATH: `${POB_RUNTIME};${process.env.PATH ?? ''}`, POB_LUA51_EXPECT: join(POB_RUNTIME, 'lua51.dll') },
  })
    .trim()
    .split('\n')
  const version = out.shift()!
  if (out.length !== seqs.length) throw new Error(`${seqs.length} sequences -> ${out.length} answers`)
  const dll = createHash('sha256').update(readFileSync(join(POB_RUNTIME, 'lua51.dll'))).digest('hex')
  const producer = { runtime: 'PathOfBuilding/runtime/lua51.dll', lua51Sha256: dll, luajit: version }
  const rows = seqs.map((s, i) => JSON.stringify([s, Number(out[i])]))
  writeFileSync(FIXTURE_OUT, `{"format":1,"producer":${JSON.stringify(producer)},"cases":[\n${rows.join(',\n')}\n]}\n`)
  console.log(`lj-tab-len: ${seqs.length} sequences, ${version}, lua51.dll ${dll.slice(0, 12)}`)
} finally {
  rmSync(tmp, { recursive: true, force: true })
}
