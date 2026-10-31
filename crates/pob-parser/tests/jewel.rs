//! R5.4: the `jewelFuncList` function values (decision 2) -- `parseMod` builds them (`call_caps`), the radius-jewel code
//! of R5.5 runs them (`visit`). Here: a synthetic node, the visitor bodies of ModParser.lua:6151-6593 and :2621/:2629.

use std::rc::Rc;

use pob_data::patches::Golden;
use pob_mods::{LuaError, Mod, ModStore, ModType, Table, Tag, Value, create_mod};
use pob_parser::helpers::JewelNode;
use pob_parser::jewel::visit;
use pob_parser::{ParseError, parse_mod_uncached};

struct Node {
    ty: &'static str,
    mods: Vec<Rc<Mod>>,
    tattoo: bool,
    conquered: bool,
}

impl JewelNode for Node {
    fn mod_list(&self) -> &[Rc<Mod>] {
        &self.mods
    }
    fn is_tattoo(&self) -> bool {
        self.tattoo
    }
    fn node_type(&self) -> &str {
        self.ty
    }
    fn conquered_by(&self) -> bool {
        self.conquered
    }
    fn sum_base(&self, stat: &str) -> Result<f64, LuaError> {
        Ok(self
            .mods
            .iter()
            .filter(|m| &*m.name == stat && m.ty == Some(ModType::Base))
            .filter_map(|m| m.value.as_num())
            .sum())
    }
}

fn node(ty: &'static str, mods: Vec<Mod>) -> Node {
    Node {
        ty,
        mods: mods.into_iter().map(Rc::new).collect(),
        tattoo: false,
        conquered: false,
    }
}

fn base(name: &str, v: f64) -> Mod {
    create_mod(name, Some(ModType::Base), Value::Num(v), None, 0, 0, Vec::new())
}

/// The function value of the one mod a jewel line parses to, and the mod's name.
fn func_of(line: &str) -> (String, Value, Value) {
    let r = parse_mod_uncached(line).unwrap_or_else(|e| panic!("{line}: {e}"));
    let mods = r.mod_list.expect("a mod list");
    assert_eq!(mods.len(), 1, "{line}");
    let m = &mods[0];
    let f = m.value.get("func");
    assert!(matches!(f, Value::Func(_)), "{line}: func is {f:?}");
    (m.name.to_string(), f, m.value.get("type"))
}

fn data(src: &str) -> Table {
    let mut t = Table::default();
    t.hash.insert(Rc::from("modSource"), Value::str(src));
    t
}

fn list() -> ModStore {
    Rc::try_unwrap(ModStore::new_list(None)).ok().expect("fresh").into_inner()
}

fn mods(out: &ModStore) -> Vec<String> {
    match &out.kind {
        pob_mods::StoreKind::List(l) => l.mods.iter().map(|m| pob_mods::format::format_source_mod(m)).collect(),
        pob_mods::StoreKind::Db(_) => unreachable!(),
    }
}

#[test]
fn factory_with_a_capture_builds_a_visitor() {
    // :6303 (jewelOtherFuncs, wrapped by :6555)
    let (name, f, ty) = func_of("Passive Skills in Radius also grant 5% increased Armour");
    assert_eq!((name.as_str(), ty.as_str()), ("JewelFunc", Some("Other")));
    let mut out = list();
    let mut d = data("Item:1:Jewel");
    visit(&f, Some(&node("Normal", vec![])), &mut out, &mut d).unwrap();
    visit(&f, Some(&node("Keystone", vec![])), &mut out, &mut d).unwrap();
    let mut conquered = node("Normal", vec![]);
    conquered.conquered = true;
    visit(&f, Some(&conquered), &mut out, &mut d).unwrap(); // :6565 timeless nodes are skipped
    visit(&f, None, &mut out, &mut d).unwrap();
    assert_eq!(mods(&out), ["5|Item:1:Jewel|Armour|INC|-|-|-"]);
}

#[test]
fn get_per_stat_accumulates_then_adds() {
    // :6370 getPerStat("Life", "BASE", 0, "Int", 1 / 3) -- the data carries the upvalues
    let (_, f, ty) = func_of("Adds 1 to Maximum Life per 3 Intelligence Allocated in Radius");
    assert_eq!(ty.as_str(), Some("Self"));
    let mut out = list();
    out.add_mod(Rc::new(base("Int", 10.0)));
    let mut d = data("Item:2:Jewel");
    visit(&f, Some(&node("Normal", vec![])), &mut out, &mut d).unwrap();
    assert_eq!(d.get("Int").as_num(), Some(10.0));
    let mut fin = list();
    visit(&f, None, &mut fin, &mut d).unwrap();
    assert_eq!(mods(&fin), ["3|Item:2:Jewel|Life|BASE|-|-|-"]);
}

