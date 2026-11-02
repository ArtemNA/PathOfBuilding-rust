//! `jewelFuncList` (ModParser.lua:6151-6593): the radius-jewel functions as Lua function VALUES (decision R5.4/2).
//!
//! A function value is `pob_mods::Value::Func(ClosureRef { id, up })` -- data, never run by the mod core. `up` is a
//! [`JewelFn`]: either a closure of the data (`Data`, its upvalues as `data/mod-parser.json` carries them) or a closure
//! built at run time by a factory (`SimpleConv`, `Threshold`, `Inner`, `OtherGuard`, the two `ExtraJewelFunc` visitors of
//! specialModList :2621/:2629). Two calls exist, as upstream makes them:
//! * [`call_caps`] -- `patternVal.func(cap1, .., cap5)` inside `parseMod` (ModParser.lua:6644): a factory returns the
//!   visitor;
//! * [`visit`] -- `func(node, out, data)` from the radius-jewel code of CalcSetup/PassiveSpec (R5.5).
//!
//! Every body carries its upstream lines. The getSimpleConv visitors at :19 referenced from jewelOtherFuncs and the two
//! thresholds inside :6582 get their upvalues from `data-patches/mod-parser-upvalues.json` (R5.5b0: the converter wrote
//! only one level of upvalues); a body whose upvalues are still missing cannot run: `ParseError::Unrepresentable`, never
//! a guess.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_data::mod_parser::{FnRef, PVal};
use pob_mods::{ClosureRef, LuaError, Mod, ModStore, ModType, StoreKind, Str, Table, Value};

use crate::ParseError;
use crate::helpers::{
    Arith, JewelNode, SimpleConv, Threshold, arith, create_mod_lua, first_to_upper, get_simple_conv, get_threshold, global_flag,
    gsub_upper, lua_string, tbl, tonumber,
};

const SRC: &str = "Modules/ModParser.lua";

/// The upvalues of a jewel function value.
pub enum JewelFn {
    /// A closure of the data (`jewelFuncList[k].func`, or an upvalue of one).
    Data(Rc<FnRef>),
    /// The visitor of getSimpleConv (:19) built by a factory at run time.
    SimpleConv(SimpleConv),
    /// The visitor of getThreshold (:6457).
    Threshold(Rc<Threshold>),
    /// The visitor a jewelOtherFuncs factory returns (`return function(node, out, data)` at `factory + 1`), with the
    /// factory's arguments (the captures, strings).
    Inner { factory: u32, args: Vec<Value> },
    /// :6564-6569 `return function(node, out, other)` of the jewelOtherFuncs wrapper, holding `innerFuncOrNil`.
    OtherGuard(Value),
    /// :2622-2627 the `ExtraJewelFunc` visitor of "non-unique jewels cause increases and reductions ...".
    Special2621 { dmg_type: Value },
    /// :2630-2634 the `ExtraJewelFunc` visitor of "non-unique jewels cause small and notable passive skills ...".
    Special2629 { val: Value, attr: Value },
}

/// A function value for `up` defined at `line`.
pub fn func_value(line: u32, up: JewelFn) -> Value {
    Value::Func(Rc::new(ClosureRef {
        id: Str::from(format!("{SRC}:{line}").as_str()),
        up: Rc::new(up),
    }))
}

fn unrep(s: impl Into<String>) -> ParseError {
    ParseError::Unrepresentable(s.into())
}

fn lua_err(s: impl Into<String>) -> ParseError {
    ParseError::Lua(LuaError(s.into()))
}

/// The `JewelFn` of a function value (a non-function is the Lua call error).
fn jewel_fn<'a>(f: &'a Value, what: &str) -> Result<(&'a ClosureRef, &'a JewelFn), ParseError> {
    match f {
        Value::Func(c) => match (*c.up).downcast_ref::<JewelFn>() {
            Some(j) => Ok((c, j)),
            None => Err(unrep(format!("{}: not a jewel function", c.id))),
        },
        other => Err(lua_err(format!("attempt to call {what} (a {} value)", other.type_name()))),
    }
}

thread_local! {
    /// One function value per closure of the data: Lua builds each `jewelFuncList` closure once per process, so its
    /// identity -- and the state a getThreshold visitor keeps in its `value` upvalue -- is shared by every use.
    static DATA_FUNCS: RefCell<HashMap<usize, Value>> = RefCell::new(HashMap::new());
}

