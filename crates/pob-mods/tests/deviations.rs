//! Places where pob-mods deliberately departs from a literal port, pinned by tests (R2.2 DoD), plus formatTag shapes
//! the corpora do not reach (MOD-CORE §7.15).

use std::rc::Rc;

use pob_mods::{ModStore, ModType, Str, Table, Tag, Value, create_mod, format_tag};

fn tag(fields: &[(&str, Value)]) -> Tag {
    Tag::new(Table {
        hash: fields.iter().map(|(k, v)| (Str::from(*k), v.clone())).collect(),
        ..Table::default()
    })
}

/// ModStore.lua:401 writes `tag.div = GetMultiplier(self, tag.divVar, cfg)` into the shared tag (MOD-CORE §7.5
/// bug-compat): every holder of the same tag object sees it -- `get("div")`, Dump/`ser`, `formatTag` -- while a Lua
/// `copyTable` (CopyList) gets its own tag. Corpus case `tag-div-mutation` checks the Dump against LuaJIT.
#[test]
fn div_var_is_written_into_the_shared_tag() {
    let db = ModStore::new_db(None);
    let t = tag(&[
        ("type", Value::str("Multiplier")),
        ("var", Value::str("V")),
        ("divVar", Value::str("D")),
    ]);
    let m = Rc::new(create_mod("M", Some(ModType::Base), Value::Num(10.0), None, 0, 0, vec![t.clone()]));
    let copy = ModStore::new_list(None);
    {
        let mut s = db.borrow_mut();
        s.multipliers.insert(Str::from("V"), 7.0);
        s.multipliers.insert(Str::from("D"), 2.0);
        s.add_mod(m.clone());
    }
    copy.borrow_mut().copy_list(std::slice::from_ref(&m)); // copyTable before any evaluation: no div
    assert!(t.get("div").is_nil());
    assert_eq!(format_tag(&t), "type=Multiplier/divVar=D/var=V");
    assert_eq!(db.borrow().sum(ModType::Base, None, &["M"]).unwrap(), 30.0); // floor(7/2 + 1e-4) = 3
    assert_eq!(
        t.get("div").as_num(),
        Some(2.0),
        "the write is visible through the caller's tag object"
    );
    assert_eq!(format_tag(&t), "type=Multiplier/div=2/divVar=D/var=V");
    db.borrow_mut().multipliers.insert(Str::from("D"), 7.0);
    assert_eq!(db.borrow().sum(ModType::Base, None, &["M"]).unwrap(), 10.0); // recomputed: floor(7/7 + 1e-4) = 1
    assert_eq!(t.get("div").as_num(), Some(7.0));
    let pob_mods::StoreKind::List(l) = &copy.borrow().kind else {
        unreachable!()
    };
    assert!(l.mods[0].tags[0].get("div").is_nil(), "a copyTable'd mod has its own tag");
}

/// GetStat("ManaUnreserved") with a NaN output and no `output.Mana`: ModStore.lua:349 returns `output["Mana"]` -- nil.
#[test]
fn mana_unreserved_nan_without_mana_is_nil() {
    let db = ModStore::new_db(None);
    {
        let s = db.borrow();
        let mut a = s.actor.borrow_mut();
        a.output = Some([(Str::from("ManaUnreserved"), f64::NAN)].into_iter().collect());
    }
    assert!(db.borrow().get_stat("ManaUnreserved", None).unwrap().is_nil());
    // ... and a PerStat over it dies where LuaJIT does: `m_floor(base / (tag.div or 1) + 0.0001)`, ModStore.lua:501.
    let per = tag(&[("type", Value::str("PerStat")), ("stat", Value::str("ManaUnreserved"))]);
    let m = create_mod("X", Some(ModType::Base), Value::Num(1.0), None, 0, 0, vec![per]);
    let e = db.borrow().eval_mod(&m, None, None).unwrap_err();
    assert_eq!(
        e.0,
        "Classes/ModStore.lua:501: attempt to perform arithmetic on local 'base' (a nil value)"
    );
}

/// MOD-CORE §7.15: a list of tables prints with 1-based keys, a list of scalars joined by ",".
#[test]
fn format_tag_lists() {
    let pair = |a: f64, b: f64| {
        Value::table(Table {
            array: vec![Value::Num(a), Value::Num(b)],
            ..Table::default()
        })
    };
    let ramp = Value::table(Table {
        array: vec![pair(10.0, 1.5), pair(40.0, 0.0)],
        ..Table::default()
    });
    let t = tag(&[("type", Value::str("DistanceRamp")), ("ramp", ramp)]);
    assert_eq!(format_tag(&t), "type=DistanceRamp/ramp={1=10/2=1.5,1=40/2=0}");
    let list = Value::table(Table {
        array: vec![Value::str("a"), Value::str("b")],
        ..Table::default()
    });
    let t = tag(&[("varList", list), ("type", Value::str("Condition")), ("neg", Value::Bool(true))]);
    assert_eq!(format_tag(&t), "type=Condition/neg=true/varList={a,b}");
}
