//! R4.4c: the pool helpers (`src/pools.rs`) against the synthetic LuaJIT corpus `tests/fixtures/pools-synth.luajit.json`
//! (producer `tools/lua-corpus/export-pools-synth.ts`): 18 scenarios on a synthetic Dual Savior actor covering the branches
//! the goldens leave dead -- MoM, ES protects mana, prevented life loss, allies (explicit and from output), ward bypass /
//! not-break / avoid-break, output fallbacks, takenFlat, split shift rows, extreme pools, a nil energy shield.
//!
//! Shape: `columns.*` (pool columns = R4.1 + `Allies.<key>`), `scenarios[]` with `inputs.cases[]` (`pool: false` = nil pool
//! table; a missing `Aegis`/`Guard`/`Mana`/`Life`/... = nil field -> output fallback; a missing `Allies` = nil ->
//! `buildAllyLifePools`), `results` (`%.17g` strings; `["ERROR", msg]` = a LuaJIT runtime error) and `actorReads`.
//! A LuaJIT error counts as ONE field and matches when Rust returns a `PoolError` with the same `CalcDefence.lua` line
//! number (only the line is compared: the message wording is not ported). The actor answers only the captured reads.

use pob_calc::pools;

use std::collections::HashMap;

use pob_calc::pools::{
    ALLY_LIFE_POOL_LIST, Aegis, AllyPool, AllyPools, DMG_TYPE_LIST, DamageTable, DmgType, Guard, PoolActor, PoolError, PoolTable,
    PoolsResult,
};
use pob_lua::format_g;
use serde_json::Value as Json;

/// Producer report: pob-ts = LuaJIT on all of these.
const TOTAL_FIELDS: usize = 14_577;
const SCENARIOS: usize = 18;

fn parse_num(s: &str) -> Option<f64> {
    match s {
        "nil" => None,
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        "nan" | "-nan" => Some(f64::NAN),
        _ => Some(s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}"))),
    }
}

fn canon(s: &str) -> String {
    if s == "-nan" { "nan".to_owned() } else { s.to_owned() }
}

fn fmt(v: Option<f64>) -> String {
    v.map_or_else(|| "nil".to_owned(), |v| canon(&format_g(v, 17)))
}

fn strs(v: &Json) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_str().expect("string").to_owned())
        .collect()
}

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

fn actor_of(columns: &Json, ar: &Json) -> FixtureActor {
    let read_keys = strs(&columns["reads"]);
    let reads_obj = ar["reads"].as_object().expect("reads");
    assert_eq!(reads_obj.len(), read_keys.len(), "every READS key captured");
    let reads = reads_obj
        .iter()
        .map(|(k, v)| (k.clone(), parse_num(v.as_str().expect("read"))))
        .collect();
    let flags = strs(&columns["flags"])
        .into_iter()
        .zip(strs(&ar["flags"]))
        .map(|(k, v)| (k, v == "true"))
        .collect();
    let sums = strs(&columns["sums"])
        .into_iter()
        .zip(strs(&ar["sums"]))
        .map(|(k, v)| (k, parse_num(&v).expect("Sum is a number")))
        .collect();
    let flat = strs(&ar["shift"]);
    assert_eq!(flat.len(), 25);
    let mut shift = [[None; 5]; 5];
    for (i, s) in flat.iter().enumerate() {
        shift[i / 5][i % 5] = parse_num(s);
    }
    FixtureActor { reads, flags, sums, shift }
}

fn num_field(o: &Json, k: &str) -> Option<f64> {
    o.get(k).map(|v| v.as_f64().unwrap_or_else(|| panic!("{k}: not a number")))
}

fn typed(o: &Json) -> [Option<f64>; 5] {
    DMG_TYPE_LIST.map(|t| num_field(o, t))
}

fn allies_of(o: &Json) -> AllyPools {
    let obj = o.as_object().expect("Allies");
    for k in obj.keys() {
        assert!(ALLY_LIFE_POOL_LIST.iter().any(|a| a.key == k), "unknown ally {k}");
    }
    ALLY_LIFE_POOL_LIST.map(|a| {
        obj.get(a.key).map(|v| AllyPool {
            remaining: num_field(v, "remaining").expect("remaining"),
            percent: num_field(v, "percent").expect("percent"),
            damage_type: None,
        })
    })
}

/// The probe's pool table (`export-pools-synth.ts` PROBE): `false` = nil; absent fields = nil.
fn pool_table_of(pool: &Json) -> Option<PoolTable> {
    if pool == &Json::Bool(false) {
        return None;
    }
    let known = [
        "Life",
        "Mana",
        "EnergyShield",
        "Ward",
        "WardActiveChance",
        "Aegis",
        "Guard",
        "Allies",
    ];
    for k in pool.as_object().expect("pool").keys() {
        assert!(known.contains(&k.as_str()), "unknown pool field {k}");
    }
    Some(PoolTable {
        allies_taken_before_you: pool.get("Allies").map(allies_of),
        aegis: pool.get("Aegis").map(|a| Aegis {
            shared: num_field(a, "shared"),
            shared_elemental: num_field(a, "sharedElemental"),
            typed: typed(a),
        }),
        guard: pool.get("Guard").map(|g| Guard {
            shared: num_field(g, "shared"),
            typed: typed(g),
        }),
        ward: num_field(pool, "Ward"),
        ward_active_chance: num_field(pool, "WardActiveChance"),
        energy_shield: num_field(pool, "EnergyShield"),
        mana: num_field(pool, "Mana"),
        life: num_field(pool, "Life"),
        ..PoolTable::default()
    })
}

