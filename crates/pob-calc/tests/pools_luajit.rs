//! R4.4a / R4-ARCH gate (g): the pool helpers of CalcDefence.lua (`src/pools.rs`) against LuaJIT, bit for bit.
//!
//! Fixtures `tests/fixtures/<build>.pools.luajit.json` (producer `tools/lua-corpus/export-r4.ts`, probe = pob-ts
//! `test/calc/estimations-ehp-oracle.test.ts:27-118`): `inputs` (21 `reducePoolsByDamage` cases = 3 pool sets x 7 damage
//! tables, 15 `takenHitFromDamage` calls), `results` (LuaJIT answers, `%.17g` strings, `nil`/`nan`/`inf` tokens) and
//! `actorReads` (every `output`/`Flag`/`Sum`/`damageShiftTable` value the helpers read, taken off the LuaJIT actor).
//! The Rust actor answers ONLY those reads -- anything else panics, so a read the probe did not capture cannot pass silently.
//! Every column of every call is compared as a `%.17g` string (`-nan` folded into `nan`, as the producer does).

use pob_calc::pools;

use std::collections::HashMap;

use pob_calc::pools::{Aegis, DMG_TYPE_LIST, DamageTable, DmgType, Guard, PoolActor, PoolTable, PoolsResult};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use serde_json::Value as Json;

/// 21 pool calls x 32 columns + 15 taken calls x 6 columns.
const FIELDS_PER_BUILD: usize = 21 * 32 + 15 * 6;

fn parse_num(s: &str) -> Option<f64> {
    match s {
        "nil" => None,
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        "nan" | "-nan" => Some(f64::NAN),
        _ => Some(s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}"))),
    }
}

fn fmt(v: Option<f64>) -> String {
    match v {
        None => "nil".to_owned(),
        Some(v) => canon(&format_g(v, 17)),
    }
}

fn canon(s: &str) -> String {
    if s == "-nan" { "nan".to_owned() } else { s.to_owned() }
}

fn strs(v: &Json) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_str().expect("string").to_owned())
        .collect()
}

/// The LuaJIT actor, reduced to what the probe captured.
struct FixtureActor {
    reads: HashMap<String, Option<f64>>,
    flags: HashMap<String, bool>,
    sums: HashMap<String, f64>,
    shift: [[Option<f64>; 5]; 5],
}

impl PoolActor for FixtureActor {
    fn output(&self, key: &str) -> Option<f64> {
        *self
            .reads
            .get(key)
            .unwrap_or_else(|| panic!("output read not captured by the probe: {key}"))
    }
    fn flag(&self, name: &str) -> bool {
        *self
            .flags
            .get(name)
            .unwrap_or_else(|| panic!("flag not captured by the probe: {name}"))
    }
    fn sum_base(&self, name: &str) -> f64 {
        *self
            .sums
            .get(&format!("BASE:{name}"))
            .unwrap_or_else(|| panic!("Sum BASE not captured: {name}"))
    }
    fn sum_more(&self, name: &str) -> f64 {
        *self
            .sums
            .get(&format!("MORE:{name}"))
            .unwrap_or_else(|| panic!("Sum MORE not captured: {name}"))
    }
    fn shift_table(&self, damage_type: DmgType) -> Option<[Option<f64>; 5]> {
        Some(self.shift[damage_type as usize])
    }
}

fn actor_of(fix: &Json) -> FixtureActor {
    let inputs = &fix["inputs"];
    let ar = &fix["actorReads"];
    let reads = ar["reads"]
        .as_object()
        .expect("reads")
        .iter()
        .map(|(k, v)| (k.clone(), parse_num(v.as_str().expect("read"))))
        .collect();
    let flags = strs(&inputs["flags"])
        .into_iter()
        .zip(strs(&ar["flags"]))
        .map(|(k, v)| (k, v == "true"))
        .collect();
    let sums = strs(&inputs["sums"])
        .into_iter()
        .zip(strs(&ar["sums"]))
        .map(|(k, v)| (k, parse_num(&v).expect("Sum is a number")))
        .collect();
    let shift_flat = strs(&ar["shift"]);
    assert_eq!(shift_flat.len(), 25);
    let mut shift = [[None; 5]; 5];
    for (i, s) in shift_flat.iter().enumerate() {
        shift[i / 5][i % 5] = parse_num(s);
    }
    assert_eq!(strs(&inputs["types"]), DMG_TYPE_LIST.to_vec());
    FixtureActor { reads, flags, sums, shift }
}

fn num_field(o: &Json, k: &str) -> Option<f64> {
    o.get(k).map(|v| v.as_f64().unwrap_or_else(|| panic!("{k}: not a number")))
}

fn typed(o: &Json) -> [Option<f64>; 5] {
    DMG_TYPE_LIST.map(|t| num_field(o, t))
}

/// The probe's pool table: `{ Life, Mana, EnergyShield, Ward, Aegis = copy, Guard = copy, AlliesTakenBeforeYou = {} }`.
fn pool_table_of(pool: &Json) -> PoolTable {
    let a = &pool["Aegis"];
    let g = &pool["Guard"];
    PoolTable {
        allies_taken_before_you: Some([None; 9]),
        aegis: Some(Aegis {
            shared: num_field(a, "shared"),
            shared_elemental: num_field(a, "sharedElemental"),
            typed: typed(a),
        }),
        guard: Some(Guard {
            shared: num_field(g, "shared"),
            typed: typed(g),
        }),
        ward: num_field(pool, "Ward"),
        energy_shield: num_field(pool, "EnergyShield"),
        mana: num_field(pool, "Mana"),
        life: num_field(pool, "Life"),
        ..PoolTable::default()
    }
}