/// The function value of a closure of the data. An unregistered definition is `Unported`.
pub fn data_func(f: &Rc<FnRef>) -> Result<Value, ParseError> {
    crate::closures::lookup(f)?;
    let key = Rc::as_ptr(f) as usize;
    if let Some(v) = DATA_FUNCS.with(|m| m.borrow().get(&key).cloned()) {
        return Ok(v);
    }
    let line = def_line(f);
    let up = if line == 6457
        && let Ok(t) = Threshold::from_up(f)
    {
        JewelFn::Threshold(Rc::new(t))
    } else if line == 19
        && let Ok(sc) = simple_conv_from_up(f)
    {
        JewelFn::SimpleConv(sc)
    } else {
        JewelFn::Data(f.clone())
    };
    let v = Value::Func(Rc::new(ClosureRef {
        id: f.id.clone(),
        up: Rc::new(up),
    }));
    DATA_FUNCS.with(|m| m.borrow_mut().insert(key, v.clone()));
    Ok(v)
}

/// The visitor at :19 from the upvalues the data patch (`data-patches/mod-parser-upvalues.json`, R5.5b0) carries:
/// `srcList, dst, type, remove` and, when not nil, `factor, srcType`.
fn simple_conv_from_up(f: &FnRef) -> Result<SimpleConv, ParseError> {
    let s = |k: &str| -> Result<Str, ParseError> {
        match crate::helpers::upv(f, k)? {
            Value::Str(s) => Ok(s),
            other => Err(unrep(format!("{}: upvalue '{k}' is a {}", f.id, other.type_name()))),
        }
    };
    let Value::Table(list) = crate::helpers::upv(f, "srcList")? else {
        return Err(unrep(format!("{}: upvalue 'srcList' not in the data", f.id)));
    };
    let src_list = list
        .array
        .iter()
        .map(|v| match v {
            Value::Str(s) => Ok(s.clone()),
            other => Err(unrep(format!("{}: srcList holds a {}", f.id, other.type_name()))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let src_type = match crate::helpers::upv(f, "srcType")? {
        Value::Nil => None,
        Value::Str(s) => Some(s),
        other => return Err(unrep(format!("{}: upvalue 'srcType' is a {}", f.id, other.type_name()))),
    };
    let factor = match crate::helpers::upv(f, "factor")? {
        Value::Nil => None,
        Value::Num(n) => Some(n),
        other => return Err(unrep(format!("{}: upvalue 'factor' is a {}", f.id, other.type_name()))),
    };
    Ok(SimpleConv {
        src_list,
        dst: s("dst")?,
        ty: s("type")?,
        remove: crate::helpers::upv(f, "remove")?.truthy(),
        factor,
        src_type,
    })
}

/// A `PVal` as a Lua value: closures become function values (`jewelFuncList` entries `{ func = f, type = ... }`).
pub fn pval_value(v: &PVal) -> Result<Value, ParseError> {
    match v {
        PVal::Lua(x) => Ok(x.clone()),
        PVal::Fn(f) => data_func(f),
        PVal::Table { array, hash } => {
            let mut t = Table::default();
            for x in array {
                t.array.push(pval_value(x)?);
            }
            for (k, x) in hash {
                t.hash.insert(k.clone(), pval_value(x)?);
            }
            Ok(Value::table(t))
        }
        PVal::CFn | PVal::Named(_) => Err(unrep("a C function or named table as a jewel value")),
    }
}

/// The `from` line of a definition id `"<src>:<from>"`.
/// The definition line of `f` in the R5 numbering (ModParser.lua @ ed354c2f8), which the `match` arms of this module use.
/// Upstream updates move these lines (R-UP, 3dbca4463: +15); the definition is recognised by the sha256 of its Lua body (`fns[id].sha`,
/// [`CANON`]). A definition whose body is not in [`CANON`] gets a number no arm matches (`Unrepresentable`, never a wrong visitor).
fn def_line(f: &FnRef) -> u32 {
    let data = pob_data::mod_parser::mod_parser();
    let d = data.fns.get(f.id.as_ref());
    match d.and_then(|d| CANON.iter().find(|(_, sha)| *sha == d.sha)) {
        Some((line, _)) => *line,
        None => u32::MAX,
    }
}

/// `(line @ ed354c2f8, sha256 of the body)` of every definition this module dispatches on (`tests/jewel.rs` checks each is in `fns`).
pub const CANON: [(u32, &str); 48] = [
    (19, "7eb893c0fe0803de8d1fbd4a498e6edafb54dc8f1a7181e1778181d8debcf645"),
    (6160, "ab5073ca87a3e11e5c67e190c3dbb4b961854462ce95b209b16748d8103b43c9"),
    (6163, "095a1f49474ff35d72e9450c14192b40c6eb2a6c969b7ee6ff143063dc0cbc60"),
    (6169, "2eb301edfb97a4fe9da4c7e39986faa227480f8c01e82809ecfde53daf8c2840"),
    (6172, "8e77e608ae148aa8115423a711ed6f5f2285eb8a4f2bed10ad9baaffb94602f0"),
    (6175, "dc120e11b240d47d8908523d809b621c596d8bc45627130a7f983443b8be23e0"),
    (6178, "688fe8c52f067a84da10cacbcf201fa896e7a6e3b85aad747b5492c0ffba7cad"),
    (6181, "844b2835eca1b9dadbc20e212ca44cc7af1d2721fc16b5c4e35ac60a6c52bd49"),
    (6184, "50b74a93a661b6c180a629b269b4902d38ef49930a16c96bb0d224f2fd8e6e1b"),
    (6187, "07b66feb631a068c9c5feb623b9d376aea2b06e86ba6156fa0bf3009a53c7422"),
    (6190, "41e3c7526435ba11715b5beb10a368ec06f666aaac2dcc8117a19d0e3b1e8eb9"),
    (6193, "0d066c27e4c8e273b9143ddaa5739c4497505dd7c561275cb5d4ece2c84f00d7"),
    (6196, "33d355e203c18a5d96a65a61f25b28260e1ca60a2407822935ca8e4607cf310f"),
    (6199, "2ac5b5881d09bdd4e096837ffea798558b458e3906401ca69bb2adcf3ce97299"),
    (6202, "e6490d61a4aeb8e50781ba0b21cee6e62013aa873cf461244baae7d0afd144b2"),
    (6231, "4f022f3004abd639877423f2b966a58e86483b8709eb62fb1e1f0a5176545d9b"),
    (6238, "a4ae13e724c783a8d1fcdf380e62d37b5fdb0b76b5552643824784c2a48e5231"),
    (6243, "97095cc678375eb49a94efdf8df9b5816b002f68d89f0d3dd582936a92ecb2c8"),
    (6250, "d4a278d9ccbc4cdde9e2fe5f49ae110f0c7814f762ca40e219654e57fb00d5d8"),
    (6255, "33c2c825675c1df407552a477503fef7ab75cc372643797368a69660b28dff94"),
    (6260, "e2fbb51802b364689da528cf2ddcab154a56d5db06a6ccf180233343b312dfff"),
    (6268, "10abc653b907e295b596fad33cf3e19569b1780bd6ab7be4a0712bfa2d69db4f"),
    (6275, "9317f37034d223f08bd0172a9c7659d7c2c0834121fdb6ba3eed1cf38ea9e3fd"),
    (6282, "2d05558f538309c42df9077e78d9e6af8ade44aaeda541833734d6b6a70e47cd"),
    (6289, "eedd34abcad32bb1156de166ae5fb2f47abe7eda5db7eea1ff5c044da3c5cb1d"),
    (6296, "629cf7f223eccb892f6bd9345bd3ecfeeb01502b8e57a91df0d5cf70a0c93c5e"),
    (6303, "a4a4804db6e2a183f7b667b681f10cf442a465ad921e4f41d78686597eb06874"),
    (6310, "a355efea6414fb760f5c0feb91b2e534e4ad65dd443a4aedcc892722d3d5129f"),
    (6317, "f746ec270b045f31d966fc97df7b3433988030527e929279add4a6e93aa11543"),
    (6327, "43cf36625e93dcee74e67da43a90955f4520681dc2b706209d012d0ee31143c2"),
    (6334, "5521b463906aefb4455d8cde8d7d08209a13555640e7f60cb62ee9941f70d1ab"),
    (6341, "e20234d005ec7dcbb00a9d28bae35347af82b6bfe11b32902bc2956d26e6d714"),
    (6350, "48a92e63187ec9128e26a34da3c520988c88d334e680c766703786159ea23cfa"),
    (6358, "e31114d5ae84d6a1ff9ad3f99f2df6072d78e3e23dd8e1e3423bbdab5e2a1bef"),
    (6370, "0829725a980694b83b774b2edcaee8437195c934abfa6afeb2b5d0f1b1c5133b"),
    (6388, "e23706e3b1f19fb3108973cb8309cd67b1ab52cd519522ef983487fe26d16490"),
    (6420, "ac689a0094151f11b1113d1ee073c4d713b1806be7165e3fa708d53d0114190d"),
    (6436, "be61cfea740f5ff018eb3bce4f5cabfe9c1cc5bb89c99799965c5eeb72f12f64"),
    (6457, "9e666b35d09cddfe4a399416fc23eea81ae4e104cbca716a1a6aed77037459cd"),
    (6513, "282f504dc43c0c9c8d77076c1fcfaf2aa56411a9cade753420374975001c26c9"),
    (6514, "efe8a05ef33c31b7f6f180ad1fd23618fb46f761e1c6490b64798e22dfb09782"),
    (6515, "8fd56e6165e85ccc93c0362d724b6aecaaaf58feab7955f91b5f96e2fc894e18"),
    (6537, "aaa4c0a4e7d16630c171982cd40f5a47d67a2dbd4d53948bdaf1f386560832d1"),
    (6541, "74c40d8aad030b97e3145d776600aec8653398209c8b35c353c6f10a54f82bf0"),
    (6542, "ef959bfdfd38d9593283bc789006846b26f07c9dd65fd6b0e97fb23c7e7d5136"),
    (6543, "a1bba5df22518ca908f143e04ab901a5b51f85c5a39c76d8543399c923fff94b"),
    (6555, "1b5c08e478099a7ec08d409955314854c1d3f00cea7415a1edefcaac2ec27a66"),
    (6582, "259525b2f0e95b89c3a0bd43ae3f676a03f2a18610336f96686964d941f355eb"),
];

fn up_fn(f: &FnRef, name: &str) -> Result<Value, ParseError> {
    match f.up.get(name) {
        Some(p) => pval_value(p),
        None => Err(unrep(format!("{}: upvalue '{name}' not in the data", f.id))),
    }
}

// --- the call inside parseMod: `patternVal.func(cap1, .., cap5)` ---------------------------------------------------

/// `f(cap1, .., cap5)` with the captures of a `jewelFuncList` pattern (strings): the one value kept.
pub fn call_caps(f: &Value, caps: &[Value]) -> Result<Value, ParseError> {
    let (c, j) = jewel_fn(f, "field 'func'")?;
    let JewelFn::Data(d) = j else {
        return Err(unrep(format!("{}: a run-time visitor called with captures", c.id)));
    };
    let a = |i: usize| caps.get(i).cloned().unwrap_or_default();
    let line = def_line(d);
    match line {
        // :6555-6570 the jewelOtherFuncs wrapper
        6555 => {
            if let Some(Value::Table(t)) = caps.first()
                && t.get("conqueredBy").truthy()
            {
                return Ok(Value::Nil);
            }
            let v = up_fn(d, "v")?;
            let inner = call_caps(&v, caps)?;
            Ok(func_value(6564, JewelFn::OtherGuard(inner)))
        }
        // :6160-6201 getSimpleConv factories, `function(num) return getSimpleConv(..., num / 100[, "BASE"]) end`
        6160 | 6163 | 6169 | 6172 | 6175 | 6178 | 6181 | 6184 | 6187 | 6190 | 6193 | 6196 | 6199 => {
            let factor = arith(Arith::Div, &a(0), &Value::Num(100.0), "local 'num'", "")?;
            let Value::Num(factor) = factor else {
                unreachable!("arith yields a number")
            };
            const L: [&str; 2] = ["LightningResist", "ElementalResist"];
            const C: [&str; 2] = ["ColdResist", "ElementalResist"];
            const F: [&str; 2] = ["FireResist", "ElementalResist"];
            let (src, dst, ty, remove, src_type): (&[&str], &str, &str, bool, Option<&str>) = match line {
                6160 => (&["EnergyShield"], "Armour", "INC", true, None),
                6163 => (&["Life"], "Mana", "INC", true, None),
                6169 => (&L, "SpellBlockChance", "BASE", false, None),
                6172 => (&L, "EnergyShield", "INC", false, Some("BASE")),
                6175 => (&L, "LightningDamageConvertToChaos", "BASE", false, None),
                6178 => (&C, "AttackDodgeChance", "BASE", false, None),
                6181 => (&C, "SpellSuppressionChance", "BASE", false, None),
                6184 => (&C, "Mana", "INC", false, Some("BASE")),
                6187 => (&C, "ColdDamageConvertToChaos", "BASE", false, None),
                6190 | 6193 => (&F, "BlockChance", "BASE", false, None),
                6196 => (&F, "Life", "INC", false, Some("BASE")),
                _ => (&F, "FireDamageConvertToChaos", "BASE", false, None),
            };
            Ok(func_value(
                19,
                JewelFn::SimpleConv(get_simple_conv(src, dst, ty, remove, Some(factor), src_type)),
            ))
        }
        // :6231-6365 factories returning `function(node, out, data)` (one line below the factory)
        6231 | 6243 | 6260 | 6268 | 6275 | 6282 | 6289 | 6296 | 6303 | 6310 | 6317 | 6327 | 6334 | 6341 | 6350 | 6358 => Ok(func_value(
            line + 1,
            JewelFn::Inner {
                factory: line,
                args: caps.to_vec(),
            },
        )),
        // :6513-6543 `function(num) return getThreshold(...) end`
        6513 | 6514 | 6515 | 6537 | 6541 | 6542 | 6543 => {
            let num = a(0);
            let s = |k: &str, v: &str| (k.to_owned(), Value::str(v));
            let tag = |fields: Vec<(String, Value)>| {
                let mut t = Table::default();
                for (k, v) in fields {
                    t.hash.insert(Str::from(k.as_str()), v);
                }
                Value::table(t)
            };
            let dual = || {
                tag(vec![
                    s("type", "SkillName"),
                    s("skillName", "Dual Strike"),
                    ("includeTransfigured".into(), Value::Bool(true)),
                ])
            };
            let cond = |var: &str| tag(vec![s("type", "Condition"), s("var", var)]);
            let ramp = || {
                let pair = |a: f64, b: f64| {
                    Value::table(Table {
                        array: vec![Value::Num(a), Value::Num(b)],
                        ..Table::default()
                    })
                };
                let r = Value::table(Table {
                    array: vec![pair(0.0, 0.0), pair(50.0, 1.0)],
                    ..Table::default()
                });
                tag(vec![s("type", "DistanceRamp"), ("ramp".into(), r)])
            };
            let t = match line {
                6513 => get_threshold(Value::str("Dex"), "Speed", "INC", num, &[dual(), cond("UsingClaw")])?,
                6514 => get_threshold(Value::str("Dex"), "CritMultiplier", "BASE", num, &[dual(), cond("UsingDagger")])?,
                6515 => get_threshold(Value::str("Dex"), "Accuracy", "INC", num, &[dual(), cond("UsingSword")])?,
                6537 => get_threshold(
                    Value::str("Int"),
                    "EnemyScorchChance",
                    "BASE",
                    num,
                    &[tag(vec![s("type", "SkillName"), s("skillName", "Fireball")])],
                )?,
                6541 => get_threshold(Value::str("Int"), "ManaRecoup", "BASE", num, &[cond("UsedWarcryRecently")])?,
                6542 => get_threshold(Value::str("Int"), "AreaOfEffect", "BASE", num, &[ramp()])?,
                _ => {
                    let v = arith(Arith::Mul, &num, &Value::Num(10.0), "local 'num'", "")?;
                    get_threshold(Value::str("Int"), "AreaOfEffect", "BASE", v, &[ramp()])?
                }
            };
            Ok(func_value(6457, JewelFn::Threshold(Rc::new(t))))
        }
        _ => Err(unrep(format!("{}: a visitor called with captures", d.id))),
    }
}

// --- the visit: `func(node, out, data)` (R5.5's caller) ------------------------------------------------------------

/// `out:NewMod(name, type, value, ...)`.
fn new_mod(out: &mut ModStore, name: &Value, ty: &str, value: Value, va: &[Value]) -> Result<(), ParseError> {
    out.new_mod(create_mod_lua(name, &Value::str(ty), value, va)?);
    Ok(())
}

fn flags(t: crate::helpers::GlobalFlags, names: &[&str]) -> Value {
    Value::Num(
        names
            .iter()
            .fold(0.0, |a, n| pob_lua::number::bor(a, global_flag(t, n).as_num().unwrap_or(0.0))),
    )
}

/// `data[k] or 0` as a number (arithmetic on a non-number is LuaJIT's error).
fn data_num(data: &Table, k: &str) -> Result<f64, ParseError> {
    match data.get(k) {
        Value::Nil | Value::Bool(false) => Ok(0.0),
        v => match arith(Arith::Add, v, &Value::Num(0.0), "", "")? {
            Value::Num(n) => Ok(n),
            _ => unreachable!(),
        },
    }
}

/// `f(node, out, data)` -- `node` is nil on the final call of a jewel pass.
pub fn visit(f: &Value, node: Option<&dyn JewelNode>, out: &mut ModStore, data: &mut Table) -> Result<(), ParseError> {
    let (c, j) = jewel_fn(f, "local 'func'")?;
    match j {
        JewelFn::SimpleConv(sc) => sc.visit(node, out),
        JewelFn::Threshold(t) => t.visit(node.is_some(), out, data),
        JewelFn::Inner { factory, args } => visit_inner(*factory, args, node, out, data),
        // :6564-6569
        JewelFn::OtherGuard(inner) => {
            if node.is_some_and(|n| n.conquered_by()) {
                return Ok(());
            }
            if !inner.truthy() {
                return Err(lua_err("attempt to call upvalue 'innerFuncOrNil' (a nil value)"));
            }
            visit(inner, node, out, data)
        }
        // :2622-2627
        JewelFn::Special2621 { dmg_type } => match node {
            Some(n) if !n.conquered_by() => {
                let dst = format!("{}Damage", gsub_upper(&lua_string(dmg_type, "upvalue 'dmgType'")?, "^%l")?);
                let src = [
                    "PhysicalDamage",
                    "FireDamage",
                    "ColdDamage",
                    "LightningDamage",
                    "ChaosDamage",
                    "ElementalDamage",
                ];
                get_simple_conv(&src, &dst, "INC", true, None, None).visit(node, out)
            }
            _ => Ok(()),
        },
        // :2630-2634
        JewelFn::Special2629 { val, attr } => match node {
            Some(n) if !n.conquered_by() && matches!(n.node_type(), "Notable" | "Normal") => {
                let up = first_to_upper(&lua_string(attr, "upvalue 'attr'")?);
                let name = pob_lua::match_(up.as_bytes(), b"^%a%l%l", None)
                    .map_err(|e| lua_err(e.0))?
                    .and_then(|c| {
                        c.get(0)
                            .and_then(|x| x.bytes(up.as_bytes()))
                            .map(|b| Value::str(&String::from_utf8_lossy(b)))
                    })
                    .unwrap_or_default();
                new_mod(out, &name, "BASE", tonumber(val), &[data.get("modSource").clone()])
            }
            _ => Ok(()),
        },
        JewelFn::Data(d) => visit_data(c, d, node, out, data),
    }
}

/// The visitor bodies of the data.
fn visit_data(c: &ClosureRef, d: &Rc<FnRef>, node: Option<&dyn JewelNode>, out: &mut ModStore, data: &mut Table) -> Result<(), ParseError> {
    let src = data.get("modSource").clone();
    let ty_is = |t: &str| node.is_some_and(|n| n.node_type() == t);
    match def_line(d) {
        // :6555-6570 the wrapper called as a visitor (an exact-key entry): `cap1` is the node
        6555 => {
            if node.is_some_and(|n| n.conquered_by()) {
                return Ok(());
            }
            let v = up_fn(d, "v")?;
            // `local innerFuncOrNil = v(cap1, cap2, cap3)` -- the visitor runs; the returned closure is dropped
            visit(&v, node, out, data)
        }
        // :6202-6229 Melee and Melee Weapon Type modifiers in Radius are Transformed to Bow Modifiers
        6202 => {
            let Some(node) = node else { return Ok(()) };
            use crate::helpers::GlobalFlags::ModFlag as MF;
            let num = |v: Value| v.as_num().unwrap_or(0.0);
            let mask1 = num(flags(MF, &["Axe", "Claw", "Dagger", "Mace", "Staff", "Sword", "Melee"]));
            let mask2 = num(flags(MF, &["Weapon1H", "WeaponMelee"]));
            let mask3 = num(flags(MF, &["Weapon2H", "WeaponMelee"]));
            let bow = num(global_flag(MF, "Bow"));
            use pob_lua::number::{band, bnot, bor};
            let using = [
                "UsingAxe",
                "UsingClaw",
                "UsingDagger",
                "UsingMace",
                "UsingStaff",
                "UsingSword",
                "UsingMeleeWeapon",
            ];
            let is_using = |t: &Table| t.get("type").is_str("Condition") && t.get("var").as_str().is_some_and(|v| using.contains(&v));
            for m in node.mod_list() {
                let f = f64::from(m.flags);
                let head = |flags: f64| {
                    vec![
                        Value::from(m.source.as_ref()),
                        Value::Num(flags),
                        Value::Num(f64::from(m.keyword_flags)),
                    ]
                };
                let tags: Vec<Value> = m.tags.iter().map(|t| Value::Table(t.table())).collect();
                let merge = |out: &mut ModStore, value: Value, va: Vec<Value>| -> Result<(), ParseError> {
                    let ty = m.ty.map_or(Value::Nil, |t| Value::str(t.as_str()));
                    let nm = create_mod_lua(&Value::Str(m.name.clone()), &ty, value, &va)?;
                    out.merge_mod(Rc::new(nm), false).map_err(ParseError::Lua)
                };
                let negv = crate::helpers::neg_v(&m.value, "field 'value'");
                if band(f, mask1) != 0.0 || band(f, mask2) == mask2 || band(f, mask3) == mask3 {
                    merge(out, negv?, [head(f), tags.clone()].concat())?;
                    let nf = bor(band(f, bnot(bor(bor(mask1, mask2), mask3))), bow);
                    merge(out, m.value.clone(), [head(nf), tags].concat())?;
                } else if !m.tags.is_empty() {
                    for t in &m.tags {
                        if is_using(&t.table()) {
                            // `copyTable(mod)` then the first matching tag of the copy gets `var = "UsingBow"`
                            let mut new_tags: Vec<Value> = m.tags.iter().map(|t| Value::table((*t.table()).clone())).collect();
                            if let Some(Value::Table(nt)) = new_tags.iter_mut().find(|x| matches!(x, Value::Table(t) if is_using(t))) {
                                Rc::make_mut(nt).hash.insert(Str::from("var"), Value::str("UsingBow"));
                            }
                            merge(out, negv?, [head(f), tags.clone()].concat())?;
                            merge(out, m.value.clone(), [head(f), new_tags].concat())?;
                            break;
                        }
                    }
                }
            }
            Ok(())
        }
        // :6238-6242 / :6250-6254 / :6255-6259
        6238 => {
            if ty_is("Notable") {
                new_mod(out, &Value::str("PassiveSkillHasNoEffect"), "FLAG", Value::Bool(true), &[src])?;
            }
            Ok(())
        }
        6250 | 6255 => {
            if ty_is(if def_line(d) == 6250 { "Normal" } else { "Notable" }) {
                new_mod(
                    out,
                    &Value::str("AllocatedPassiveSkillHasNoEffect"),
                    "FLAG",
                    Value::Bool(true),
                    &[src],
                )?;
            }
            Ok(())
        }
        // :6370-6376 getPerStat(dst, modType, flags, stat, factor)
        6370 => {
            let up = |k: &str| crate::helpers::upv(d, k);
            let stat = lua_string(&up("stat")?, "upvalue 'stat'")?;
            if node.is_some() {
                let s = out.sum(ModType::Base, None, &[&stat]).map_err(ParseError::Lua)?;
                let cur = data_num(data, &stat)?;
                data.hash.insert(Str::from(stat.as_str()), Value::Num(cur + s));
            } else if !data.get(&stat).raw_eq(&Value::Num(0.0)) {
                let v = arith(
                    Arith::Mul,
                    &Value::Num(data_num(data, &stat)?),
                    &up("factor")?,
                    "",
                    "upvalue 'factor'",
                )?;
                let v = crate::helpers::math_floor(&v)?;
                let ty = lua_string(&up("modType")?, "upvalue 'modType'")?;
                new_mod(out, &up("dst")?, &ty, v, &[src, up("flags")?])?;
            }
            Ok(())
        }
        // :6388-6395
        6388 => {
            if let Some(n) = node {
                for k in ["Dex", "Int"] {
                    let s = n.sum_base(k).map_err(ParseError::Lua)?;
                    let cur = data_num(data, k)?;
                    data.hash.insert(Str::from(k), Value::Num(cur + s));
                }
            } else if data.get("Dex").truthy() || data.get("Int").truthy() {
                let v = data_num(data, "Dex")? + data_num(data, "Int")?;
                new_mod(out, &Value::str("DexIntToMeleeBonus"), "BASE", Value::Num(v), &[src])?;
            }
            Ok(())
        }
        // :6420-6435 / :6436-6451 Grants all bonuses of Unallocated Small / Notable Passive Skills in Radius
        6420 | 6436 => {
            let want = if def_line(d) == 6420 { "Normal" } else { "Notable" };
            if node.is_some() {
                if ty_is(want) {
                    // `data.modList = data.modList or new("ModList")` -- a list of the same mod objects
                    let mut list = match data.get("modList") {
                        Value::Table(t) => (**t).clone(),
                        Value::Nil | Value::Bool(false) => Table::default(),
                        other => return Err(unrep(format!("data.modList is a {}", other.type_name()))),
                    };
                    let mods: &[Rc<Mod>] = match &out.kind {
                        StoreKind::List(l) => &l.mods,
                        StoreKind::Db(_) => &[],
                    };
                    for m in mods {
                        if !m.name.starts_with("Condition:ConnectedTo") {
                            list.array.push(Value::Mod(m.clone()));
                        }
                    }
                    data.hash.insert(Str::from("modList"), Value::table(list));
                }
            } else if let Value::Table(t) = data.get("modList").clone() {
                for m in &t.array {
                    if let Value::Mod(m) = m {
                        out.add_mod(m.clone());
                    }
                }
            }
            Ok(())
        }
        // :6582-6589 several thresholds on one line
        6582 => {
            let v = up_fn(d, "v")?;
            let funcs = match &v {
                Value::Table(t) => t.array.clone(),
                other => {
                    return Err(lua_err(format!(
                        "bad argument #1 to 'ipairs' (table expected, got {})",
                        other.type_name()
                    )));
                }
            };
            let mut fd = match data.get("funcData") {
                Value::Table(t) => (**t).clone(),
                _ => Table::default(),
            };
            for (i, func) in funcs.iter().enumerate() {
                let mut sub = match fd.array.get(i) {
                    Some(Value::Table(t)) => (**t).clone(),
                    _ => Table::default(),
                };
                if src.is_nil() {
                    sub.hash.shift_remove("modSource");
                } else {
                    sub.hash.insert(Str::from("modSource"), src.clone());
                }
                visit(func, node, out, &mut sub)?;
                fd.set(&Value::Num((i + 1) as f64), Value::table(sub));
            }
            data.hash.insert(Str::from("funcData"), Value::table(fd));
            Ok(())
        }
        19 => Err(unrep(format!("{}: the getSimpleConv upvalues are not in the data", c.id))),
        6457 => Err(unrep(format!("{}: the getThreshold upvalues are not in the data", c.id))),
        _ => Err(unrep(format!("{}: a factory called as a visitor", c.id))),
    }
}

/// The visitors the jewelOtherFuncs factories return (:6231-6365).
fn visit_inner(factory: u32, a: &[Value], node: Option<&dyn JewelNode>, out: &mut ModStore, data: &mut Table) -> Result<(), ParseError> {
    use crate::helpers::GlobalFlags::{KeywordFlag, ModFlag};
    let Some(n) = node else { return Ok(()) };
    let ty = n.node_type();
    let not_ks = ty != "Keystone" && ty != "ClassStart";
    let not_kss = not_ks && ty != "Socket";
    let src = data.get("modSource").clone();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    let num = || tonumber(&arg(0));
    let s = Value::str;
    let node_mod = |out: &mut ModStore, inner: Mod| -> Result<(), ParseError> {
        new_mod(
            out,
            &s("PassiveSkillHasOtherEffect"),
            "FLAG",
            Value::Bool(true),
            std::slice::from_ref(&src),
        )?;
        new_mod(
            out,
            &s("NodeModifier"),
            "LIST",
            tbl([("mod", Value::Mod(Rc::new(inner)))]),
            std::slice::from_ref(&src),
        )
    };
    let minion = |inner: Mod| -> Result<Mod, ParseError> {
        create_mod_lua(&s("MinionModifier"), &s("LIST"), tbl([("mod", Value::Mod(Rc::new(inner)))]), &[])
    };
    match factory {
        6231 if not_ks => new_mod(out, &s("PassiveSkillEffect"), "INC", num(), &[src]),
        6243 if n.is_tattoo() => new_mod(out, &s("PassiveSkillEffect"), "INC", num(), &[src]),
        6260 if not_ks => {
            let kw = flags(KeywordFlag, &["Trap", "Mine"]);
            new_mod(out, &s("PhysicalMin"), "BASE", num(), &[src.clone(), Value::Num(0.0), kw.clone()])?;
            new_mod(out, &s("PhysicalMax"), "BASE", tonumber(&arg(1)), &[src, Value::Num(0.0), kw])
        }
        6268 if not_ks => new_mod(
            out,
            &s("Speed"),
            "INC",
            num(),
            &[src, flags(ModFlag, &["Unarmed", "Attack", "Melee"])],
        ),
        6275 if not_kss => new_mod(out, &s("CritChance"), "INC", num(), &[src]),
        6282 if not_kss => new_mod(out, &s("Life"), "BASE", num(), &[src]),
        6289 if not_kss => new_mod(out, &s("Mana"), "BASE", num(), &[src]),
        6296 if not_kss => new_mod(out, &s("EnergyShield"), "INC", num(), &[src]),
        6303 if not_kss => new_mod(out, &s("Armour"), "INC", num(), &[src]),
        6310 if not_kss => new_mod(out, &s("Evasion"), "INC", num(), &[src]),
        6317 if not_kss => {
            for k in ["Str", "Dex", "Int", "All"] {
                new_mod(out, &s(k), "BASE", num(), std::slice::from_ref(&src))?;
            }
            Ok(())
        }
        6327 if not_kss => new_mod(out, &s("ChaosResist"), "BASE", num(), &[src]),
        6334 if not_kss => {
            let name = format!("{}Damage", first_to_upper(&lua_string(&arg(1), "upvalue 'type'")?));
            new_mod(out, &s(&name), "INC", num(), &[src])
        }
        6341 if ty == "Notable" => {
            let mana = create_mod_lua(&s("ManaCost"), &s("INC"), num(), std::slice::from_ref(&src))?;
            new_mod(
                out,
                &s("PassiveSkillHasOtherEffect"),
                "FLAG",
                Value::Bool(true),
                std::slice::from_ref(&src),
            )?;
            new_mod(
                out,
                &s("NodeModifier"),
                "LIST",
                tbl([("mod", Value::Mod(Rc::new(mana)))]),
                std::slice::from_ref(&src),
            )?;
            let dmg = create_mod_lua(
                &s("Damage"),
                &s("INC"),
                tonumber(&arg(1)),
                &[src.clone(), global_flag(ModFlag, "Spell")],
            )?;
            new_mod(out, &s("NodeModifier"), "LIST", tbl([("mod", Value::Mod(Rc::new(dmg)))]), &[src])
        }
        6350 if ty == "Notable" => {
            let inner = create_mod_lua(&s("DamageTaken"), &s("INC"), num(), std::slice::from_ref(&src))?;
            node_mod(out, minion(inner)?)
        }
        6358 if ty == "Notable" => {
            let v = crate::helpers::neg_v(&num(), "")?;
            let inner = create_mod_lua(&s("MovementSpeed"), &s("INC"), v, std::slice::from_ref(&src))?;
            node_mod(out, minion(inner)?)
        }
        _ => Ok(()),
    }
}