fn ty(s: &str) -> usize {
    DmgType::from_name(s).unwrap_or_else(|| panic!("type {s}")) as usize
}

fn pool_column(r: &PoolsResult, damage: &DamageTable, col: &str) -> Option<f64> {
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
    if let Some(k) = col.strip_prefix("Allies.") {
        let i = ALLY_LIFE_POOL_LIST
            .iter()
            .position(|a| a.key == k)
            .unwrap_or_else(|| panic!("ally {k}"));
        return r.allies_taken_before_you[i].map(|a| a.remaining);
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
    damages[ty(col.strip_prefix("damages.").unwrap_or_else(|| panic!("unknown taken column {col}")))]
}

/// `./Modules/CalcDefence.lua:241: attempt ...` -> 241.
fn lua_error_line(msg: &str) -> u32 {
    let rest = msg
        .split("CalcDefence.lua:")
        .nth(1)
        .unwrap_or_else(|| panic!("not a CalcDefence error: {msg}"));
    rest.split(':')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no line in {msg}"))
}

#[derive(Default)]
struct Tally {
    total: usize,
    errors: usize,
    bad: Vec<String>,
}

impl Tally {
    /// One call: LuaJIT row vs the Rust result (columns via `get`).
    fn call<T>(
        &mut self,
        label: &str,
        cols: &[String],
        theirs: &[String],
        ours: Result<T, PoolError>,
        get: impl Fn(&T, &str) -> Option<f64>,
    ) {
        if theirs.first().map(String::as_str) == Some("ERROR") {
            self.total += 1;
            self.errors += 1;
            let want = lua_error_line(&theirs[1]);
            match ours {
                Err(e) if e.line == want => {}
                Err(e) => self.bad.push(format!("{label}: luajit {} / rust {e}", theirs[1])),
                Ok(_) => self.bad.push(format!("{label}: luajit {} / rust no error", theirs[1])),
            }
            return;
        }
        assert_eq!(theirs.len(), cols.len(), "{label}: row width");
        self.total += cols.len();
        match ours {
            Err(e) => self.bad.push(format!("{label}: rust error {e}, luajit returned a row")),
            Ok(v) => {
                for (col, t) in cols.iter().zip(theirs) {
                    let o = fmt(get(&v, col));
                    if o != canon(t) {
                        self.bad.push(format!("{label}.{col}: rust {o} != luajit {t}"));
                    }
                }
            }
        }
    }
}

#[test]
fn pools_match_luajit_on_synthetic_corpus() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pools-synth.luajit.json");
    let fix: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json");
    let columns = &fix["columns"];
    assert_eq!(strs(&columns["types"]), DMG_TYPE_LIST.to_vec());
    assert_eq!(strs(&columns["allyKeys"]), ALLY_LIFE_POOL_LIST.map(|a| a.key.to_owned()).to_vec());
    let pool_cols = strs(&columns["pool"]);
    let taken_cols = strs(&columns["taken"]);
    let scenarios = fix["scenarios"].as_array().expect("scenarios");
    assert_eq!(scenarios.len(), SCENARIOS);

    let mut tally = Tally::default();
    let mut report = Vec::new();
    for sc in scenarios {
        let name = sc["name"].as_str().expect("name");
        let actor = actor_of(columns, &sc["actorReads"]);
        let (t0, b0) = (tally.total, tally.bad.len());
        let cases = sc["inputs"]["cases"].as_array().expect("cases");
        let rows = sc["results"]["pools"].as_array().expect("results.pools");
        assert_eq!(cases.len(), rows.len(), "{name}");
        for (case, row) in cases.iter().zip(rows) {
            let label = format!("{name} / {}", case["name"].as_str().expect("case name"));
            let mut damage: DamageTable = typed(&case["damage"]);
            let r = pools::reduce_pools_by_damage(pool_table_of(&case["pool"]), &mut damage, &actor);
            tally.call(&label, &pool_cols, &strs(row), r, |r, col| pool_column(r, &damage, col));
        }
        let taken = sc["inputs"]["taken"].as_array().expect("taken");
        let rows = sc["results"]["taken"].as_array().expect("results.taken");
        assert_eq!(taken.len(), rows.len(), "{name}");
        for (call, row) in taken.iter().zip(rows) {
            let label = format!("{name} / taken {}", call["name"].as_str().expect("call name"));
            let raw = parse_num(call["raw"].as_str().expect("raw")).expect("raw number");
            let r = pools::taken_hit_from_damage(
                raw,
                DmgType::from_name(call["type"].as_str().expect("type")).expect("dmg type"),
                &actor,
            );
            tally.call(&label, &taken_cols, &strs(row), r, |(sum, damages), col| {
                taken_column(*sum, damages, col)
            });
        }
        let n = tally.total - t0;
        report.push(format!("{name}: {}/{n}", n - (tally.bad.len() - b0)));
    }
    eprintln!("{}\nLuaJIT errors (line-matched): {}", report.join("\n"), tally.errors);
    assert_eq!(tally.total, TOTAL_FIELDS, "field count");
    assert!(
        tally.bad.is_empty(),
        "{} of {} fields differ:\n{}",
        tally.bad.len(),
        tally.total,
        tally.bad.join("\n")
    );
}
