//! pob-golden against the six committed goldens of pob-ts, plus the red-first (LESSONS 2) negative tests:
//! a corrupted reference must FAIL the gate.

use pob_golden::compare::{StatClass, compare_meta, compare_stats, is_parity, render_report, round4};
use pob_golden::{CandidateOutput, GOLDEN_BUILDS, PartialMeta, StatValue, Stats};

/// Key counts of the goldens (PLAN.md progress bar, POB-TS-MAP §1).
const TOTALS: [(&str, usize); 6] = [
    ("Dual Savior", 632),
    ("Dual Wield Cospris CoC", 749),
    ("Dual Wield Cospris CoC - Lethal Pride", 749),
    ("Generals Perforate Zerker", 649),
    ("Mirage Archer Toxic Rain", 658),
    ("OccVortex", 773),
];

fn stats(pairs: &[(&str, StatValue)]) -> Stats {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

#[test]
fn round4_matches_common_lua_formula() {
    // Expected values: JS Math.floor(v*10000+0.5)/10000 (same IEEE ops as Common.lua round), node 24, 2026-10-31.
    let cases = [
        (3707.62245, 3707.6225),
        (3707.6224499999994, 3707.6224), // O8: the exact channel falls on the other side of the tie
        (14852.58635, 14852.5864),
        (14852.586349999998, 14852.5863),
        (-1.00005, -1.0001),
        (2.5, 2.5),
        (0.00005, 0.0001),
        (1e300, 1e300),
    ];
    for (v, want) in cases {
        assert_eq!(round4(v).to_bits(), f64::to_bits(want), "round4({v})");
    }
    let z = round4(-0.00005);
    assert!(z == 0.0 && z.is_sign_positive(), "round4(-0.00005) = +0 in JS");
    assert!(round4(f64::NAN).is_nan());
    assert_eq!(round4(f64::INFINITY), f64::INFINITY);
}

#[test]
fn classes_and_gate_on_synthetic_stats() {
    let reference = stats(&[
        ("A", 1.00004.into()),
        ("B", 2.0.into()),
        ("C", true.into()),
        ("D", f64::NAN.into()),
        ("E", 3.0.into()),
        ("Z", (-0.0).into()),
    ]);
    let candidate = stats(&[
        ("A", 1.0.into()),              // round4-equal, not exact
        ("B", 2.0001.into()),           // mismatch
        ("C", true.into()),             // exact
        ("D", StatValue::from("-nan")), // canonical "nan" == "nan"
        ("X", 1.0.into()),              // extra
        ("Z", 0.0.into()),              // ok, but the sign of zero breaks exact
    ]);
    let r = compare_stats(&reference, &candidate, &[]);
    assert_eq!((r.total, r.ok, r.exact, r.mismatch, r.unimplemented, r.extra), (6, 4, 2, 1, 1, 1));
    let cls = |k: &str| r.entries.iter().find(|e| e.key == k).map(|e| e.cls);
    assert_eq!(cls("E"), Some(StatClass::Unimplemented));
    assert_eq!(cls("X"), Some(StatClass::Extra));
    assert!(!is_parity(&r, false));

    let tie = compare_stats(
        &stats(&[("T", 3707.62245.into())]),
        &stats(&[("T", 3707.6224499999994.into())]),
        &[],
    );
    assert_eq!(tie.mismatch, 1);
    assert!(tie.entries[0].near_tie, "O8 tie must be flagged nearTie");
    assert!(!is_parity(&tie, false));

    let num_vs_inf = compare_stats(&stats(&[("I", f64::INFINITY.into())]), &stats(&[("I", 1e308.into())]), &[]);
    assert_eq!(num_vs_inf.mismatch, 1);
    let ignored = compare_stats(&reference, &candidate, &["A", "B", "C", "D", "E", "X", "Z"]);
    assert_eq!(ignored.total + ignored.extra, 0);
}

#[test]
fn every_golden_parses_and_self_compares_bit_exact() {
    for (b, (name, total)) in GOLDEN_BUILDS.iter().zip(TOTALS) {
        assert_eq!(b.name, name);
        let snap = b.load_snapshot().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(snap.case.build, name);
        assert!(snap.case.mutations.is_empty());
        assert!(b.xml_path().is_file(), "{}: XML missing at {}", name, b.xml_path().display());
        let r = compare_stats(&snap.stats, &snap.stats, &[]);
        assert_eq!((r.total, r.ok, r.exact), (total, total, total), "{name}");
        assert!(is_parity(&r, true));
        assert!(compare_meta(&snap.meta, Some(&PartialMeta::from(&snap.meta))).is_empty());
    }
}

#[test]
fn corrupted_reference_fails_the_gate() {
    for b in GOLDEN_BUILDS {
        let snap = b.load_snapshot().unwrap();
        let (key, v) = snap
            .stats
            .iter()
            .find_map(|(k, v)| v.as_num().filter(|x| x.is_finite() && *x != 0.0).map(|x| (k.clone(), x)))
            .expect("a nonzero number");

        // A real change: mismatch 1, FAIL.
        let mut bad = snap.stats.clone();
        bad.insert(key.clone(), StatValue::Num(v + 1.0));
        let r = compare_stats(&bad, &snap.stats, &[]);
        assert_eq!((r.mismatch, r.ok + 1), (1, r.total), "{}", b.name);
        assert!(!is_parity(&r, false));
        assert!(render_report(&r, b.name, true, 50, &[]).contains("-> FAIL"));

        // One ulp: still round4-ok, but no longer exact.
        let mut ulp = snap.stats.clone();
        ulp.insert(key.clone(), StatValue::Num(f64::from_bits(v.to_bits() + 1)));
        let r = compare_stats(&ulp, &snap.stats, &[]);
        assert_eq!((r.ok, r.exact + 1, r.mismatch), (r.total, r.total, 0), "{}", b.name);

        // A missing key on the reference side = extra on the candidate side: FAIL.
        let mut fewer = snap.stats.clone();
        fewer.remove(&key);
        assert!(!is_parity(&compare_stats(&fewer, &snap.stats, &[]), false));

        // Corrupted _meta: exactly one diff.
        let mut meta = PartialMeta::from(&snap.meta);
        meta.alloc_nodes = meta.alloc_nodes.map(|n| n + 1);
        let diffs = compare_meta(&snap.meta, Some(&meta));
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "allocNodes");
        assert_eq!(
            compare_meta(&snap.meta, None).len(),
            if snap.meta.main_socket_group.is_some() { 7 } else { 6 }
        );
    }
}

