//! The 57 definitions the generator does not take (R5.3a list `KNOWLEDGE/drafts/R5-manual-closures.md`), hand-ported
//! (R5.4): 11 `specialModList` closures (nested functions, loops, `string.format`, L9 orphan tags) and the 46
//! `jewelFuncList` definitions (factories and visitors -- their bodies are in `jewel.rs`, the call sites run them).
//! Every entry pins the sha256 of the Lua body it ports (drift test `tests/registry.rs`).

use pob_data::mod_parser::{FnRef, ModParserData};
use pob_data::wire::OrphanTag;
use pob_mods::{Table, Tag, Value};

use crate::ParseError;
use crate::closures::{ClosureFn, Registered, Ret2};
use crate::helpers::{
    arg, call_mod, check_str, create_mod_lua, first_to_upper_v, gsub_upper, lua_eq, named_gem_id_lookup, neg_v, self_str, seq, tbl,
};
use crate::jewel::{JewelFn, func_value};
use crate::parse::RawParse;

const fn r(id: &'static str, to: u32, sha: &'static str, f: ClosureFn) -> Registered {
    Registered { id, to, sha, f }
}

/// The hand-ported definitions, in `fns` order.
pub static MANUAL: [Registered; 57] = [
    r(
        "Modules/ModParser.lua:2621",
        2628,
        "8172968c35f37103b8c6f0b40b919a24a29f6f96613e101cce49f0ba63e30c9a",
        ClosureFn::Special(c2621),
    ),
    r(
        "Modules/ModParser.lua:2629",
        2635,
        "962b95f6212fa72fb61c90fbac44d708f94efac64fc6dc61a9f23a5d291852fd",
        ClosureFn::Special(c2629),
    ),
    r(
        "Modules/ModParser.lua:2898",
        2904,
        "fff722f3aca694847fb7396eda4ca60d18393a10f9fa623e64674d86e61928a4",
        ClosureFn::Special(c2898),
    ),
    r(
        "Modules/ModParser.lua:3272",
        3281,
        "f20f9ff3a88041c5619306157b0b7c84e58f2ebed282c9d32b62d511e3550e07",
        ClosureFn::Special(c3272),
    ),
    r(
        "Modules/ModParser.lua:4497",
        4504,
        "c8c1912d00e02a72e4aae1d007f69d13965415a972c06de75d042ce6dfa86eca",
        ClosureFn::Special(c4497),
    ),
    r(
        "Modules/ModParser.lua:4576",
        4576,
        "17bf3d926c9b1af4384358577f5f4b0505a92ddcd94619f40c30ad2bbdf4dcfc",
        ClosureFn::SpecialOrphan(c4576),
    ),
    r(
        "Modules/ModParser.lua:4577",
        4577,
        "756d09e2d19d61a985931d80b4754f4192fa3dc0788f5c214c121f8c59b525d9",
        ClosureFn::SpecialOrphan(c4577),
    ),
    r(
        "Modules/ModParser.lua:4578",
        4578,
        "98c9d082672e16afdc54cf498f235db76a093e41fde13fa51e9e344004ae841e",
        ClosureFn::SpecialOrphan(c4578),
    ),
    r(
        "Modules/ModParser.lua:5686",
        5688,
        "c0cd86ca11f268c9f7cf8af746df27f433935a42e9c588136ecf7664d3b4c3dd",
        ClosureFn::SpecialOrphan(c5686),
    ),
    r(
        "Modules/ModParser.lua:5689",
        5691,
        "154170983b9f305f8235b786cbd165cca6d126266ba6d7c74e0fd377661f8b36",
        ClosureFn::SpecialOrphan(c5689),
    ),
    r(
        "Modules/ModParser.lua:5692",
        5694,
        "7265be83cdae799205d841934be4f5e6360e6695ae9c5f6f998997dd70a46668",
        ClosureFn::SpecialOrphan(c5692),
    ),
    r(
        "Modules/ModParser.lua:6160",
        6162,
        "ab5073ca87a3e11e5c67e190c3dbb4b961854462ce95b209b16748d8103b43c9",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6163",
        6165,
        "095a1f49474ff35d72e9450c14192b40c6eb2a6c969b7ee6ff143063dc0cbc60",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6169",
        6171,
        "2eb301edfb97a4fe9da4c7e39986faa227480f8c01e82809ecfde53daf8c2840",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6172",
        6174,
        "8e77e608ae148aa8115423a711ed6f5f2285eb8a4f2bed10ad9baaffb94602f0",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6175",
        6177,
        "dc120e11b240d47d8908523d809b621c596d8bc45627130a7f983443b8be23e0",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6178",
        6180,
        "688fe8c52f067a84da10cacbcf201fa896e7a6e3b85aad747b5492c0ffba7cad",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6181",
        6183,
        "844b2835eca1b9dadbc20e212ca44cc7af1d2721fc16b5c4e35ac60a6c52bd49",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6184",
        6186,
        "50b74a93a661b6c180a629b269b4902d38ef49930a16c96bb0d224f2fd8e6e1b",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6187",
        6189,
        "07b66feb631a068c9c5feb623b9d376aea2b06e86ba6156fa0bf3009a53c7422",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6190",
        6192,
        "41e3c7526435ba11715b5beb10a368ec06f666aaac2dcc8117a19d0e3b1e8eb9",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6193",
        6195,
        "0d066c27e4c8e273b9143ddaa5739c4497505dd7c561275cb5d4ece2c84f00d7",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6196",
        6198,
        "33d355e203c18a5d96a65a61f25b28260e1ca60a2407822935ca8e4607cf310f",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6199",
        6201,
        "2ac5b5881d09bdd4e096837ffea798558b458e3906401ca69bb2adcf3ce97299",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6202",
        6230,
        "e6490d61a4aeb8e50781ba0b21cee6e62013aa873cf461244baae7d0afd144b2",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6231",
        6237,
        "4f022f3004abd639877423f2b966a58e86483b8709eb62fb1e1f0a5176545d9b",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6238",
        6242,
        "a4ae13e724c783a8d1fcdf380e62d37b5fdb0b76b5552643824784c2a48e5231",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6243",
        6249,
        "97095cc678375eb49a94efdf8df9b5816b002f68d89f0d3dd582936a92ecb2c8",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6250",
        6254,
        "d4a278d9ccbc4cdde9e2fe5f49ae110f0c7814f762ca40e219654e57fb00d5d8",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6255",
        6259,
        "33c2c825675c1df407552a477503fef7ab75cc372643797368a69660b28dff94",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6260",
        6267,
        "e2fbb51802b364689da528cf2ddcab154a56d5db06a6ccf180233343b312dfff",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6268",
        6274,
        "10abc653b907e295b596fad33cf3e19569b1780bd6ab7be4a0712bfa2d69db4f",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6275",
        6281,
        "9317f37034d223f08bd0172a9c7659d7c2c0834121fdb6ba3eed1cf38ea9e3fd",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6282",
        6288,
        "2d05558f538309c42df9077e78d9e6af8ade44aaeda541833734d6b6a70e47cd",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6289",
        6295,
        "eedd34abcad32bb1156de166ae5fb2f47abe7eda5db7eea1ff5c044da3c5cb1d",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6296",
        6302,
        "629cf7f223eccb892f6bd9345bd3ecfeeb01502b8e57a91df0d5cf70a0c93c5e",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6303",
        6309,
        "a4a4804db6e2a183f7b667b681f10cf442a465ad921e4f41d78686597eb06874",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6310",
        6316,
        "a355efea6414fb760f5c0feb91b2e534e4ad65dd443a4aedcc892722d3d5129f",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6317",
        6326,
        "f746ec270b045f31d966fc97df7b3433988030527e929279add4a6e93aa11543",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6327",
        6333,
        "43cf36625e93dcee74e67da43a90955f4520681dc2b706209d012d0ee31143c2",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6334",
        6340,
        "5521b463906aefb4455d8cde8d7d08209a13555640e7f60cb62ee9941f70d1ab",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6341",
        6349,
        "e20234d005ec7dcbb00a9d28bae35347af82b6bfe11b32902bc2956d26e6d714",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6350",
        6357,
        "48a92e63187ec9128e26a34da3c520988c88d334e680c766703786159ea23cfa",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6358",
        6365,
        "e31114d5ae84d6a1ff9ad3f99f2df6072d78e3e23dd8e1e3423bbdab5e2a1bef",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6370",
        6376,
        "0829725a980694b83b774b2edcaee8437195c934abfa6afeb2b5d0f1b1c5133b",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6388",
        6395,
        "e23706e3b1f19fb3108973cb8309cd67b1ab52cd519522ef983487fe26d16490",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6420",
        6435,
        "ac689a0094151f11b1113d1ee073c4d713b1806be7165e3fa708d53d0114190d",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6436",
        6451,
        "be61cfea740f5ff018eb3bce4f5cabfe9c1cc5bb89c99799965c5eeb72f12f64",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6513",
        6513,
        "282f504dc43c0c9c8d77076c1fcfaf2aa56411a9cade753420374975001c26c9",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6514",
        6514,
        "efe8a05ef33c31b7f6f180ad1fd23618fb46f761e1c6490b64798e22dfb09782",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6515",
        6515,
        "8fd56e6165e85ccc93c0362d724b6aecaaaf58feab7955f91b5f96e2fc894e18",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6537",
        6537,
        "aaa4c0a4e7d16630c171982cd40f5a47d67a2dbd4d53948bdaf1f386560832d1",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6541",
        6541,
        "74c40d8aad030b97e3145d776600aec8653398209c8b35c353c6f10a54f82bf0",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6542",
        6542,
        "ef959bfdfd38d9593283bc789006846b26f07c9dd65fd6b0e97fb23c7e7d5136",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6543",
        6543,
        "a1bba5df22518ca908f143e04ab901a5b51f85c5a39c76d8543399c923fff94b",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6555",
        6570,
        "1b5c08e478099a7ec08d409955314854c1d3f00cea7415a1edefcaac2ec27a66",
        ClosureFn::Jewel,
    ),
    r(
        "Modules/ModParser.lua:6582",
        6589,
        "259525b2f0e95b89c3a0bd43ae3f676a03f2a18610336f96686964d941f355eb",
        ClosureFn::Jewel,
    ),
];

