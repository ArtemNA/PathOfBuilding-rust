//! The mode tails of `calcs.buildOutput` the Calcs tab reads (R8.6): `env.multipliersUsed` of the MAIN tail (Calcs.lua:457-584, the part the
//! CALCS tail reads -- `build.calcsTab.mainEnv.multipliersUsed`) and the CALCS tail (:601-829): `output.BuffList` / `CombatList` / `CurseList`,
//! the `SkillBuffs` / `SkillDebuffs` breakdown tables, and the minion's counterparts.
//!
//! `pairs(env.buffs)` / `pairs(env.debuffs)` / `pairs(env.minionBuffs)` (R13 -- string keys) are followed by `table.sort`, so the lists do not
//! depend on the walk; the rows of `SkillBuffs` / `SkillDebuffs` follow the sorted names, and inside one name the ModList order (the order
//! [`crate::perform::BuffTables`] keeps -- the probe sorts these rows, MINEFIELD-RUST R13).

use std::collections::HashSet;
use std::rc::Rc;

use pob_mods::{Mod, ModStore, ModType, StoreKind, StoreRef, TagKind, Value};

use crate::breakdown::{BTable, BVal, ModRef};
use crate::env::Env;
use crate::output::OutTable;
use crate::perform::PerformState;
use crate::setup::ok;

const CALCS: &str = "Modules/Calcs.lua";

/// `env.multipliersUsed` (Calcs.lua:457-584) -- the multiplier names the player's mods and the base skill lists of the player's active skills
/// tag (`Multiplier` / `MultiplierThreshold` with no `actor`, before an `IgnoreCond`; a mod whose source is `"Base"` is not counted).
pub fn multipliers_used(env: &Env) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut add = |m: &Mod| {
        for tag in &m.tags {
            match tag.kind() {
                TagKind::Multiplier | TagKind::MultiplierThreshold => {
                    if !tag.get("actor").truthy() {
                        // :470-480 `addVarTag` -> `addTo` (skips `mod.source == "Base"`)
                        if m.source.as_deref() == Some("Base") {
                            continue;
                        }
                        match tag.get("varList") {
                            Value::Table(t) => {
                                for v in &t.array {
                                    out.insert(var_name(v));
                                }
                            }
                            _ => {
                                out.insert(var_name(&tag.get("var")));
                            }
                        }
                    }
                }
                _ if tag.ty().as_str() == Some("IgnoreCond") => break,
                _ => {}
            }
        }
    };
    // :549-555 `for _, actor in ipairs({env.player, env.minion})` -- only the player's own mods add multipliers
    {
        let db = env.player.borrow().mod_db.clone();
        let d = db.borrow();
        if let StoreKind::Db(d) = &d.kind {
            for list in d.mods.values() {
                for m in list {
                    add(m);
                }
            }
        }
    }
    // :556-560 `activeSkill.baseSkillModList` of every player skill
    if let Some(st) = &env.skills {
        for sk in &st.active_skill_list {
            if let Some(l) = &sk.skill_mod_list {
                let l = l.borrow();
                if let StoreKind::List(l) = &l.kind {
                    for m in &l.mods {
                        add(m);
                    }
                }
            }
        }
    }
    out
}

fn var_name(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        // `out[nil] = { }` (:476)
        Value::Nil => panic!("{CALCS}:476: table index is nil"),
        other => unimplemented!("{CALCS}:476 env.multipliersUsed keyed by a {}", other.type_name()),
    }
}

/// A Lua value from `EvalMod` as a breakdown value.
fn bval(v: &Value) -> BVal {
    match v {
        Value::Nil => unreachable!("filtered: `value and value ~= 0`"),
        Value::Bool(b) => BVal::Bool(*b),
        Value::Num(n) => BVal::Num(*n),
        Value::Str(s) => BVal::Str(s.to_string()),
        Value::Mod(m) => BVal::Mod(ModRef(m.clone())),
        Value::Func(_) => BVal::Fn,
        Value::Table(t) => {
            let mut b = BTable::new();
            for x in &t.array {
                b.push(bval(x));
            }
            for (k, x) in &t.hash {
                b.fields.insert(k.to_string(), bval(x));
            }
            BVal::Table(b)
        }
    }
}