#[test]
fn threshold_factory_keeps_the_capture_as_given() {
    // :6513 `function(num) return getThreshold("Dex", "Speed", "INC", num, ...)` -- `num` is the capture STRING
    let (_, f, ty) = func_of("With at least 40 Dexterity in Radius, Dual Strike has 20% increased Attack Speed while wielding a Claw");
    assert_eq!(ty.as_str(), Some("Threshold"));
    let mut out = list();
    out.add_mod(Rc::new(base("Dex", 45.0)));
    let mut d = data("Item:3:Jewel");
    visit(&f, Some(&node("Normal", vec![])), &mut out, &mut d).unwrap();
    let mut fin = list();
    visit(&f, None, &mut fin, &mut d).unwrap();
    let got = mods(&fin);
    assert_eq!(got.len(), 1);
    assert!(got[0].starts_with("20|Item:3:Jewel|Speed|INC|"), "{got:?}");
    match &fin.kind {
        pob_mods::StoreKind::List(l) => assert!(matches!(l.mods[0].value, Value::Str(_)), "value stays a string"),
        pob_mods::StoreKind::Db(_) => unreachable!(),
    }
}

#[test]
fn extra_jewel_func_of_special_mod_list() {
    // :2629 non-unique jewels ... also grant +4 to Strength -> ExtraJewelFunc with the :2630 visitor
    let (name, f, ty) = func_of("Non-Unique Jewels cause Small and Notable Passive Skills in a Large Radius to also grant +4 to Strength");
    assert_eq!((name.as_str(), ty.as_str()), ("ExtraJewelFunc", Some("Other")));
    let mut out = list();
    let mut d = data("Item:4:Jewel");
    visit(&f, Some(&node("Notable", vec![])), &mut out, &mut d).unwrap();
    visit(&f, Some(&node("Keystone", vec![])), &mut out, &mut d).unwrap();
    assert_eq!(mods(&out), ["4|Item:4:Jewel|Str|BASE|-|-|-"]);
}

#[test]
fn melee_to_bow_transforms_weapon_mods() {
    // :6202 (an exact-key entry: the :6555 wrapper runs the visitor with the node as `cap1`)
    let (_, f, _) = func_of("Melee and Melee Weapon Type modifiers in Radius are Transformed to Bow Modifiers");
    let g = pob_data::globals::globals();
    let axe = g.mod_flag["Axe"] as u32;
    let m = create_mod(
        "PhysicalDamage",
        Some(ModType::Inc),
        Value::Num(10.0),
        Some(&Rc::from("Tree:1")),
        axe,
        0,
        Vec::new(),
    );
    let mut out = list();
    let mut d = data("Item:5:Jewel");
    visit(&f, Some(&node("Normal", vec![m])), &mut out, &mut d).unwrap();
    let got = mods(&out);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].starts_with("-10|Tree:1|PhysicalDamage|INC|"), "{got:?}");
    assert!(got[1].starts_with("10|Tree:1|PhysicalDamage|INC|"), "{got:?}");
}

// --- R5.5b0: the upvalues of the closures the converter did not write (data-patches/mod-parser-upvalues.json) ---------------

/// A mod of the LuaJIT scenario (`goldens.scenario`).
fn scen_mod(j: &Golden) -> Mod {
    let tags = j["tags"]
        .as_array()
        .expect("tags")
        .iter()
        .map(|t| {
            let mut tb = Table::default();
            for (k, v) in t.as_object().expect("tag") {
                tb.hash.insert(Rc::from(k.as_str()), Value::str(v.as_str().expect("tag field")));
            }
            Tag::new(tb)
        })
        .collect();
    create_mod(
        j["name"].as_str().expect("name"),
        ModType::parse(j["type"].as_str().expect("type")),
        Value::Num(j["value"].as_f64().expect("value")),
        j["source"].as_str(),
        j["flags"].as_u64().expect("flags") as u32,
        j["keywordFlags"].as_u64().expect("keywordFlags") as u32,
        tags,
    )
}

/// `jewelFuncList[key]` as the function value `parseMod` hands out (an exact-key entry: `{func = ..., type = ...}`).
fn entry_func(key: &str) -> Value {
    let d = pob_data::mod_parser::mod_parser();
    let e = d
        .table("jewelFuncList")
        .get(key)
        .unwrap_or_else(|| panic!("no jewelFuncList entry {key:?}"));
    pob_parser::jewel::pval_value(&e.field("func").expect("func")).unwrap()
}