fn item_condition(rarity: &str) -> Value {
    tbl([
        ("type", Value::str("ItemCondition")),
        ("itemSlot", Value::str("{SlotName}")),
        ("rarityCond", Value::str(rarity)),
        ("neg", Value::Bool(true)),
    ])
}

/// `{ mod("ExtraJewelFunc", "LIST", {radius = radius, type = "Other", func = func}, <UNIQUE>, <RELIC>) }`
fn extra_jewel_func(radius: Value, func: Value) -> Result<Ret2, ParseError> {
    let v = tbl([("radius", radius), ("type", Value::str("Other")), ("func", func)]);
    let m = call_mod(vec![
        Value::str("ExtraJewelFunc"),
        Value::str("LIST"),
        v,
        item_condition("UNIQUE"),
        item_condition("RELIC"),
    ])?;
    Ok((seq(vec![m]), Value::Nil))
}

/// `Modules/ModParser.lua:2621-2628` -- `function(_, radius, dmgType)`: `radius = firstToUpper(radius)`, the visitor
/// (:2622) captures `dmgType`.
fn c2621(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<Ret2, ParseError> {
    let radius = first_to_upper_v(&arg(a, 1))?;
    extra_jewel_func(radius, func_value(2622, JewelFn::Special2621 { dmg_type: arg(a, 2) }))
}

/// `Modules/ModParser.lua:2629-2635` -- `function(_, radius, val, attr)`: `radius = (radius:gsub("^%l", string.upper))`,
/// the visitor (:2630) captures `attr` and `val`.
fn c2629(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<Ret2, ParseError> {
    let radius = Value::str(&gsub_upper(&self_str(&arg(a, 1), "local 'radius'")?, "^%l")?);
    let func = func_value(
        2630,
        JewelFn::Special2629 {
            val: arg(a, 2),
            attr: arg(a, 3),
        },
    );
    extra_jewel_func(radius, func)
}

/// `Modules/ModParser.lua:2898-2904` -- one `Self<ailment>Effect` per `data.nonDamagingAilmentTypeList` entry.
fn c2898(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<Ret2, ParseError> {
    let num = arg(a, 0);
    let mut mods = Vec::new();
    for ailment in &pob_data::rules::rules().non_damaging_ailment_type_list {
        mods.push(call_mod(vec![
            Value::str(&format!("Self{ailment}Effect")),
            Value::str("INC"),
            neg_v(&num, "local 'num'")?,
            tbl([("type", Value::str("Condition")), ("var", Value::str("AffectedByArcaneSurge"))]),
        ])?);
    }
    Ok((seq(mods), Value::Nil))
}

/// `Modules/ModParser.lua:3272-3281` -- `function(num, _, property, skill)`: a known gem name keeps `key = "level"`,
/// otherwise the words of `skill` (`gmatch("%S+")`) become `keywordList`.
fn c3272(d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<Ret2, ParseError> {
    let (num, property, skill) = (arg(a, 0), arg(a, 2), arg(a, 3));
    if named_gem_id_lookup(d, &skill)?.truthy() {
        let v = tbl([
            ("keyword", skill),
            ("key", Value::str("level")),
            ("value", num),
            ("keyOfScaledMod", Value::str("value")),
        ]);
        return Ok((
            seq(vec![call_mod(vec![Value::str("GemProperty"), Value::str("LIST"), v])?]),
            Value::Nil,
        ));
    }
    let s = self_str(&skill, "local 'skill'")?;
    let mut words = Vec::new();
    for c in pob_lua::gmatch(s.as_bytes(), b"%S+") {
        let c = c.map_err(|e| ParseError::Lua(pob_mods::LuaError(e.0)))?;
        let w = c.get(0).and_then(|x| x.bytes(s.as_bytes())).unwrap_or_default();
        words.push(Value::str(&String::from_utf8_lossy(w)));
    }
    let v = tbl([
        ("keywordList", seq(words)),
        ("key", property),
        ("value", num),
        ("keyOfScaledMod", Value::str("value")),
    ]);
    Ok((
        seq(vec![call_mod(vec![Value::str("GemProperty"), Value::str("LIST"), v])?]),
        Value::Nil,
    ))
}

/// `Modules/ModParser.lua:4497-4504` -- `function(num, _, source, target, colour)`; no return for another colour.
fn c4497(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<Ret2, ParseError> {
    let (num, source, target, colour) = (arg(a, 0), arg(a, 2), arg(a, 3), arg(a, 4));
    if !["red", "green", "blue"].iter().any(|c| lua_eq(&colour, &Value::str(c))) {
        return Ok((Value::Nil, Value::Nil));
    }
    // `string.format("Socketed%sGemsIn{SlotName}", colour:gsub("^%l", string.upper))` -- the count is an extra argument
    let colour = gsub_upper(&self_str(&colour, "local 'colour'")?, "^%l")?;
    let var = format!("Socketed{}GemsIn{{SlotName}}", check_str(&Value::str(&colour), 2, "format")?);
    let name = format!(
        "{}DamageConvertTo{}",
        gsub_upper(&self_str(&source, "local 'source'")?, "^%l")?,
        gsub_upper(&self_str(&target, "local 'target'")?, "^%l")?
    );
    let inner = call_mod(vec![Value::str(&name), Value::str("BASE"), num])?;
    let m = call_mod(vec![
        Value::str("MinionModifier"),
        Value::str("LIST"),
        tbl([("mod", inner)]),
        tbl([("type", Value::str("Multiplier")), ("var", Value::str(&var))]),
    ])?;
    Ok((seq(vec![m]), Value::Nil))
}

// --- L9: `mod(name, type, value, nil, {tag})` -- the tag sits at [2] after a nil [1] (decision R5.4/3) ------------

/// `modLib.createMod(name, type, value, nil, tag)` (ModTools.lua:33-67): slot 1 is nil (no source) and slot 2 a table
/// (no flags), so `tagStart` = 1 and the array part is `{ nil, tag }` -- `ipairs` sees no tag; the tag is kept beside the
/// mod as an orphan at index 2, exactly as `mod_cache`/static entries keep it (R3-ARCH §2.1). Not applied.
fn orphan_parse(name: &str, ty: &str, value: Value, tag: Value) -> Result<RawParse, ParseError> {
    let m = create_mod_lua(&Value::str(name), &Value::str(ty), value, &[])?;
    let Value::Table(t) = tag else {
        return Err(ParseError::Unrepresentable("an L9 orphan that is not a table".into()));
    };
    Ok(RawParse {
        mod_list: Some(vec![m]),
        extra: None,
        orphan_tags: vec![(
            0,
            OrphanTag {
                index: 2,
                tag: Tag::from_rc(t),
            },
        )],
    })
}

fn skill_names(names: [&str; 2]) -> Value {
    let list = Value::table(Table {
        array: names.iter().map(|n| Value::str(n)).collect(),
        ..Table::default()
    });
    tbl([
        ("type", Value::str("SkillName")),
        ("skillNameList", list),
        ("includeTransfigured", Value::Bool(true)),
    ])
}

fn zeal() -> Value {
    tbl([("type", Value::str("Condition")), ("var", Value::str("SacrificialZeal"))])
}

/// `Modules/ModParser.lua:4576`
fn c4576(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<RawParse, ParseError> {
    orphan_parse(
        "ProjectileCount",
        "MORE",
        arg(a, 0),
        skill_names(["Caustic Arrow", "Scourge Arrow"]),
    )
}

/// `Modules/ModParser.lua:4577`
fn c4577(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<RawParse, ParseError> {
    orphan_parse("ProjectileCount", "BASE", arg(a, 0), skill_names(["Essence Drain", "Soulrend"]))
}

/// `Modules/ModParser.lua:4578`
fn c4578(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<RawParse, ParseError> {
    orphan_parse(
        "ProjectileSpeed",
        "INC",
        neg_v(&arg(a, 0), "local 'num'")?,
        skill_names(["Essence Drain", "Soulrend"]),
    )
}

/// `Modules/ModParser.lua:5686-5688`
fn c5686(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<RawParse, ParseError> {
    orphan_parse("EnemyPhysicalDamageReduction", "BASE", neg_v(&arg(a, 0), "local 'num'")?, zeal())
}

/// `Modules/ModParser.lua:5689-5691`
fn c5689(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<RawParse, ParseError> {
    orphan_parse("ChanceToIgnoreEnemyPhysicalDamageReduction", "BASE", arg(a, 0), zeal())
}

/// `Modules/ModParser.lua:5692-5694`
fn c5692(_d: &ModParserData, _f: &FnRef, a: &[Value]) -> Result<RawParse, ParseError> {
    orphan_parse("ChanceToIgnoreEnemyPhysicalDamageReduction", "BASE", arg(a, 0), zeal())
}
