//! The Rust half of the ModStore micro-oracle (MOD-CORE §9): the analogue of pob-ts `test/mods/harness.ts` (`runCaseOnTs`).
//! A `ModStoreCase` (pob-ts `tools/oracle/modstore-oracle.ts`) builds the same stores the Lua half
//! (`tools/oracle/lua/modstore-oracle.lua`) builds inside PoB, applies the ops, answers the queries, and serialises every
//! answer the way the oracle's `ser`/`serMod`/`dumpStore` do. `same` is `compareRuns`/`sameValue`: numbers bit for bit
//! (NaN == NaN, -0 != 0), errors by text.
//! Shared by `tests/modstore_corpus.rs` and `examples/bench_modstore.rs` (`#[path]`), hence the dead-code allowance.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_mods::{
    ActiveSkill, Actor, Gem, GrantedEffect, ItemLike, LuaError, Mod, ModCfg, ModStore, ModType, StoreKind, StoreRef, Str, TabEntry, Table,
    Tag, Value, create_mod,
};
use serde_json::Value as J;

/// `OracleValue` after `normalize`: numbers are doubles again.
#[derive(Clone, Debug)]
pub enum O {
    N(f64),
    B(bool),
    S(String),
    Nil,
    T(BTreeMap<String, O>),
    Mod {
        name: String,
        ty: Option<String>,
        value: Box<O>,
        flags: f64,
        keyword_flags: f64,
        source: Box<O>,
        tags: Vec<O>,
    },
    Err(String),
}

