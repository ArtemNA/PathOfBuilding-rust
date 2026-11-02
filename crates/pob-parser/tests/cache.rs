//! `parse_mod_line` = ModParser.lua:6994-7021 cache branch: every ModCache line hits, every answer is a fresh deep copy,
//! a line outside the cache misses with `nil, line`.

use std::rc::Rc;

use pob_mods::{ModStore, ModType, Table, Tag, Value, create_mod};
use pob_parser::{ParseMode, copy_table_mod, parse_mod_line, parse_mod_stats, reset_parse_mod_stats};

/// Structural equality of two mod values with Lua semantics where it matters (numbers by bits).
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x.to_bits() == y.to_bits(),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Table(x), Value::Table(y)) => {
            x.array.len() == y.array.len()
                && x.array.iter().zip(&y.array).all(|(p, q)| same_value(p, q))
                && x.hash.len() == y.hash.len()
                && x.hash.iter().all(|(k, v)| y.hash.get(k).is_some_and(|w| same_value(v, w)))
        }
        (Value::Mod(x), Value::Mod(y)) => same_mod(x, y),
        _ => false,
    }
}

fn same_mod(a: &pob_mods::Mod, b: &pob_mods::Mod) -> bool {
    a.name == b.name
        && a.ty == b.ty
        && same_value(&a.value, &b.value)
        && a.flags == b.flags
        && a.keyword_flags == b.keyword_flags
        && a.source == b.source
        && a.tags.len() == b.tags.len()
        && a.tags
            .iter()
            .zip(&b.tags)
            .all(|(x, y)| same_value(&Value::Table(x.table()), &Value::Table(y.table())))
}

#[test]
fn every_cache_line_hits_with_an_equal_copy() {
    reset_parse_mod_stats();
    let cache = pob_data::mod_cache::mod_cache();
    let mut mods = 0;
    for e in &cache.entries {
        let p = parse_mod_line(&e.line, false, ParseMode::CacheOnly);
        assert!(p.hit, "{}", e.line);
        assert_eq!(p.extra.as_deref(), e.extra.as_deref(), "{}", e.line);
        match (&p.mod_list, &e.mods) {
            (None, None) => {}
            (Some(got), Some(want)) => {
                assert_eq!(got.len(), want.len());
                for (g, w) in got.iter().zip(want) {
                    assert!(same_mod(g, w), "{}", e.line);
                }
                mods += got.len();
            }
            _ => panic!("modList nil-ness differs: {}", e.line),
        }
    }
    let s = parse_mod_stats();
    // 3dbca4463 (R-UP): 23 201 records, 23 334 mods (data/mod-cache.json counted by node; pob-data tests/loaders.rs)
    assert_eq!((s.hits, s.misses), (23_201, 0));
    assert_eq!(mods, 23_334);
}

#[test]
fn extra_survives_a_nil_mod_list() {
    // W16: `unpack(copyTable({nil, extra}))` -- LuaJIT keeps extra.
    // " Wolf Alpha Talisman" left ModCache.lua in 3dbca4463; another `{nil, extra}` record (ModCache.lua:4)
    let p = parse_mod_line("(10-15)% increased Energy Shield Recharge Rate", false, ParseMode::CacheOnly);
    assert!(p.hit && p.mod_list.is_none());
    assert_eq!(p.extra.as_deref(), Some("(10-15)% increased Energy Shield Recharge Rate "));
}

#[test]
fn a_copy_is_independent_of_the_cache() {
    let line = "+1 to Level of Socketed AoE Gems";
    let mut a = parse_mod_line(line, false, ParseMode::CacheOnly).mod_list.unwrap();
    a[0].name = Rc::from("Mutated");
    a[0].value = Value::Num(99.0);
    a[0].tags.clear();
    let b = parse_mod_line(line, false, ParseMode::CacheOnly).mod_list.unwrap();
    assert_eq!(&*b[0].name, "GemProperty");
    assert!(matches!(&b[0].value, Value::Table(t) if t.get("keyword").as_str() == Some("aoe")));
    assert_eq!(b[0].tags.len(), 1);
    assert_eq!(b[0].tags[0].get("slotName").as_str(), Some("{SlotName}"));

    // a nested `{ mod = createMod(...) }` is a new object per call
    let cache = pob_data::mod_cache::mod_cache();
    let nested = cache
        .entries
        .iter()
        .find(|e| {
            e.mods
                .as_ref()
                .and_then(|m| m.first())
                .is_some_and(|m| matches!(&m.value, Value::Table(t) if matches!(t.get("mod"), Value::Mod(_))))
        })
        .expect("a LIST mod with a nested mod");
    let x = parse_mod_line(&nested.line, false, ParseMode::CacheOnly).mod_list.unwrap();
    let y = parse_mod_line(&nested.line, false, ParseMode::CacheOnly).mod_list.unwrap();
    let inner = |m: &pob_mods::Mod| match &m.value {
        Value::Table(t) => match t.get("mod") {
            Value::Mod(i) => i.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    assert!(!Rc::ptr_eq(&inner(&x[0]), &inner(&y[0])));
    assert!(!Rc::ptr_eq(&inner(&x[0]), &inner(&nested.mods.as_ref().unwrap()[0])));
}

/// MINEFIELD-RUST R4: EvalMod writes `tag.div` into the tag object; on a copy that write must not reach the original.
/// The cache holds no `divVar` tag (0 of 23 324 mods), so the copy routine is checked on a synthetic mod.
#[test]
fn tag_div_written_on_a_copy_stays_out_of_the_original() {
    let tag = Tag::new(Table {
        hash: [
            ("type", Value::str("Multiplier")),
            ("var", Value::str("A")),
            ("divVar", Value::str("B")),
        ]
        .into_iter()
        .map(|(k, v)| (Rc::from(k), v))
        .collect(),
        ..Table::default()
    });
    let original = create_mod("X", Some(ModType::Base), Value::Num(2.0), None, 0, 0, vec![tag]);
    let copy = copy_table_mod(&original);
    let db = ModStore::new_db(None);
    db.borrow_mut().multipliers.insert(Rc::from("B"), 4.0);
    db.borrow_mut().add_mod(Rc::new(copy.clone()));
    let _ = db.borrow().sum(ModType::Base, None, &["X"]).unwrap();
    assert_eq!(copy.tags[0].get("div").as_num(), Some(4.0), "EvalMod wrote tag.div into the copy");
    assert!(original.tags[0].get("div").is_nil(), "the original's tag is untouched");
}

#[test]
fn a_miss_answers_nil_and_the_line() {
    reset_parse_mod_stats();
    let line = "This line is not in ModCache, synthetic R3.2";
    for _ in 0..2 {
        let p = parse_mod_line(line, true, ParseMode::CacheOnly);
        assert!(!p.hit && p.mod_list.is_none());
        assert_eq!(p.extra.as_deref(), Some(line));
    }
    let s = parse_mod_stats();
    assert_eq!((s.hits, s.misses), (0, 2));
    assert_eq!(s.miss_lines, [line]);
}
