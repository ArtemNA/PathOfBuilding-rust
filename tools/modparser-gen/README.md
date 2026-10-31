# tools/modparser-gen — Lua subset → Rust for the ModParser closures (R5.3a, R5-ARCH §4)

```
$env:Path = "C:\nvm4w\nodejs;" + $env:Path
node tools/modparser-gen/gen.ts      # from the pob-rust root; no npm packages
```

Inputs: `data/mod-parser.json` (`fns`, the `{fn, up}` references in `tables`), `data/globals.json` (static types of
`ModFlag.X`/`KeywordFlag.X`/`SkillType.X`), the upstream Lua (`project_15/PathOfBuilding/src`, read-only).
Outputs (both committed, both deterministic — byte-identical on every run):

* `crates/pob-parser/src/closures_gen.rs` — one Rust function per generated definition + `GENERATED` (`id`, `to`, `sha`,
  `ClosureFn`), formatted by `rustfmt` with the workspace `rustfmt.toml`;
* `KNOWLEDGE/drafts/R5-manual-closures.md` — every definition the generator does not take, with the reason.

The sha256 of each body is recomputed from the checkout and must equal `fns` (otherwise the generator aborts: data drift).
The 11 hand-ported definitions of `closures.rs` `HELPERS` are never generated.

## What a definition becomes

| referenced from | `ClosureFn` | Rust signature | returns |
| --- | --- | --- | --- |
| `specialModList` | `Special` | `fn(&ModParserData, &FnRef, &[Value]) -> Result<Ret2, ParseError>` | first two values (L11) |
| `modTagList`, `preFlagList` | `Value` | same, `-> Result<Value, ParseError>` | first value |
| only another closure's upvalue (`explodeFunc`) | `Helper` | same as `Value`; called directly | first value |
| `jewelFuncList` (or an upvalue of jewel closures only) | — | manual (R5.4: `parse.rs` stops on jewel lines) | |

Arguments are the call site's (`closures.rs`): `[tonumber(cap1), cap1..capN]` for special, `[tonumber(cap1) | cap1, caps…]`
for modTag, the captures for preFlag; a parameter without an argument is nil.

## Grammar (Lua 5.1 subset; anything else → manual list, never an approximate translation)

```
funcbody  := 'function' [Name {'.' Name}] '(' [Name {',' Name} [',' '...'] | '...'] ')' block 'end'   -- ends on `to`
block     := {stat [';']} [retstat]
stat      := 'local' Name {',' Name} ['=' explist]
           | Name '=' exp                                  -- a local/parameter only
           | Name '[' exp ']' '=' exp | Name '.' Name '=' exp -- a local table nothing else reads (then: key check only)
           | 'if' exp 'then' block {'elseif' exp 'then' block} ['else' block] 'end'
retstat   := 'return' [explist] [';']
exp       := 'nil' | 'true' | 'false' | Number | String | '...' | tablecons | prefixexp
           | exp binop exp | unop exp                       -- lparser.c priorities
binop     := '+' | '-' | '*' | '/' | '%' | '^' | '..' | '==' | '~=' | '<' | '<=' | '>' | '>=' | 'and' | 'or'
unop      := '-' | 'not' | '#'
prefixexp := Name | prefixexp '[' exp ']' | prefixexp '.' Name | call | '(' exp ')'
tablecons := '{' [field {(','|';') field} [(','|';')]] '}'
field     := exp | Name '=' exp | '[' String ']' '=' exp        -- no duplicate keys
call      := mod(...) | flag(...)                               -- upvalues bound to ModTools.lua:33 / ModParser.lua:2012
           | firstToUpper(x) | triggerExtraSkill(...) | grantedExtraSkill(...) | extraSupport(...)
           | dealNoNonDamageType(...)                           -- the hand-ported helpers (helpers.rs)
           | <upvalue bound to a generated Helper>(...)        -- e.g. explodeFunc
           | bor(...) | tonumber(x) | math.floor(x) | math.min(...) | math.max(...)
           | string.gsub(s, Literal, string.upper | String)
           | exp ':' gsub '(' Literal ',' (string.upper | String) ')' | exp ':' (lower|upper) '(' ')'
           | exp ':' sub '(' exp [',' exp] ')'
```

Names resolve as Lua does: locals/parameters (block scoped, shadowing), then upvalues by the reference's `up`
(`lua` value → `upv(f, name)`, `{fn}` → the calls above, `{named}` → `gemIdLookup`/`conquerorList`/`dmgTypes`,
`{cfn}` → `bor`), then the globals `ModFlag`/`KeywordFlag`/`SkillType`/`tonumber`/`math.*`/`string.*`. A definition
reached only through another closure's upvalue has no `up` in the data (the dumper records one level); its free names are
the top-level `local` declarations of the source above it (`mod = modLib.createMod`, `local function flag`, …).
Every upvalue must be bound the same way in every reference.

Not taken (examples): loops, nested functions, `type`/`select`/`unpack`/`pairs`, `string.format`, `:match`, `data.*`,
calls of hand-port-only helpers with logic (`getSimpleConv`, `getThreshold`), and `mod()/flag()` with a literal `nil`
before a later argument in the tag part — the L9 orphan tag a closure result cannot carry (`RawParse.orphan_tags` exists
for static entries only).

## Semantics (helpers.rs, "Lua operators for the generated closures")

* Every value is a `pob_mods::Value`; a runtime error is `ParseError::Lua` with LuaJIT's text (`attempt to perform
  arithmetic on local 'num' (a nil value)`, …), a value the model cannot hold `ParseError::Unrepresentable`.
* Order of evaluation is Lua's (arguments, operands, constructor fields left to right; `a > b` evaluates `a` then `b`
  and compares `b < a`). `and`/`or` short-circuit and yield operands (L4); conditions use Lua truthiness.
* Arithmetic on doubles; numeric strings convert (L2); `%` = `a - floor(a/b)*b`; `..` formats numbers with `%.14g`
  (L1); comparisons: numbers, or strings byte-wise; `==` raw equality.
* Multiple values (L11): the last expression of an argument list, constructor or `return` expands (`...`, `gsub`);
  everywhere else one value. Table constructors drop trailing nils and nil named fields.
* `mod(...)` = `create_mod_lua` with the vararg heuristic (MOD-CORE §1); `string.upper` as a `gsub` replacement gets the
  first capture (the whole match); `ModFlag.X` etc. come from `data/globals.json` at run time.
