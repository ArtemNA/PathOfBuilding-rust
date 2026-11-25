//! `data/mod-parser.json` as `pob_data::mod_parser` reads it (counts = P5-ARCH §2.1 / R5.1b) and the closure registry's
//! drift contract (every registered definition carries the sha of the Lua body `fns` records).

use pob_data::mod_parser::{PVal, mod_parser};
use pob_parser::closures::{HELPERS, all_entries, drift};
use pob_parser::closures_manual::MANUAL;

#[test]
fn tables_and_fns_count_as_upstream() {
    let d = mod_parser();
    let want = [
        ("formList", 85),
        // 07fa089bf (R-UP2): every count below measured in LuaJIT on the real upvalues of the inner parseMod
        // (tools/.scratch/count-modparser.ts, `pairs` count per table; fns = distinct definitions reachable through table values
        // and upvalues, fn_refs = function values inside the table values). 3dbca4463: modNameList 726, specialModList 3 117.
        // 5606cc64f (2.69.0, the same script): modFlagList 183 -> 184, specialModList 3 181 -> 3 182.
        ("modNameList", 728),
        ("modFlagList", 184),
        ("preFlagList", 191),
        ("modTagList", 667),
        ("specialModList", 3_182),
        ("jewelFuncList", 142),
        ("clusterJewelSkills", 362),
        ("unsupportedModList", 3),
        ("preSkillNameList", 2_456),
        ("skillNameList", 664),
        ("suffixTypes", 35),
        ("penTypes", 6),
        ("flagTypes", 37),
        ("dmgTypes", 5),
        ("regenTypes", 32),
        ("degenTypes", 32),
        ("costTypes", 32),
        ("baseCostTypes", 32),
    ];
    assert_eq!(d.tables.len(), want.len());
    for (name, n) in want {
        assert_eq!(d.table(name).len(), n, "{name}");
    }
    assert_eq!(d.fns.len(), 1_292);
    // direct closure values per table (R5.1b §2): special 1 688, modTag 136, preFlag 7, jewel 142 (`{func = fn, type}`)
    let fn_values = |name: &str| d.table(name).vals.iter().filter(|v| matches!(v, PVal::Fn(_))).count();
    assert_eq!(fn_values("specialModList"), 1_726);
    assert_eq!(fn_values("modTagList"), 137);
    assert_eq!(fn_values("preFlagList"), 7);
    let jewel = d.table("jewelFuncList");
    assert!(jewel.vals.iter().all(|v| v.field("func").is_some_and(|f| matches!(f, PVal::Fn(_)))));
    assert_eq!(d.fn_refs, 2_012);
    // everything else is static
    for name in pob_data::mod_parser::TABLE_NAMES {
        if !matches!(name, "specialModList" | "modTagList" | "preFlagList" | "jewelFuncList") {
            assert!(d.table(name).vals.iter().all(|v| v.as_lua().is_some()), "{name}");
        }
    }
    // L9: the two `^tornado shot fires ...` entries keep a tag at [2] with [1] nil
    assert_eq!(d.table("specialModList").orphan_tags.len(), 2);
    assert_eq!(d.gem_id_lookup.len(), d.gem_id_lookup.len().max(1));
    // 07fa089bf: + "mazcotl" (ModParser.lua:46-47; 29 `["..."]` entries counted in the Lua table)
    assert_eq!(d.conqueror_list.len(), 29);
}

#[test]
fn registered_sha_matches_fns() {
    let d = mod_parser();
    let entries = all_entries();
    assert_eq!(entries.len(), HELPERS.len() + MANUAL.len());
    assert_eq!(drift(&entries, &d), Vec::<String>::new());
    println!("registry: {}/{} sha = mod-parser.json fns", entries.len(), entries.len());
}

#[test]
fn a_tampered_sha_or_range_is_red() {
    let d = mod_parser();
    let mut entries = all_entries();
    entries[3].sha = "0000000000000000000000000000000000000000000000000000000000000000";
    entries[5].to += 1;
    entries[7].id = "Modules/ModParser.lua:1";
    let red = drift(&entries, &d);
    assert_eq!(red.len(), 3, "{red:#?}");
}