/// `{ mod = mod, value = value }` rows for every mod of `mods` whose `db:EvalMod(mod)` is neither nil/false nor 0.
fn eval_rows(db: &StoreRef, mods: &[Rc<Mod>], rows: &mut Vec<BVal>) {
    for m in mods {
        let v = ok(db.borrow().eval_mod(m, None, None));
        let keep = match &v {
            Value::Nil | Value::Bool(false) => false,
            Value::Num(n) => *n != 0.0,
            _ => true,
        };
        if keep {
            let mut row = BTable::new();
            row.fields.insert("mod".into(), BVal::Mod(ModRef(m.clone())));
            row.fields.insert("value".into(), bval(&v));
            rows.push(BVal::Table(row));
        }
    }
}

fn mod_list_table(rows: Vec<BVal>) -> BTable {
    let mut list = BTable::new();
    list.list = rows;
    BTable::new().with("modList", list)
}

fn flag(db: &StoreRef, name: &str) -> bool {
    ok(db.borrow().flag(None, &[name])) == Some(true)
}

/// `output[k] > 0` (a nil is the Lua compare error).
fn positive(o: &crate::output::Output, k: &str) -> bool {
    o.opt_num(k)
        .unwrap_or_else(|| panic!("{CALCS}: attempt to compare nil with number (output.{k})"))
        > 0.0
}

/// `s_format("%d ...", output[k])`.
fn count(o: &crate::output::Output, label: &str, k: &str) -> String {
    let v = o
        .opt_num(k)
        .unwrap_or_else(|| panic!("{CALCS}: bad argument #2 to 'format' (number expected, got nil) (output.{k})"));
    crate::sf!(&format!("%d {label}"), v)
}

const CHARGES: [(&str, &str); 10] = [
    ("PowerCharges", "Power Charges"),
    ("AbsorptionCharges", "Absorption Charges"),
    ("FrenzyCharges", "Frenzy Charges"),
    ("AfflictionCharges", "Affliction Charges"),
    ("EnduranceCharges", "Endurance Charges"),
    ("BrutalCharges", "Brutal Charges"),
    ("BrineCharges", "Brine Charges"),
    ("SiphoningCharges", "Siphoning Charges"),
    ("ChallengerCharges", "Challenger Charges"),
    ("BlitzCharges", "Blitz Charges"),
];

/// :656-752 `env.modDB:Flag(nil, <flag>)` -> `<label>`, in order.
const PLAYER_FLAGS: [(&str, &str); 33] = [
    ("Fortify", "Fortify"),
    ("Onslaught", "Onslaught"),
    ("UnholyMight", "Unholy Might"),
    ("ChaoticMight", "Chaotic Might"),
    ("Tailwind", "Tailwind"),
    ("Adrenaline", "Adrenaline"),
    ("AlchemistsGenius", "Alchemist's Genius"),
    ("HerEmbrace", "Her Embrace"),
    ("AccelerationShrine", "Acceleration Shrine"),
    ("BrutalShrine", "Brutal Shrine"),
    ("DiamondShrine", "Diamond Shrine"),
    ("DivineShrine", "Divine Shrine"),
    ("EchoingShrine", "Echoing Shrine"),
    ("GloomShrine", "Gloom Shrine"),
    ("GreaterFreezingShrine", "Greater Freezing Shrine"),
    ("GreaterShockingShrine", "Greater Shocking Shrine"),
    ("GreaterSkeletalShrine", "Greater Skeletal Shrine"),
    ("ImpenetrableShrine", "Impenetrable Shrine"),
    ("MassiveShrine", "Massive Shrine"),
    ("ReplenishingShrine", "Replenishing Shrine"),
    ("ResistanceShrine", "Resistance Shrine"),
    ("ResonatingShrine", "Resonating Shrine"),
    ("LesserAccelerationShrine", "Lesser Acceleration Shrine"),
    ("LesserBrutalShrine", "Lesser Brutal Shrine"),
    ("LesserImpenetrableShrine", "Lesser Impenetrable Shrine"),
    ("LesserMassiveShrine", "Lesser Massive Shrine"),
    ("LesserReplenishingShrine", "Lesser Replenishing Shrine"),
    ("LesserResistanceShrine", "Lesser Resistance Shrine"),
    ("BloodShrineOfRats", "Blood Shrine of Rats"),
    ("BloodShrineOfLocusts", "Blood Shrine of Locusts"),
    ("BloodShrineOfToads", "Blood Shrine of Toads"),
    ("BloodShrineOfCrows", "Blood Shrine of Crows"),
    ("BloodShrineOfBats", "Blood Shrine of Bats"),
];

