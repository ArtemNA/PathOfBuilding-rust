//! The named helpers on lines whose closure is a direct helper call (ModCache answer = LuaJIT answer for these lines,
//! `cacheEq` in the R5.1 fixtures), until R5.3's closures call them on the whole corpus.

use pob_data::mod_parser::mod_parser;
use pob_mods::Value;
use pob_parser::helpers::{deal_no_non_damage_type, extra_support, first_to_upper, granted_extra_skill, mod_list, trigger_extra_skill};
use pob_parser::scan::value_eq;

fn cache_mods(line: &str) -> Value {
    let c = pob_data::mod_cache::mod_cache();
    mod_list(c.get(line).and_then(|e| e.mods.clone()).expect(line))
}

#[test]
fn helpers_match_the_cache() {
    let d = mod_parser();
    let s = Value::str;
    assert_eq!(first_to_upper("cold damage"), "Cold damage");
    assert_eq!(first_to_upper("1st"), "1st");
    // `function(_, skill, num) return grantedExtraSkill(skill, num) end`-style call
    let g = granted_extra_skill(&d, &s("aspect of the spider"), &s("20"), &Value::Nil).unwrap();
    assert!(value_eq(&g, &cache_mods("Grants Level 20 Aspect of the Spider Skill")));
    assert!(granted_extra_skill(&d, &s("no such skill"), &s("1"), &Value::Nil).unwrap().is_nil());
    // ModParser.lua:3325 `function(num, _, skill) return triggerExtraSkill(skill, num) end` (options nil)
    let t = trigger_extra_skill(&d, &s("storm cascade"), &Value::Num(20.0), &Value::Nil).unwrap();
    assert!(value_eq(&t, &cache_mods("Trigger Level 20 Storm Cascade when you Attack")), "{t:?}");
    for (support, line) in [
        ("added fire damage", "Socketed Gems are Supported by Level 20 Added Fire Damage"),
        ("brutality", "Socketed Gems are Supported by Level 10 Brutality"),
    ] {
        let lvl = if line.contains("Level 20") { "20" } else { "10" };
        let e = extra_support(&d, &s(support), &s(lvl), &Value::Nil, &Value::Nil).unwrap();
        assert!(value_eq(&e, &cache_mods(line)), "{line}: {e:?}");
    }
    let dn = deal_no_non_damage_type(&s("cold"), &Value::Nil).unwrap();
    let Value::Table(t) = dn else { panic!() };
    let names: Vec<String> = t
        .array
        .iter()
        .map(|m| if let Value::Mod(m) = m { m.name.to_string() } else { panic!() })
        .collect();
    assert_eq!(names, ["DealNoPhysical", "DealNoLightning", "DealNoFire", "DealNoChaos"]);
    assert!(deal_no_non_damage_type(&s("void"), &Value::Nil).unwrap().is_nil());
}