/// `sameValue` (modstore-oracle.ts): `Object.is` for numbers, structural for tables and mods.
pub fn same(a: &O, b: &O) -> bool {
    match (a, b) {
        (O::N(x), O::N(y)) => (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits(),
        (O::B(x), O::B(y)) => x == y,
        (O::S(x), O::S(y)) | (O::Err(x), O::Err(y)) => x == y,
        (O::Nil, O::Nil) => true,
        (O::T(x), O::T(y)) => x.len() == y.len() && x.iter().zip(y).all(|((ka, va), (kb, vb))| ka == kb && same(va, vb)),
        (
            O::Mod {
                name: n1,
                ty: t1,
                value: v1,
                flags: f1,
                keyword_flags: k1,
                source: s1,
                tags: g1,
            },
            O::Mod {
                name: n2,
                ty: t2,
                value: v2,
                flags: f2,
                keyword_flags: k2,
                source: s2,
                tags: g2,
            },
        ) => {
            n1 == n2
                && t1 == t2
                && f1 == f2
                && k1 == k2
                && same(v1, v2)
                && same(s1, s2)
                && g1.len() == g2.len()
                && g1.iter().zip(g2).all(|(x, y)| same(x, y))
        }
        _ => false,
    }
}

// --- the fixture side ----------------------------------------------------------------------------------------------

fn parse_num(s: &str) -> f64 {
    match s {
        "nan" => f64::NAN,
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        // `%.17g` text names exactly one double; std's parser is correctly rounded (test code, not ported code).
        _ => s.parse().unwrap_or_else(|_| panic!("fixture: '{s}' is not a %.17g number")),
    }
}

/// `normalize(OracleValue)`. dkjson writes an empty table as `[]` (O2).
pub fn expected(j: &J) -> O {
    let k = j["k"].as_str().expect("OracleValue.k");
    let v = &j["v"];
    match k {
        "n" => O::N(parse_num(v.as_str().expect("n.v"))),
        "b" => O::B(v.as_bool().expect("b.v")),
        "s" => O::S(v.as_str().expect("s.v").to_owned()),
        "nil" => O::Nil,
        "error" => O::Err(v.as_str().expect("error.v").to_owned()),
        "t" => match v {
            J::Array(a) if a.is_empty() => O::T(BTreeMap::new()),
            J::Object(m) => O::T(m.iter().map(|(k, x)| (k.clone(), expected(x))).collect()),
            other => panic!("fixture: bad table {other}"),
        },
        "mod" => O::Mod {
            name: v["name"].as_str().expect("mod.name").to_owned(),
            ty: v["type"].as_str().map(str::to_owned),
            value: Box::new(expected(&v["value"])),
            flags: v["flags"].as_f64().expect("mod.flags"),
            keyword_flags: v["keywordFlags"].as_f64().expect("mod.keywordFlags"),
            source: Box::new(expected(&v["source"])),
            tags: v["tags"].as_array().expect("mod.tags").iter().map(expected).collect(),
        },
        other => panic!("fixture: unknown OracleValue kind '{other}'"),
    }
}

// --- the Rust side: ser / serMod / dumpStore -----------------------------------------------------------------------

/// The oracle's `fmt17` for a numeric table key.
fn fmt17_key(i: usize) -> String {
    pob_lua::format_g(i as f64, 17)
}

fn ser_mod(m: &Mod) -> O {
    O::Mod {
        name: m.name.to_string(),
        ty: m.ty.map(|t| t.as_str().to_owned()),
        value: Box::new(ser(&m.value)),
        flags: f64::from(m.flags),
        keyword_flags: f64::from(m.keyword_flags),
        source: Box::new(m.source.as_ref().map_or(O::Nil, |s| O::S(s.to_string()))),
        tags: m.tags.iter().map(|t| ser(&Value::Table(t.table()))).collect(),
    }
}

/// A plain Lua table that carries name+flags+keywordFlags is serialised as a mod (a LIST value's nested `mod` built from
/// JSON); its tags are its array part.
fn ser_table(t: &Table) -> O {
    if !t.get("name").is_nil() && !t.get("flags").is_nil() && !t.get("keywordFlags").is_nil() {
        let num = |k: &str| {
            t.get(k)
                .as_num()
                .unwrap_or_else(|| panic!("harness: mod table field {k} is not a number"))
        };
        return O::Mod {
            name: t.get("name").as_str().expect("harness: mod table name").to_owned(),
            ty: t.get("type").as_str().map(str::to_owned),
            value: Box::new(ser(t.get("value"))),
            flags: num("flags"),
            keyword_flags: num("keywordFlags"),
            source: Box::new(ser(t.get("source"))),
            tags: t.array.iter().map(ser).collect(),
        };
    }
    let mut out = BTreeMap::new();
    for (i, v) in t.array.iter().enumerate() {
        out.insert(fmt17_key(i + 1), ser(v));
    }
    for (k, v) in &t.hash {
        // `pairs` never sees a nil field.
        if !v.is_nil() {
            out.insert(k.to_string(), ser(v));
        }
    }
    O::T(out)
}

pub fn ser(v: &Value) -> O {
    match v {
        Value::Nil => O::Nil,
        Value::Bool(b) => O::B(*b),
        Value::Num(n) => O::N(*n),
        Value::Str(s) => O::S(s.to_string()),
        Value::Table(t) => ser_table(t),
        Value::Mod(m) => ser_mod(m),
        Value::Func(_) => O::S(pob_mods::FUNCTION_TOSTRING.to_owned()),
    }
}

fn ser_list(items: impl Iterator<Item = O>) -> O {
    O::T(items.enumerate().map(|(i, o)| (fmt17_key(i + 1), o)).collect())
}

fn ser_tab(entries: &[TabEntry]) -> O {
    ser_list(entries.iter().map(|e| {
        let mut m = BTreeMap::new();
        m.insert("value".to_owned(), ser(&e.value));
        m.insert("mod".to_owned(), ser_mod(&e.mod_));
        O::T(m)
    }))
}

/// `dumpStore`: ModDB buckets by name in byte order (empty ones included), ModList in array order.
fn dump_store(s: &ModStore) -> O {
    match &s.kind {
        StoreKind::Db(db) => {
            let mut out = BTreeMap::new();
            for (name, bucket) in &db.mods {
                out.insert(name.to_string(), ser_list(bucket.iter().map(|m| ser_mod(m))));
            }
            O::T(out)
        }
        StoreKind::List(l) => ser_list(l.mods.iter().map(|m| ser_mod(m))),
    }
}

// --- building ------------------------------------------------------------------------------------------------------

/// JSON (dkjson-decoded) into a Lua value: arrays are the array part, objects the string keys.
pub fn lua_value(j: &J) -> Value {
    match j {
        J::Null => panic!("harness: JSON null decodes to nil in dkjson -- not expressible in a case"),
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => Value::Num(n.as_f64().expect("number")),
        J::String(s) => Value::str(s),
        J::Array(a) => Value::table(Table {
            array: a.iter().map(lua_value).collect(),
            ..Table::default()
        }),
        J::Object(m) => Value::table(Table {
            hash: m.iter().map(|(k, v)| (Str::from(k.as_str()), lua_value(v))).collect(),
            ..Table::default()
        }),
    }
}

fn lua_table(j: &J) -> Table {
    match lua_value(j) {
        Value::Table(t) => Rc::unwrap_or_clone(t),
        _ => panic!("harness: expected a JSON object"),
    }
}

fn opt_str(j: &J) -> Option<Str> {
    j.as_str().map(Str::from)
}

fn flags(j: &J) -> u32 {
    let f = j.as_f64().unwrap_or(0.0);
    assert!(
        f >= 0.0 && f.fract() == 0.0 && f < 4_294_967_296.0,
        "harness: flags {f} is not a u32"
    );
    f as u32
}

/// `modLib.createMod(name, type, value, source, flags or 0, keywordFlags or 0, unpack(tags))`.
pub fn build_mod(spec: &J) -> Mod {
    let ty = spec["type"].as_str().expect("mod.type");
    let ty = ModType::parse(ty).unwrap_or_else(|| panic!("harness: unknown mod type '{ty}'"));
    let tags = spec["tags"]
        .as_array()
        .map_or_else(Vec::new, |a| a.iter().map(|t| Tag::new(lua_table(t))).collect());
    let value = if spec["value"].is_null() {
        Value::Nil
    } else {
        lua_value(&spec["value"])
    };
    create_mod(
        spec["name"].as_str().expect("mod.name"),
        Some(ty),
        value,
        spec["source"].as_str(),
        flags(&spec["flags"]),
        flags(&spec["keywordFlags"]),
        tags,
    )
}

fn build_mods(specs: &J) -> Vec<Rc<Mod>> {
    specs.as_array().expect("mods").iter().map(|m| Rc::new(build_mod(m))).collect()
}

/// The Item STUB of the oracle's `buildItem`: `FindModifierSubstring` is any mod line containing the substring (plain
/// find, lowercased) that is not the "<substring> modifier" line -- the reduced Item.lua:289-312; the real Item is R3.
pub struct StubItem {
    name: String,
    ty: String,
    rarity: Option<String>,
    corrupted: Option<bool>,
    shaper: Option<bool>,
    elder: Option<bool>,
    mod_lines: Vec<String>,
}

impl ItemLike for StubItem {
    fn name(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn item_type(&self) -> Option<&str> {
        Some(&self.ty)
    }
    fn rarity(&self) -> Option<&str> {
        self.rarity.as_deref()
    }
    fn corrupted(&self) -> Option<bool> {
        self.corrupted
    }
    fn shaper(&self) -> Option<bool> {
        self.shaper
    }
    fn elder(&self) -> Option<bool> {
        self.elder
    }
    fn find_modifier_substring(&self, substring: &str, _slot: &str) -> bool {
        let modifier = format!("{substring} modifier");
        self.mod_lines.iter().any(|line| {
            let low = pob_lua::string::lower(line);
            low.contains(substring) && !low.contains(&modifier)
        })
    }
}

fn build_item(j: &J) -> Rc<dyn ItemLike> {
    Rc::new(StubItem {
        name: j["name"].as_str().expect("item.name").to_owned(),
        ty: j["type"].as_str().expect("item.type").to_owned(),
        rarity: j["rarity"].as_str().map(str::to_owned),
        corrupted: j["corrupted"].as_bool(),
        shaper: j["shaper"].as_bool(),
        elder: j["elder"].as_bool(),
        mod_lines: j["modLines"]
            .as_array()
            .map_or_else(Vec::new, |a| a.iter().map(|l| l.as_str().expect("modLine").to_owned()).collect()),
    })
}

fn num_map(j: &J) -> IndexMap<Str, f64> {
    j.as_object()
        .expect("number map")
        .iter()
        .map(|(k, v)| (Str::from(k.as_str()), v.as_f64().expect("number")))
        .collect()
}

fn bool_map(j: &J) -> IndexMap<Str, bool> {
    j.as_object()
        .expect("bool map")
        .iter()
        .map(|(k, v)| (Str::from(k.as_str()), v.as_bool().expect("bool")))
        .collect()
}

fn num_list(j: &J) -> Vec<f64> {
    j.as_array()
        .expect("number list")
        .iter()
        .map(|v| v.as_f64().expect("number"))
        .collect()
}

/// ModStore.lua:42 -- with a parent, `store.actor` IS the parent's table, so this writes into the shared actor.
fn apply_actor(actor: &mut Actor, spec: &J, stores: &IndexMap<String, StoreRef>) {
    if !spec["output"].is_null() {
        actor.output = Some(num_map(&spec["output"]));
    }
    if !spec["weaponData1"].is_null() {
        actor.weapon_data1 = Some(Rc::new(lua_table(&spec["weaponData1"])));
    }
    if !spec["weaponData2"].is_null() {
        actor.weapon_data2 = Some(Rc::new(lua_table(&spec["weaponData2"])));
    }
    if !spec["minionData"].is_null() {
        let tags = spec["minionData"]["monsterTags"]
            .as_array()
            .map(|a| a.iter().map(|t| Str::from(t.as_str().expect("monsterTag"))).collect());
        actor.minion_data = Some(tags);
    }
    if let Some(e) = spec["ManaEfficiency"].as_f64() {
        actor.mana_efficiency = Some(e);
    }
    if let Some(items) = spec["itemList"].as_object() {
        actor.item_list = Some(items.iter().map(|(slot, it)| (Str::from(slot.as_str()), build_item(it))).collect());
    }
    if let Some(list) = spec["activeSkillList"].as_array() {
        actor.active_skill_list = Some(
            list.iter()
                .map(|s| ActiveSkill {
                    skill_types: num_list(&s["skillTypes"]),
                    disable: s["disable"].as_bool(),
                    buff_list: Some(
                        s["buffNames"]
                            .as_array()
                            .map_or_else(Vec::new, |a| a.iter().map(|n| Str::from(n.as_str().expect("buff"))).collect()),
                    ),
                    skill_data: if s["skillData"].is_null() {
                        IndexMap::new()
                    } else {
                        num_map(&s["skillData"])
                    },
                })
                .collect(),
        );
    }
    if let Some(actors) = spec["actors"].as_object() {
        for (name, id) in actors {
            let db = stores
                .get(id.as_str().expect("actor store id"))
                .unwrap_or_else(|| panic!("harness: unknown actor store {id}"));
            let a = Actor {
                mod_db: Some(db.clone()),
                ..Actor::default()
            };
            actor.actors.insert(Str::from(name.as_str()), Rc::new(std::cell::RefCell::new(a)));
        }
    }
}

fn build_store(spec: &J, stores: &IndexMap<String, StoreRef>) -> StoreRef {
    let parent = spec["parent"]
        .as_str()
        .map(|p| stores.get(p).unwrap_or_else(|| panic!("harness: unknown parent {p}")).clone());
    let store = match spec["kind"].as_str() {
        Some("ModDB") => ModStore::new_db(parent),
        Some("ModList") => ModStore::new_list(parent),
        k => panic!("harness: unknown store kind {k:?}"),
    };
    {
        let mut s = store.borrow_mut();
        if let Some(c) = spec["conditions"].as_object() {
            for (k, v) in c {
                s.conditions.insert(Str::from(k.as_str()), v.as_bool().expect("condition"));
            }
        }
        if let Some(m) = spec["multipliers"].as_object() {
            for (k, v) in m {
                s.multipliers.insert(Str::from(k.as_str()), v.as_f64().expect("multiplier"));
            }
        }
        if !spec["actor"].is_null() {
            let actor = s.actor();
            apply_actor(&mut actor.borrow_mut(), &spec["actor"], stores);
        }
        if let Some(mods) = spec["mods"].as_array() {
            for m in mods {
                s.add_mod(Rc::new(build_mod(m)));
            }
        }
    }
    store
}

/// ModStore.lua:56-143 through the public mutators, in the oracle's order. Not caught: a broken case fails loudly.
fn apply_op(op: &J, stores: &IndexMap<String, StoreRef>) {
    let store = stores.get(op["store"].as_str().expect("op.store")).expect("op store");
    let replace = op["replace"].as_bool().unwrap_or(false);
    let r: Result<(), LuaError> = match op["op"].as_str().expect("op.op") {
        "NewMod" => {
            store.borrow_mut().new_mod(build_mod(&op["mod"]));
            Ok(())
        }
        "ReplaceMod" => {
            store.borrow_mut().replace_mod(build_mod(&op["mod"]));
            Ok(())
        }
        "ConvertMod" => {
            store
                .borrow_mut()
                .convert_mod(op["oldName"].as_str().expect("oldName"), build_mod(&op["mod"]));
            Ok(())
        }
        "ScaleAddMod" => store
            .borrow_mut()
            .scale_add_mod(&Rc::new(build_mod(&op["mod"])), op["scale"].as_f64().expect("scale"), replace),
        "ScaleAddList" => store
            .borrow_mut()
            .scale_add_list(&build_mods(&op["mods"]), op["scale"].as_f64().expect("scale"), replace),
        "CopyList" => {
            store.borrow_mut().copy_list(&build_mods(&op["mods"]));
            Ok(())
        }
        "AddList" => {
            store.borrow_mut().add_list(&build_mods(&op["mods"]));
            Ok(())
        }
        "MergeMod" => store
            .borrow_mut()
            .merge_mod(Rc::new(build_mod(&op["mod"])), op["skipNonAdditive"].as_bool().unwrap_or(false)),
        "AddDB" => {
            let from = stores.get(op["from"].as_str().expect("from")).expect("AddDB source");
            // Snapshot the source first: `AddDB(self)` must append each bucket once (`for i = 1, #modList`).
            let src = match &from.borrow().kind {
                StoreKind::Db(db) => db.clone(),
                StoreKind::List(_) => panic!("harness: AddDB from a ModList"),
            };
            store.borrow_mut().add_db(&src)
        }
        "SetCondition" => {
            store
                .borrow_mut()
                .conditions
                .insert(Str::from(op["var"].as_str().expect("var")), op["value"].as_bool().expect("value"));
            Ok(())
        }
        "SetMultiplier" => {
            store
                .borrow_mut()
                .multipliers
                .insert(Str::from(op["var"].as_str().expect("var")), op["value"].as_f64().expect("value"));
            Ok(())
        }
        other => panic!("harness: unknown op {other}"),
    };
    r.unwrap_or_else(|e| panic!("harness: op {op} died: {e}"));
}

/// `buildCfg`: `skillTypes` arrives as a list and is the set the SkillType tag indexes; `item` is a stub Item.
pub fn build_cfg(j: &J) -> Option<ModCfg> {
    let obj = j.as_object()?;
    let gem = |g: &J| Gem {
        name: Str::from(g["name"].as_str().expect("gem.name")),
        tags: bool_map(&g["tags"]),
    };
    let mut c = ModCfg::default();
    // One pass over the fields present (the cfg schema is strict: every key is one of these).
    for (k, v) in obj {
        match k.as_str() {
            "flags" => c.flags = v.as_f64(),
            "keywordFlags" => c.keyword_flags = v.as_f64(),
            "source" => c.source = opt_str(v),
            "skillName" => c.skill_name = opt_str(v),
            "summonSkillName" => c.summon_skill_name = opt_str(v),
            "skillDist" => c.skill_dist = v.as_f64(),
            "skillCond" => c.skill_cond = Some(bool_map(v)),
            "actor" => c.actor = opt_str(v),
            "item" => c.item = Some(build_item(v)),
            "slotName" => c.slot_name = opt_str(v),
            "skillGem" => c.skill_gem = Some(gem(v)),
            "socketColor" => c.socket_color = opt_str(v),
            "socketNum" => c.socket_num = v.as_f64(),
            "strengthGems" => c.strength_gems = v.as_f64(),
            "dexterityGems" => c.dexterity_gems = v.as_f64(),
            "intelligenceGems" => c.intelligence_gems = v.as_f64(),
            "skillGrantedEffect" => {
                c.skill_granted_effect = Some(GrantedEffect {
                    id: Str::from(v["id"].as_str().expect("skillGrantedEffect.id")),
                    base_flags: (!v["baseFlags"].is_null()).then(|| bool_map(&v["baseFlags"])),
                })
            }
            "baseFlags" => c.base_flags = Some(bool_map(v)),
            "skillPart" => c.skill_part = v.as_f64(),
            "skillTypes" => c.skill_types = Some(num_list(v)),
            "skillStats" => c.skill_stats = Some(num_map(v)),
            other => panic!("harness: unknown cfg field {other}"),
        }
    }
    Some(c)
}

/// The stores of one case after its ops. Dropping it breaks the actor links (store -> actor -> store cycles).
pub struct Built {
    pub stores: IndexMap<String, StoreRef>,
}

impl Drop for Built {
    fn drop(&mut self) {
        for s in self.stores.values() {
            s.borrow().actor().borrow_mut().actors.clear();
        }
    }
}

/// Stores in order (a parent before its children), then ops in order.
pub fn build_case(c: &J) -> Built {
    assert_eq!(c["format"].as_f64(), Some(1.0), "harness: unsupported case format");
    let mut stores = IndexMap::new();
    for spec in c["stores"].as_array().expect("stores") {
        let s = build_store(spec, &stores);
        stores.insert(spec["id"].as_str().expect("store id").to_owned(), s);
    }
    for op in c["ops"].as_array().map_or(&[][..], |v| v) {
        apply_op(op, &stores);
    }
    Built { stores }
}

fn ty_of(j: &J) -> ModType {
    let t = j.as_str().expect("query type");
    ModType::parse(t).unwrap_or_else(|| panic!("harness: unknown type {t}"))
}

/// One query, caught the way the oracle `pcall`s it: a Lua error is the answer `{k:'error', v}`.
pub fn run_query(q: &J, b: &Built) -> O {
    let store = b
        .stores
        .get(q["store"].as_str().expect("query store"))
        .expect("query store")
        .borrow();
    let method = q["method"].as_str().expect("method");
    if method == "Dump" {
        return dump_store(&store);
    }
    let cfg = build_cfg(&q["cfg"]);
    let cfg = cfg.as_ref();
    let names: Vec<&str> = q["names"]
        .as_array()
        .map_or_else(Vec::new, |a| a.iter().map(|n| n.as_str().expect("name")).collect());
    let r: Result<O, LuaError> = (|| {
        Ok(match method {
            "Sum" => O::N(store.sum(ty_of(&q["type"]), cfg, &names)?),
            "More" => O::N(store.more(cfg, &names)?),
            "Flag" => ser(&Value::from(store.flag(cfg, &names)?)),
            "Override" => ser(&store.override_(cfg, &names)?.unwrap_or_default()),
            "List" => ser_list(store.list(cfg, &names)?.iter().map(ser)),
            "Max" => ser(&Value::from(store.max(cfg, &names)?)),
            "Min" => ser(&store.min(cfg, &names)?),
            "Tabulate" => {
                let ty = if q["type"].is_null() { None } else { Some(ty_of(&q["type"])) };
                ser_tab(&store.tabulate(ty, cfg, &names)?)
            }
            "HasMod" => O::B(store.has_mod(ty_of(&q["type"]), cfg, &names)?),
            "Combine" => ser(&store.combine(ty_of(&q["type"]), cfg, &names)?),
            "GetCondition" => ser(&Value::from(store.get_condition(
                q["var"].as_str().expect("var"),
                cfg,
                q["noMod"].as_bool().unwrap_or(false),
            )?)),
            "GetMultiplier" => ser(&store.get_multiplier(q["var"].as_str().expect("var"), cfg, q["noMod"].as_bool().unwrap_or(false))?),
            "GetStat" => ser(&store.get_stat(q["stat"].as_str().expect("stat"), cfg)?),
            "EvalMod" => {
                let m = build_mod(&q["mod"]);
                let mut gl = IndexMap::new();
                let gl = if q["globalLimits"].as_bool() == Some(true) {
                    Some(&mut gl)
                } else {
                    None
                };
                ser(&store.eval_mod(&m, cfg, gl)?)
            }
            other => panic!("harness: unknown query method {other}"),
        })
    })();
    r.unwrap_or_else(|e| O::Err(e.0))
}

/// `runCaseOnTs`: every query's serialised answer.
pub fn run_case(c: &J) -> Vec<O> {
    let b = build_case(c);
    c["queries"].as_array().expect("queries").iter().map(|q| run_query(q, &b)).collect()
}