/// :816-845 the minion's flags.
const MINION_FLAGS: [(&str, &str); 7] = [
    ("Fortify", "Fortify"),
    ("Onslaught", "Onslaught"),
    ("UnholyMight", "Unholy Might"),
    ("ChaoticMight", "Chaotic Might"),
    ("Tailwind", "Tailwind"),
    ("DiamondShrine", "Diamond Shrine"),
    ("MassiveShrine", "Massive Shrine"),
];

/// Calcs.lua:601-829, `elseif mode == "CALCS"`. `main_multipliers` = `build.calcsTab.mainEnv.multipliersUsed` (the MAIN run of the same
/// `CalcsTab:BuildOutput`).
pub fn calcs_tail(env: &Env, state: &mut PerformState, main_multipliers: &HashSet<String>) {
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy.borrow().mod_db.clone();
    let mut buff_list: Vec<String> = Vec::new();
    let mut combat_list: Vec<String> = Vec::new();
    let mut curse_list: Vec<String> = Vec::new();
    {
        let o = &state.player.output;
        for (k, label) in CHARGES {
            if positive(o, k) {
                combat_list.push(count(o, label, k));
            }
        }
        if main_multipliers.contains("InspirationCharge") {
            combat_list.push(count(o, "Inspiration Charges", "InspirationCharges"));
        }
        if positive(o, "GhostShrouds") {
            combat_list.push(count(o, "Ghost Shrouds", "GhostShrouds"));
        }
        if positive(o, "CrabBarriers") {
            combat_list.push(count(o, "Crab Barriers", "CrabBarriers"));
        }
        for (m, label, k) in [
            ("BloodCharge", "Blood Charges", "BloodCharges"),
            ("SpiritCharge", "Spirit Charges", "SpiritCharges"),
            ("SpiritInfusion", "Spirit Infusions", "SpiritInfusions"),
        ] {
            if main_multipliers.contains(m) {
                combat_list.push(count(o, label, k));
            }
        }
    }
    // :653 `env.player.mainSkill.baseSkillModList:Flag(nil, "Cruelty")`
    let st = env
        .skills
        .as_ref()
        .unwrap_or_else(|| panic!("{CALCS}:653: attempt to index field 'mainSkill' (a nil value)"));
    let main = st
        .main_skill
        .unwrap_or_else(|| panic!("{CALCS}:653: attempt to index field 'mainSkill' (a nil value)"));
    let base = st
        .skill(main)
        .skill_mod_list
        .clone()
        .unwrap_or_else(|| panic!("{CALCS}:653: attempt to index field 'baseSkillModList' (a nil value)"));
    if flag(&base, "Cruelty") {
        combat_list.push("Cruelty".into());
    }
    for (f, label) in PLAYER_FLAGS {
        if flag(&mod_db, f) {
            combat_list.push(label.into());
        }
    }
    let tabs = state.buff_tables.clone();
    // :755-761
    for (name, _) in &tabs.buffs {
        buff_list.push(name.clone());
    }
    // :758-760 (after the buff names)
    if flag(&mod_db, "Elusive") {
        combat_list.push("Elusive".into());
    }
    buff_list.sort();
    // :762-773 SkillBuffs
    let mut rows = Vec::new();
    for name in &buff_list {
        let mods = &tabs.buffs.iter().find(|(n, _)| n == name).expect("listed").1;
        eval_rows(&mod_db, mods, &mut rows);
    }
    let skill_buffs = mod_list_table(rows);
    // :774-806 SkillDebuffs
    for (name, _) in &tabs.debuffs {
        curse_list.push(name.clone());
    }
    curse_list.sort();
    let mut rows = Vec::new();
    for entry in curse_list.iter_mut() {
        let name = entry.clone();
        let mods = &tabs.debuffs.iter().find(|(n, _)| *n == name).expect("listed").1;
        eval_rows(&enemy_db, mods, &mut rows);
        // :789 `env.debuffs[name]:Sum("BASE", nil, "Multiplier:"..name.."Stack")`
        let list = ModStore::new_list(None);
        list.borrow_mut().add_list(mods);
        let stack = ok(list.borrow().sum(ModType::Base, None, &[&format!("Multiplier:{name}Stack")]));
        if stack > 0.0 {
            *entry = format!(
                "{name} ({} stack{})",
                pob_lua::format::tostring_num(stack),
                if stack > 1.0 { "s" } else { "" }
            );
        }
    }
    for (name, mods) in &tabs.curse_slots {
        curse_list.push(name.clone());
        if let Some(mods) = mods {
            eval_rows(&enemy_db, mods, &mut rows);
        }
    }
    let skill_debuffs = mod_list_table(rows);
    {
        let bd = state
            .player
            .breakdown
            .as_mut()
            .unwrap_or_else(|| panic!("{CALCS}:762: attempt to index field 'breakdown' (a nil value)"));
        bd.put("SkillBuffs", skill_buffs);
        bd.put("SkillDebuffs", skill_debuffs.clone());
        let o = &mut state.player.output;
        o.set_str("BuffList", &buff_list.join(", "));
        o.set_str("CombatList", &combat_list.join(", "));
        o.set_str("CurseList", &curse_list.join(", "));
    }
    // :810-850 the minion
    let Some(minion) = env.minion.clone() else { return };
    let mstate = match state.player.output.table("Minion") {
        Some(OutTable::Minion(m)) => m.0.clone(),
        _ => panic!("{CALCS}:813: attempt to index field 'Minion' (a nil value)"),
    };
    let mdb = minion.borrow().mod_db.clone();
    let mut buff_list: Vec<String> = Vec::new();
    let mut combat_list: Vec<String> = Vec::new();
    {
        let ms = mstate.borrow();
        for (k, label) in [
            ("PowerCharges", "Power Charges"),
            ("FrenzyCharges", "Frenzy Charges"),
            ("EnduranceCharges", "Endurance Charges"),
        ] {
            if positive(&ms.output, k) {
                combat_list.push(count(&ms.output, label, k));
            }
        }
    }
    for (f, label) in MINION_FLAGS {
        if flag(&mdb, f) {
            combat_list.push(label.into());
        }
    }
    for (name, _) in &tabs.minion_buffs {
        buff_list.push(name.clone());
    }
    buff_list.sort();
    let mut rows = Vec::new();
    for name in &buff_list {
        let mods = &tabs.minion_buffs.iter().find(|(n, _)| n == name).expect("listed").1;
        eval_rows(&mdb, mods, &mut rows);
    }
    let curse = state.player.output.get("CurseList").cloned();
    let mut ms = mstate.borrow_mut();
    let bd = ms
        .breakdown
        .as_mut()
        .unwrap_or_else(|| panic!("{CALCS}:862: attempt to index field 'breakdown' (a nil value)"));
    bd.put("SkillBuffs", mod_list_table(rows));
    // :874 `env.minion.breakdown.SkillDebuffs = env.player.breakdown.SkillDebuffs` (the same table; never written again)
    bd.put("SkillDebuffs", skill_debuffs);
    ms.output.set_str("BuffList", &buff_list.join(", "));
    ms.output.set_str("CombatList", &combat_list.join(", "));
    match curse {
        Some(crate::output::OutValue::Str(s)) => ms.output.set_str("CurseList", &s),
        _ => unreachable!("set above"),
    }
}
