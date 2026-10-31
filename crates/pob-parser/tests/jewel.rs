//! R5.4: the `jewelFuncList` function values (decision 2) -- `parseMod` builds them (`call_caps`), the radius-jewel code
//! of R5.5 runs them (`visit`). Here: a synthetic node, the visitor bodies of ModParser.lua:6151-6593 and :2621/:2629.

use std::rc::Rc;

use pob_mods::{LuaError, Mod, ModStore, ModType, Table, Value, create_mod};
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

#[test]
fn a_visitor_without_its_upvalues_is_explicit() {
    // data gap (R5.5): the getSimpleConv visitors referenced from jewelOtherFuncs carry no upvalues in mod-parser.json
    let (_, f, _) = func_of("Increases and Reductions to Life in Radius are Transformed to apply to Energy Shield");
    let mut out = list();
    let mut d = data("Item:6:Jewel");
    let e = visit(&f, Some(&node("Normal", vec![base("Life", 5.0)])), &mut out, &mut d).unwrap_err();
    assert!(matches!(e, ParseError::Unrepresentable(_)), "{e}");
}