#[test]
fn upvalue_patch_visitors_equal_luajit() {
    // LuaJIT ran the real `jewelFuncList[key].func(node, out, data)` on a synthetic scenario (3 nodes, then the final
    // `func(nil, out, data)`); `out` is rendered with modLib.formatSourceMod. One entry per key of the patch.
    let g = pob_data::patches::upvalue_goldens().unwrap();
    let patches = pob_data::patches::upvalue_patches().unwrap();
    let sc = &g["scenario"];
    let pre: Vec<Mod> = sc["pre"].as_array().unwrap().iter().map(scen_mod).collect();
    let nodes: Vec<(String, bool, Vec<Mod>)> = sc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            (
                n["type"].as_str().unwrap().to_owned(),
                n["isTattoo"].as_bool().unwrap(),
                n["mods"].as_array().unwrap().iter().map(scen_mod).collect(),
            )
        })
        .collect();
    let mut refs = 0;
    let entries = g["entries"].as_array().unwrap();
    for e in entries {
        let key = e["key"].as_str().unwrap();
        refs += e["refs"].as_array().unwrap().len();
        let f = entry_func(key);
        let mut out = list();
        for m in &pre {
            out.add_mod(Rc::new(m.clone()));
        }
        let mut d = data(sc["modSource"].as_str().unwrap());
        for (ty, tattoo, mods) in &nodes {
            let ty: &'static str = match ty.as_str() {
                "Normal" => "Normal",
                "Notable" => "Notable",
                other => panic!("scenario node type {other}"),
            };
            let mut n = node(ty, mods.clone());
            n.tattoo = *tattoo;
            visit(&f, Some(&n), &mut out, &mut d).unwrap_or_else(|e| panic!("{key}: {e}"));
        }
        visit(&f, None, &mut out, &mut d).unwrap_or_else(|e| panic!("{key}: {e}"));
        let want: Vec<&str> = e["out"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
        assert_eq!(mods(&out), want, "{key}");
    }
    assert_eq!(refs, patches.len(), "every patched closure is covered by a golden entry");
    assert!(patches.len() >= 13, "{} patched closures", patches.len());
    assert_eq!(entries.len(), 12);
}

#[test]
fn every_patched_closure_becomes_a_runnable_visitor() {
    // the 11 getSimpleConv visitors (:19) and the 2 thresholds (:6457) inside :6582 carry their upvalues now
    let patches = pob_data::patches::upvalue_patches().unwrap();
    let n19 = patches.iter().filter(|p| p.fn_id == "Modules/ModParser.lua:19").count();
    let n6457 = patches.iter().filter(|p| p.fn_id == "Modules/ModParser.lua:6457").count();
    assert_eq!((n19, n6457, patches.len()), (11, 2, 13));
    let d = pob_data::mod_parser::mod_parser();
    assert_eq!(d.fn_refs, 1_973, "fn_refs counts what the converter wrote");
}

#[test]
fn jewel_func_list_has_no_unrepresentable_visitor() {
    // every entry of jewelFuncList, driven the way parseMod / the radius-jewel code drive it: a key with captures is called with
    // captures (the factory returns the visitor), the others are the visitor itself. 0 `Unrepresentable` over the corpus.
    let d = pob_data::mod_parser::mod_parser();
    let t = d.table("jewelFuncList");
    let caps: Vec<Value> = (0..5).map(|_| Value::str("5")).collect();
    let n = node(
        "Normal",
        vec![
            base("Str", 5.0),
            base("Dex", 6.0),
            base("Int", 7.0),
            base("Life", 8.0),
            base("EnergyShield", 9.0),
        ],
    );
    let (mut checked, mut bad) = (0, Vec::new());
    for (key, v) in t.iter() {
        let func = pob_parser::jewel::pval_value(&v.field("func").expect("func")).unwrap();
        let f = if key.contains('(') {
            match pob_parser::jewel::call_caps(&func, &caps) {
                Ok(f) => f,
                Err(e) => {
                    bad.push(format!("{key}: call_caps: {e}"));
                    continue;
                }
            }
        } else {
            func
        };
        let mut out = list();
        let mut d = data("Item:9:Jewel");
        for node in [Some(&n as &dyn JewelNode), None] {
            checked += 1;
            if let Err(e) = visit(&f, node, &mut out, &mut d)
                && matches!(e, ParseError::Unrepresentable(_))
            {
                bad.push(format!("{key}: {e}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} of {checked} visits: {bad:#?}", bad.len());
    assert!(checked >= 2 * t.len());
}
