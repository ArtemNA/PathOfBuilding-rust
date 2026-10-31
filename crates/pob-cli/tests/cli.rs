use std::io::Write;
use std::process::{Command, Output, Stdio};

use pob_golden::compare::compare_meta;
use pob_golden::{CandidateOutput, GOLDEN_BUILDS, compare_stats, find_golden, is_parity};

const BIN: &str = env!("CARGO_BIN_EXE_pob-cli");

fn dual_savior_xml() -> std::path::PathBuf {
    find_golden("Dual Savior").unwrap().xml_path()
}

fn run_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn calc_file_json_has_meta_and_allowlisted_stats() {
    let out = Command::new(BIN)
        .arg("calc")
        .arg(dual_savior_xml())
        .arg("--json")
        .env_remove("POB_EXPERIMENTAL")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let parsed = CandidateOutput::from_json(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
    assert_eq!(
        parsed.stats.len(),
        504,
        "Dual Savior: every ALLOWLIST key it emits (= oracle ok count)"
    );
    assert!(
        parsed.stats.keys().all(|k| pob_calc::is_allowed(k)),
        "stats are filtered by ALLOWLIST"
    );
    assert!(parsed.meta.is_some());
    // R6.7: the buff phase runs (no R6.7 note is left). R6.8: the ailment / exposure / ally life phases run too.
    assert!(!parsed.unimplemented.iter().any(|u| u.contains("not handed to perform yet")));
    assert!(!parsed.unimplemented.iter().any(|u| u.contains("per-skill reservation loop")));
    assert!(!parsed.unimplemented.iter().any(|u| u.contains("(R6.7)") || u.contains("3124-3341")));
    assert!(
        !parsed
            .unimplemented
            .iter()
            .any(|u| u.contains("3497-3620") || u.contains("3627-3665") || u.contains("3667-3789"))
    );
    // R7.3: offence part A runs; on Dual Savior the top env stops at the unported `calcs.mirages` branch (R7.9) before it.
    assert!(
        parsed
            .unimplemented
            .iter()
            .any(|u| u == "CalcMirages.lua:120-164 Reflection (The Saviour) for Reflection (R7.9)")
    );
}

/// `POB_EXPERIMENTAL=1`: the raw output, a strict superset of the filtered one (pob-ts `candidate.test.ts`).
#[test]
fn experimental_env_disables_the_filter() {
    let run = |experimental: bool| {
        let mut cmd = Command::new(BIN);
        cmd.arg("calc").arg(dual_savior_xml()).arg("--json").env_remove("POB_EXPERIMENTAL");
        if experimental {
            cmd.env("POB_EXPERIMENTAL", "1");
        }
        let out = cmd.output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        CandidateOutput::from_json(std::str::from_utf8(&out.stdout).unwrap()).unwrap()
    };
    let (filtered, raw) = (run(false), run(true));
    assert!(raw.stats.len() > filtered.stats.len());
    assert!(filtered.stats.keys().all(|k| raw.stats.contains_key(k)));
    assert!(!raw.stats.contains_key("_meta") && !filtered.stats.contains_key("_meta"));
    // The raw run holds exactly the ALLOWLIST ∪ EXCLUDED keys (535 on Dual Savior is a subset; all of them are classified).
    assert!(
        raw.stats
            .keys()
            .all(|k| pob_calc::is_allowed(k) || pob_calc::EXCLUDED.iter().any(|(e, _)| e == k))
    );
}

/// R4-ARCH gate (в) at the CLI boundary: for each golden, `pob-cli calc --json` stats against `snapshot.stats` (exact doubles, O8).
/// Pinned `ok` / `exact` = the pob-ts candidate run on the same goldens (measured 2026-10-31: 266/265, 268/267, 268/267, 274/273, 269/268, 285/284; not the 492/.. of the task card).
/// R5.4 (`ParseMode::Full`, the real ModParser, re-measured allowlist): 296/295, 303/302, 303/302, 314/313, 302/301, 320/319 -- the same
/// numbers `npm run oracle -- <golden> --cand tools/oracle-adapter/candidate.ts` prints. R5.5a (cluster-jewel subgraphs,
/// allowlist re-measured 333 -> 353): 316/315, 323/322, 323/322, 334/333, 322/321, 340/339 (the same `npm run oracle`).
/// R5.5b (radius jewels, allowlist 353 -> 354, `Spec:EnergyShieldInc`): 317/316, 324/323, 324/323, 335/334, 323/322, 341/340.
/// R5.5c (timeless jewels, allowlist 354 -> 377): 340/339, 347/346, 347/346, 358/357, 346/345, 364/363 -- +23 on every golden
/// (the 23 keys failed only on Lethal Pride before).
/// R6.3 (skills in initEnv: gem requirements, socket multipliers; allowlist 377 -> 381): 343/342, 351/350, 351/350, 362/361, 350/349,
/// 368/367 -- +3 on Dual Savior (`ReqDex`, `ReqIntString`, `ReqStr`; it emits no `ReqInt`), +4 elsewhere.
/// R6.5 (skills + flasks in perform, CP1 = LuaJIT; allowlist 381 -> 411): 369/368, 379/378, 379/378, 390/389, 380/379, 394/393 (the same
/// `npm run oracle`).
/// R6.6 (per-skill reservations, CP2 = LuaJIT; allowlist 411 -> 414: +4 `Mana(Un)Reserved*`, -1 `ReqIntString` on synthetic evidence): 372/371,
/// 382/381, 382/381, 393/392, 383/382, 397/396 (the same `npm run oracle`).
/// R6.7 (buffs/debuffs/curses/guards, CP3 = LuaJIT; allowlist 414 -> 542): 490/489, 500/499, 500/499, 514/513, 501/500, 522/521.
/// R6.8 (ailments/exposures/ally life, CP4 = LuaJIT; allowlist 542 -> 555: +10 `Current*`/`Maximum*` ailments, +3 former `ailments` keys):
/// 502/501, 512/511, 512/511, 526/525, 513/512, 535/534.
/// R7.3 (offence part A, OC1/OC2 = LuaJIT where the top offence runs; allowlist 555 -> 598): OccVortex 578/577 (+43); the other five
/// unchanged (their top offence stops at the unported triggers/mirages branch).
/// R7.4 (offence part B, OC3 = LuaJIT on every offence run Rust executes; allowlist 598 -> 605): OccVortex 585/584 (+7: `Accuracy`,
/// `AccuracyHitChance`, `CastRate`, `HitChance`, `Speed`, `Time`, `enemyBlockChance`); the other five unchanged (gated top offence).
/// R7.5 (offence part C, OC4 = LuaJIT on every offence run Rust executes but the 3 R7.10 minion runs; allowlist 605 -> 711): OccVortex
/// 691/690 (+106: damage by type, crits, leech, gain on hit / kill, DPS, warcry / Ruthless / Fist of War); the other five unchanged.
/// R7.6 (offence part D -- ailments, OC5 = LuaJIT on every offence run Rust executes but the 3 R7.10 minion runs; allowlist 711 -> 760):
/// OccVortex 740/739 (+49: ailment chances, ignite / chill / freeze / shock, knockback, impale chances, enemy stun); the other five unchanged.
/// R7.7 (offence part E + calcFullDPS + buildOutput tail, OC6 = LuaJIT on the same runs; allowlist 760 -> 778): every golden +2 (`FullDPS`,
/// `FullDotDPS` = 0: no `includeInFullDPS` group), OccVortex +16 more (758/757: skill DoT, combined DPS, per-second mana cost).
/// O9: `VaalArcticArmourMitigation` is `ok`, not `exact` (golden `+0`, Rust `-0`) -- hence exact = ok - 1.
#[test]
fn cli_stats_against_goldens() {
    let pinned: [(&str, usize, usize); 6] = [
        ("Dual Savior", 504, 503),
        ("Dual Wield Cospris CoC", 514, 513),
        ("Dual Wield Cospris CoC - Lethal Pride", 514, 513),
        ("Generals Perforate Zerker", 528, 527),
        ("Mirage Archer Toxic Rain", 515, 514),
        ("OccVortex", 758, 757),
    ];
    let mut got = Vec::new();
    for (g, (name, _, _)) in GOLDEN_BUILDS.iter().zip(pinned) {
        assert_eq!(g.name, name);
        let snap = g.load_snapshot().unwrap();
        let out = Command::new(BIN)
            .arg("calc")
            .arg(g.xml_path())
            .arg("--json")
            .env_remove("POB_EXPERIMENTAL")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{name}");
        let cand = CandidateOutput::from_json(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
        let report = compare_stats(&snap.stats, &cand.stats, &[]);
        println!("{name}: {}", pob_golden::summary_line(&report, false));
        assert_eq!((report.mismatch, report.extra), (0, 0), "{name}: mismatch/extra");
        assert!(is_parity(&report, false), "{name}");
        got.push((name, report.ok, report.exact));
    }
    assert_eq!(got, pinned, "ok/exact per golden");
}

/// The bench entry runs and prints one row per build.
#[test]
fn bench_prints_a_row() {
    let out = Command::new(BIN).args(["bench", "Dual Savior", "--runs", "2"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Dual Savior: total "), "{text}");
    assert_eq!(Command::new(BIN).args(["bench"]).output().unwrap().status.code(), Some(2));
    assert_eq!(
        Command::new(BIN)
            .args(["bench", "x", "--runs", "0"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn calc_stdin_matches_file_byte_for_byte() {
    let path = dual_savior_xml();
    let from_file = Command::new(BIN).arg("calc").arg(&path).arg("--json").output().unwrap();
    let from_stdin = run_stdin(&["calc", "-", "--json"], &std::fs::read(&path).unwrap());
    assert_eq!(from_stdin.status.code(), Some(0));
    assert_eq!(from_file.stdout, from_stdin.stdout);
}

#[test]
fn missing_file_exits_2() {
    let out = Command::new(BIN).args(["calc", "definitely-not-here.xml"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty());
}

#[test]
fn empty_stdin_exits_2() {
    let out = run_stdin(&["calc", "-", "--json"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("empty XML"));
}

#[test]
fn usage_and_unknown_args() {
    assert_eq!(Command::new(BIN).output().unwrap().status.code(), Some(2));
    assert_eq!(Command::new(BIN).arg("--help").output().unwrap().status.code(), Some(0));
    assert_eq!(Command::new(BIN).arg("bogus").output().unwrap().status.code(), Some(2));
    assert_eq!(
        Command::new(BIN).args(["calc", "-", "--bogus"]).output().unwrap().status.code(),
        Some(2)
    );
}

/// R3-ARCH §3 (c): `meta` against `_meta` of the six goldens (`compare_meta`, field by field, O7).
///
/// `socketGroups` = `#socketGroupList` after the skill phase appended the item/tree-granted groups (CalcSetup.lua:1448-1563): since R6.3
/// it matches on all six (was 8 vs 9 on Mirage Archer Toxic Rain and 6 vs 7 on OccVortex -- the "On Kill Monster Explosion" group
/// of a `CanExplode` tree node).
///
/// `allocNodes` (`#build.spec.allocNodes`, with the nodes `BuildClusterJewelGraphs` generates and the expansion sockets it
/// puts back, D16) matches on all six since R5.5a (was 98/98/97/103 on the 4 cluster builds).
///
/// The diffs are pinned one by one (field, golden value, ours), so any other drift fails.
#[test]
fn meta_against_goldens() {
    type Diffs = &'static [(&'static str, i64, i64)];
    let pinned: [(&str, Diffs); 6] = [
        ("Dual Savior", &[]),
        ("Dual Wield Cospris CoC", &[]),
        ("Dual Wield Cospris CoC - Lethal Pride", &[]),
        ("Generals Perforate Zerker", &[]),
        ("Mirage Archer Toxic Rain", &[]),
        ("OccVortex", &[]),
    ];
    let mut zero = 0;
    for (g, (name, want)) in GOLDEN_BUILDS.iter().zip(pinned) {
        assert_eq!(g.name, name);
        let snap = g.load_snapshot().unwrap();
        let out = pob_cli::calc(&pob_build::read_build_file(g.xml_path()).unwrap()).unwrap();
        let diffs = compare_meta(&snap.meta, out.meta.as_ref());
        let got: Vec<(&str, i64, i64)> = diffs
            .iter()
            .map(|d| {
                let num = |v: &Option<serde_json::Value>| v.as_ref().and_then(serde_json::Value::as_i64).unwrap_or(-1);
                (d.field, num(&d.expected), num(&d.actual))
            })
            .collect();
        println!("{name}: {got:?}");
        assert_eq!(got, want, "{name}: _meta diffs");
        zero += usize::from(diffs.is_empty());
    }
    assert_eq!(zero, 6, "builds with 0 _meta diffs");
}
