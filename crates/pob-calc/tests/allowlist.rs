//! R4.5 -- R4-ARCH gate (b): `ALLOWLIST` / `EXCLUDED` are MEASURED, not chosen. For the six goldens this re-runs the
//! full engine (`calculate_file`, default options -- since R5.4 `ParseMode::Full`, the real ModParser) and compares `stats` with `snapshot.stats` through
//! `pob_golden::compare_stats` (round4, O3; exact doubles, O8). A key is in `ALLOWLIST` iff it is `ok` on every golden
//! that emits it and `extra` on none; every key Rust emits on any golden is in exactly one of the two lists.
//! Any drift -- a key that starts or stops matching -- fails here.

use std::collections::{BTreeMap, BTreeSet};

use pob_calc::allowlist::{ALLOWLIST, EXCLUDED, ROOT_CAUSES, SYNTH_EXCLUDED, filter_stats, is_allowed, root_causes};
use pob_calc::{CalculateOptions, StatValue, calculate_file};
use pob_golden::{GOLDEN_BUILDS, StatClass, compare_stats, value};

fn to_golden(v: &StatValue) -> value::StatValue {
    match v {
        StatValue::Num(n) => value::StatValue::Num(*n),
        StatValue::Str(s) => value::StatValue::Str(s.clone()),
        StatValue::Bool(b) => value::StatValue::Bool(*b),
    }
}

/// Everything the measurement needs about one key.
#[derive(Default)]
struct Seen {
    /// Goldens (by index) where Rust emits the key and it is `ok`.
    ok: Vec<usize>,
    /// Goldens where it is `mismatch` or `extra` (disqualifying).
    bad: Vec<(usize, StatClass)>,
}

/// key -> its classes over the six goldens; keys Rust does not emit never appear (they are `unimplemented`).
fn measure() -> BTreeMap<String, Seen> {
    let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
    for (i, g) in GOLDEN_BUILDS.iter().enumerate() {
        let snap = g.load_snapshot().unwrap();
        let res = calculate_file(g.xml_path(), CalculateOptions::default()).unwrap();
        let ours: pob_golden::Stats = res.stats.iter().map(|(k, v)| (k.clone(), to_golden(v))).collect();
        let report = compare_stats(&snap.stats, &ours, &[]);
        for e in &report.entries {
            match e.cls {
                StatClass::Unimplemented => {}
                StatClass::Ok => seen.entry(e.key.clone()).or_default().ok.push(i),
                c => seen.entry(e.key.clone()).or_default().bad.push((i, c)),
            }
        }
    }
    seen
}

/// `cargo test -p pob-calc --test allowlist dump -- --ignored --nocapture`: prints the measured split (to regenerate the lists).
#[test]
#[ignore = "prints the measurement; not a check"]
fn dump() {
    for (k, s) in measure() {
        let tag = if s.bad.is_empty() { "ALLOW" } else { "EXCL" };
        println!("{tag}\t{k}\t{:?}", s.bad);
    }
}