#[test]
fn empty_candidate_is_all_unimplemented() {
    for b in GOLDEN_BUILDS {
        let snap = b.load_snapshot().unwrap();
        let r = compare_stats(&snap.stats, &Stats::new(), &[]);
        assert_eq!((r.unimplemented, r.ok, r.mismatch, r.extra), (r.total, 0, 0, 0));
        assert!(is_parity(&r, false), "unimplemented is allowed by the non-strict gate");
        assert!(!is_parity(&r, true));
    }
}

#[test]
fn candidate_json_round_trips_sign_of_zero_and_non_finite() {
    let out = CandidateOutput {
        stats: stats(&[
            ("NegZero", (-0.0).into()),
            ("Nan", f64::NAN.into()),
            ("Inf", f64::NEG_INFINITY.into()),
            ("Tiny", 5e-324.into()),
            ("Third", (1.0 / 3.0).into()),
            ("Flag", false.into()),
        ]),
        meta: None,
        unimplemented: vec!["Calcs.lua:384 buildOutput".to_owned()],
    };
    let json = out.to_json();
    assert!(json.contains("-0.0"), "{json}");
    let back = CandidateOutput::from_json(&json).unwrap();
    let r = compare_stats(&out.stats, &back.stats, &[]);
    assert_eq!((r.exact, r.total), (6, 6), "{json}");
    assert_eq!(back.stats["Nan"], StatValue::Str("nan".into()));
}