fn pool_column(r: &PoolsResult, damage: &DamageTable, col: &str) -> Option<f64> {
    let ty = |s: &str| DmgType::from_name(s).unwrap_or_else(|| panic!("type {s}")) as usize;
    if let Some(t) = col.strip_prefix("recouped.") {
        return r.damage_taken_that_can_be_recouped[ty(t)];
    }
    if let Some(t) = col.strip_prefix("damageTable.") {
        return damage[ty(t)];
    }
    if let Some(k) = col.strip_prefix("Aegis.") {
        return match k {
            "shared" => r.aegis.shared,
            "sharedElemental" => r.aegis.shared_elemental,
            t => r.aegis.typed[ty(t)],
        };
    }
    if let Some(k) = col.strip_prefix("Guard.") {
        return match k {
            "shared" => r.guard.shared,
            t => r.guard.typed[ty(t)],
        };
    }
    Some(match col {
        "Ward" => r.ward,
        "WardActiveChance" => r.ward_active_chance,
        "EnergyShield" => r.energy_shield,
        "Mana" => r.mana,
        "Life" => r.life,
        "LifeLossLostOverTime" => r.life_loss_lost_over_time,
        "LifeBelowHalfLossLostOverTime" => r.life_below_half_loss_lost_over_time,
        "OverkillDamage" => r.overkill_damage,
        "hitPoolRemaining" => r.hit_pool_remaining,
        other => panic!("unknown pool column {other}"),
    })
}

fn taken_column(sum: f64, damages: &DamageTable, col: &str) -> Option<f64> {
    if col == "sum" {
        return Some(sum);
    }
    let t = col.strip_prefix("damages.").unwrap_or_else(|| panic!("unknown taken column {col}"));
    damages[DmgType::from_name(t).expect("type") as usize]
}

/// Returns (fields compared, mismatches).
fn check_build(name: &str) -> (usize, Vec<String>) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{}.pools.luajit.json", name.replace(' ', "-")));
    let fix: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json");
    let actor = actor_of(&fix);
    let inputs = &fix["inputs"];
    let pool_cols = strs(&inputs["poolColumns"]);
    let taken_cols = strs(&inputs["takenColumns"]);
    let mut total = 0;
    let mut bad = Vec::new();
    let mut cmp = |label: &str, col: &str, ours: Option<f64>, theirs: &str| {
        total += 1;
        if fmt(ours) != canon(theirs) {
            bad.push(format!("{name} {label}.{col}: rust {} != luajit {theirs}", fmt(ours)));
        }
    };

    let cases = inputs["cases"].as_array().expect("cases");
    let pool_results = fix["results"]["pools"].as_array().expect("results.pools");
    assert_eq!(cases.len(), 21);
    assert_eq!(pool_results.len(), cases.len());
    for (case, row) in cases.iter().zip(pool_results) {
        let label = case["name"].as_str().expect("name");
        let mut damage: DamageTable = typed(&case["damage"]);
        let r = pools::reduce_pools_by_damage(Some(pool_table_of(&case["pool"])), &mut damage, &actor)
            .unwrap_or_else(|e| panic!("{name} {label}: {e}"));
        let row = strs(row);
        assert_eq!(row.len(), pool_cols.len());
        for (col, theirs) in pool_cols.iter().zip(&row) {
            cmp(label, col, pool_column(&r, &damage, col), theirs);
        }
    }

    let taken = inputs["taken"].as_array().expect("taken");
    let taken_results = fix["results"]["taken"].as_array().expect("results.taken");
    assert_eq!(taken.len(), 15);
    assert_eq!(taken_results.len(), taken.len());
    for (call, row) in taken.iter().zip(taken_results) {
        let label = call["name"].as_str().expect("name");
        let raw = parse_num(call["raw"].as_str().expect("raw")).expect("raw number");
        let ty = DmgType::from_name(call["type"].as_str().expect("type")).expect("dmg type");
        let (sum, damages) = pools::taken_hit_from_damage(raw, ty, &actor).unwrap_or_else(|e| panic!("{name} {label}: {e}"));
        let row = strs(row);
        assert_eq!(row.len(), taken_cols.len());
        for (col, theirs) in taken_cols.iter().zip(&row) {
            cmp(label, col, taken_column(sum, &damages, col), theirs);
        }
    }
    (total, bad)
}

#[test]
fn pools_match_luajit_on_every_golden() {
    let mut report = Vec::new();
    let mut all_bad = Vec::new();
    for g in GOLDEN_BUILDS.iter() {
        let (total, bad) = check_build(g.name);
        report.push(format!("{}: {}/{} fields = LuaJIT", g.name, total - bad.len(), total));
        assert_eq!(total, FIELDS_PER_BUILD, "{}: field count", g.name);
        all_bad.extend(bad);
    }
    eprintln!("{}", report.join("\n"));
    assert_eq!(report.len(), 6, "six goldens");
    assert!(all_bad.is_empty(), "{} mismatches:\n{}", all_bad.len(), all_bad.join("\n"));
}