#[test]
fn allowlist_is_the_measured_set() {
    let seen = measure();
    let measured_allow: BTreeSet<&str> = seen.iter().filter(|(_, s)| s.bad.is_empty()).map(|(k, _)| k.as_str()).collect();
    let mut measured_excl: BTreeSet<&str> = seen.iter().filter(|(_, s)| !s.bad.is_empty()).map(|(k, _)| k.as_str()).collect();
    // R6.6: `SYNTH_EXCLUDED` keys are ok on every golden but measured wrong on synthetic LuaJIT inputs: excluded on that evidence. Each must
    // still be emitted and ok here (once it mismatches on a golden it is an ordinary EXCLUDED key and leaves the list).
    let mut measured_allow = measured_allow;
    for k in SYNTH_EXCLUDED {
        assert!(measured_allow.remove(k), "{k}: SYNTH_EXCLUDED but not measured ok on every golden");
        measured_excl.insert(k);
    }
    let allow: BTreeSet<&str> = ALLOWLIST.iter().copied().collect();
    let excl: BTreeSet<&str> = EXCLUDED.iter().map(|(k, _)| *k).collect();

    assert_eq!(allow.len(), ALLOWLIST.len(), "ALLOWLIST has duplicates");
    assert_eq!(excl.len(), EXCLUDED.len(), "EXCLUDED has duplicates");
    assert!(ALLOWLIST.windows(2).all(|w| w[0] < w[1]), "ALLOWLIST must be sorted (byte order)");
    assert!(
        allow.is_disjoint(&excl),
        "ALLOWLIST and EXCLUDED overlap: {:?}",
        allow.intersection(&excl).collect::<Vec<_>>()
    );

    assert_eq!(
        diff(&measured_allow, &allow),
        Vec::<&str>::new(),
        "measured ok everywhere but not in ALLOWLIST"
    );
    assert_eq!(
        diff(&allow, &measured_allow),
        Vec::<&str>::new(),
        "in ALLOWLIST but not ok everywhere"
    );
    assert_eq!(diff(&measured_excl, &excl), Vec::<&str>::new(), "measured bad but not in EXCLUDED");
    assert_eq!(
        diff(&excl, &measured_excl),
        Vec::<&str>::new(),
        "in EXCLUDED but not measured bad (or not emitted)"
    );
    println!(
        "ALLOWLIST {} keys, EXCLUDED {} keys, emitted {}",
        ALLOWLIST.len(),
        EXCLUDED.len(),
        seen.len()
    );
    // R4.5: 289 / 246 (ModCache only); R5.4 (ParseMode::Full): 333 / 208; R5.5a (cluster subgraphs): 353 / 188;
    // R5.5b (radius jewels): 354 / 187; R5.5c (timeless jewels): 377 / 164; R6.3 (skills in initEnv, gem requirements): 381 / 160;
    // R6.5 (skills + flasks in perform, CP1 = LuaJIT): 411 / 142; R6.6 (reservations, CP2 = LuaJIT; ReqIntString -> SYNTH_EXCLUDED): 414 / 139;
    // R6.7 (buffs/debuffs/curses/guards, CP3 = LuaJIT): 542 / 14; R6.8 (ailments/exposures/ally life, CP4 = LuaJIT): 555 / 11;
    // R7.3 (offence part A; OC1/OC2 = LuaJIT where the top offence runs): 598 / 13;
    // R7.4 (offence part B; OC3 = LuaJIT on every offence run Rust executes): 605 / 13
    assert_eq!(ALLOWLIST.len(), 605, "pinned size of the measured allowlist");
    assert_eq!(EXCLUDED.len(), 13, "pinned size of the measured exclusions");
}

#[test]
fn every_excluded_key_names_a_root_cause() {
    let mut tally: BTreeMap<&str, usize> = ROOT_CAUSES.iter().map(|c| (*c, 0)).collect();
    for (k, reason) in EXCLUDED {
        let causes = root_causes(reason);
        assert!(!causes.is_empty(), "{k}: reason names no root cause: {reason}");
        // R5.4 closed `item-miss` (the real parser answers every item line): a key still excluded has another root
        assert!(!causes.contains(&"item-miss"), "{k}: item-miss is closed since R5.4: {reason}");
        // R5.5c closed `tree-jewel` (env class 0 on every golden, the Lethal Pride conquest = LuaJIT)
        assert!(!causes.contains(&"tree-jewel"), "{k}: tree-jewel is closed since R5.5c: {reason}");
        // R5.6 closed `cluster` (env Full = LuaJIT on every golden, every class 0)
        assert!(!causes.contains(&"cluster"), "{k}: cluster is closed since R5.5a/R5.6: {reason}");
        // R6.6 closed `reservation` (CP2 = LuaJIT on every golden)
        assert!(!causes.contains(&"reservation"), "{k}: reservation is closed since R6.6: {reason}");
        // R6.7 closed `buffs`, `curses`, `guards` and `skills` (CP3 = LuaJIT on every golden)
        for closed in ["buffs", "curses", "guards", "skills"] {
            assert!(!causes.contains(&closed), "{k}: {closed} is closed since R6.7: {reason}");
        }
        // R6.8 closed `ailments` (CP4 = LuaJIT on every golden)
        assert!(!causes.contains(&"ailments"), "{k}: ailments is closed since R6.8: {reason}");
        for c in causes {
            *tally.get_mut(c).unwrap() += 1;
        }
    }
    println!("EXCLUDED by root cause: {tally:?}");
}

#[test]
fn filter_keeps_exactly_the_allowlist() {
    let g = GOLDEN_BUILDS[0];
    let res = calculate_file(g.xml_path(), CalculateOptions::default()).unwrap();
    let filtered = filter_stats(&res.stats);
    assert!(filtered.len() < res.stats.len());
    assert!(filtered.keys().all(|k| is_allowed(k)));
    assert_eq!(filtered.len(), res.stats.keys().filter(|k| is_allowed(k)).count());
    // Order of the underlying map is preserved.
    let expect: Vec<&String> = res.stats.keys().filter(|k| is_allowed(k)).collect();
    assert_eq!(filtered.keys().collect::<Vec<_>>(), expect);
}

fn diff<'a>(a: &BTreeSet<&'a str>, b: &BTreeSet<&'a str>) -> Vec<&'a str> {
    a.difference(b).copied().collect()
}
