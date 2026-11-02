//! CalcPerform.lua:1978-2090 -- the per-skill life/mana reservation loop over `env.player.activeSkillList` (R6.6, skills on only). The
//! accumulator set-up (:1968-1973), `doActorLifeManaReservation` (:2092, [`super::do_actor_life_mana_reservation`]) and the attribute
//! requirements (:2099-2162) stay in `perform.rs`.
//!
//! * Hash-order walk (MINEFIELD-RUST R13): `pairs(pool)` (:2005) is keyed by the strings `"Mana"`/`"Life"` -- LuaJIT's order differs per
//!   process. The two iterations are independent: each reads only its own `pool[name]` and writes only `name`-keyed state
//!   (`env.player[name.."Efficiency"]`, `reserved_<name>Base/Percent`, `uncancellable_<name>Reservation`, `skillData[name..*]`,
//!   `breakdown[name.."Reserved"]`), so the order cannot change a value; Rust walks Mana, Life (the constructor's order).
//! * `breakdown` (R8.4): the `reservations` rows of :2050-2061 / :2067-2078 (the tables themselves are created in `perform.rs`, :1974-1977).
//! * `activeSkill.skillData` is the same Lua table the mod core reads through `actor.activeSkillList` (`ModStore:GetStat`,
//!   `ManaReservedPercent`/`LifeReservedPercent` read `skillData.<pool>ReservedBase`, `skillFlags.disable`): every write here is mirrored
//!   into that snapshot right after the skill ([`sync_core`]), so a later skill's queries see it as LuaJIT does.

use std::rc::Rc;

use pob_lua::number::{floor, max2, modf, round};
use pob_lua::tostring_num;
use pob_mods::{ModCfg, ModType, StoreRef, Value};

use super::{skill_cfg_of, skill_guard, skill_list_of, skill_type_u32};
use crate::env::Env;
use crate::output::Output;
use crate::setup::ok;

const CP: &str = "Modules/CalcPerform.lua";

/// `env.player.reserved_*` / `uncancellable_*` accumulators (:1968-1973).
pub(super) struct Acc {
    pub life_base: f64,
    pub life_percent: f64,
    pub mana_base: f64,
    pub mana_percent: f64,
    pub uncancellable_life: f64,
    pub uncancellable_mana: f64,
}

/// One `pool[name]` table (:2004-2005 plus the fields the loop adds).
#[derive(Default)]
struct Pool {
    base_flat: f64,
    base_percent: f64,
}

/// The per-skill list queries, each behind the malformed-mod guard (R6.4).
struct Sml<'a> {
    list: StoreRef,
    cfg: ModCfg,
    malformed: &'a [pob_data::skill_stat_map::MalformedMod],
}

impl Sml<'_> {
    fn sum(&self, ty: ModType, names: &[&str]) -> f64 {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().sum(ty, Some(&self.cfg), names))
    }

    fn more(&self, names: &[&str]) -> f64 {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().more(Some(&self.cfg), names))
    }

    fn flag(&self, names: &[&str]) -> bool {
        skill_guard(self.malformed, names);
        ok(self.list.borrow().flag(Some(&self.cfg), names)) == Some(true)
    }
}

/// `a or b` where `a` is a `skillData` field used as a number.
fn data_or(v: Option<&Value>, fallback: f64, line: u32, field: &str) -> f64 {
    match v {
        Some(x) if x.truthy() => x
            .as_num()
            .unwrap_or_else(|| panic!("{CP}:{line}: skillData.{field} is a {} (number expected)", x.type_name())),
        _ => fallback,
    }
}

/// `skillData[k]` as a number (it is only ever written with numbers here).
fn data_num(v: &Value, line: u32, field: &str) -> f64 {
    v.as_num().unwrap_or_else(|| {
        panic!(
            "{CP}:{line}: attempt to perform arithmetic on field '{field}' (a {} value)",
            v.type_name()
        )
    })
}

/// `skillData[k] = v` where `v` may be nil (the key goes away).
fn set_or_remove(data: &mut indexmap::IndexMap<String, Value>, k: &str, v: Option<Value>) {
    match v {
        Some(x) if !x.is_nil() => {
            data.insert(k.to_owned(), x);
        }
        _ => {
            data.shift_remove(k);
        }
    }
}

/// Mirror skill `i`'s `skillData` (numbers) and `skillFlags.disable` into the mod core's `actor.activeSkillList[i]`.
fn sync_core(env: &Env, i: usize) {
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    let actor = env.player.borrow().actor.clone();
    let mut a = actor.borrow_mut();
    let core = a
        .active_skill_list
        .as_mut()
        .and_then(|l| l.get_mut(i))
        .unwrap_or_else(|| panic!("{CP}:1296: actor.activeSkillList[{}] missing", i + 1));
    core.disable = sk.skill_flags.get("disable").copied();
    core.skill_data = sk
        .skill_data
        .iter()
        .filter_map(|(k, v)| v.as_num().map(|x| (Rc::from(k.as_str()), x)))
        .collect();
}

/// :1978-2090 the loop. `output` = `env.player.output` (reads `output[name]`, :2045).
#[allow(clippy::too_many_lines)]
pub(super) fn reservation_loop(env: &mut Env, output: &Output, acc: &mut Acc, mut bd: Option<&mut crate::breakdown::Breakdown>) {
    let has_reservation = skill_type_u32("HasReservation");
    let reservation_becomes_cost = skill_type_u32("ReservationBecomesCost");
    let n_skills = env.skills.as_ref().map_or(0, |st| st.active_skill_list.len());
    let player_actor = env.player.borrow().actor.clone();
    for i in 0..n_skills {
        let list = skill_list_of(env, i);
        let st = env.skills.as_mut().expect("skills on");
        let skills = st.skills.clone();
        let cfg = skill_cfg_of(st, &st.active_skill_list[i]);
        let sk = &mut st.active_skill_list[i];
        // :2005-2008 (3dbca4463 #10305) Mana-Infused Staff needs Mana to reserve
        if sk.skill_data.get("requiresManaReservation").is_some_and(Value::truthy) && output.opt_num("Mana") == Some(0.0) {
            sk.skill_flags.insert("disable".into(), true);
            sk.disable_reason = Some("This skill requires reserving Mana".into());
            sync_core(env, i);
            continue;
        }
        // :2009
        let autoexertion = sk.skill_data.get("triggeredByAutoexertion").is_some_and(Value::truthy);
        if !((sk.skill_types.contains(&has_reservation) || autoexertion)
            && !sk.skill_types.contains(&reservation_becomes_cost)
            && !sk.flag("disable"))
        {
            continue;
        }
        let sml = Sml {
            list,
            cfg,
            malformed: &sk.malformed_mods,
        };
        // :1982 `floor(skillModList:More(skillCfg, "SupportManaMultiplier"), 4)` (Common.lua:721)
        let mult = floor(sml.more(&["SupportManaMultiplier"]), Some(4.0));
        // `activeSkill.activeEffect.grantedEffectLevel` -- indexed unconditionally.
        let (ge_idx, gel_key) = {
            let ae = sk.active_effect.borrow();
            (ae.granted_effect, ae.granted_effect_level)
        };
        let ge = &skills.effects[ge_idx];
        let ge_name = ge.name.clone();
        let gel = gel_key
            .and_then(|k| ge.level(k))
            .unwrap_or_else(|| panic!("{CP}:1984: attempt to index field 'grantedEffectLevel' (a nil value)"));
        // `grantedEffectLevel.cost`: the data table, or `{}` where ProcessSocketGroup wiped it (SkillsTab :1186-1190).
        let wiped = gel_key.is_some_and(|k| st.cost_wiped.iter().any(|(id, l)| *id == ge.id && *l == k));
        let cost_truthy = wiped || gel.cost.is_some();
        let cost_of = |pool: &str| -> f64 {
            if wiped {
                0.0
            } else {
                gel.cost.as_ref().and_then(|c| c.get(pool)).copied().unwrap_or(0.0)
            }
        };
        let mut mana = Pool::default();
        let mut life = Pool::default();
        // :1984-1988
        mana.base_flat = data_or(
            sk.skill_data.get("manaReservationFlat"),
            gel.mana_reservation_flat.unwrap_or(0.0),
            1984,
            "manaReservationFlat",
        );
        if sml.flag(&["ManaCostGainAsReservation"]) && cost_truthy {
            mana.base_flat = sml.sum(ModType::Base, &["ManaCostBase"]) + cost_of("Mana");
        }
        mana.base_percent = data_or(
            sk.skill_data.get("manaReservationPercent"),
            gel.mana_reservation_percent.unwrap_or(0.0),
            1988,
            "manaReservationPercent",
        );
        // :1989-1993 (`grantedEffectLevel.lifeReservationFlat` is not a field of the level data: nil)
        life.base_flat = data_or(sk.skill_data.get("lifeReservationFlat"), 0.0, 1989, "lifeReservationFlat");
        if sml.flag(&["LifeCostGainAsReservation"]) && cost_truthy {
            life.base_flat = sml.sum(ModType::Base, &["LifeCostBase"]) + cost_of("Life");
        }
        life.base_percent = data_or(
            sk.skill_data.get("lifeReservationPercent"),
            gel.life_reservation_percent.unwrap_or(0.0),
            1993,
            "lifeReservationPercent",
        );
        // :2024-2034 BloodMagicReserved (3dbca4463: exempts `requiresManaReservation`, no longer `ManaReservationPercentForced`)
        if sml.flag(&["BloodMagicReserved"]) && !sk.skill_data.get("requiresManaReservation").is_some_and(Value::truthy) {
            life.base_flat += mana.base_flat;
            mana.base_flat = 0.0;
            let v = sk.skill_data.get("ManaReservationFlatForced").cloned();
            set_or_remove(&mut sk.skill_data, "LifeReservationFlatForced", v);
            sk.skill_data.shift_remove("ManaReservationFlatForced");
            life.base_percent += mana.base_percent;
            mana.base_percent = 0.0;
            let v = sk.skill_data.get("ManaReservationPercentForced").cloned();
            set_or_remove(&mut sk.skill_data, "LifeReservationPercentForced", v);
            sk.skill_data.shift_remove("ManaReservationPercentForced");
        }
        // :2005-2087 `for name, values in pairs(pool)` -- order-independent (module doc).
        for (name, values) in [("Mana", &mana), ("Life", &life)] {
            let resource_more = sml.more(&[&format!("{name}Reserved")]);
            let generic_more = sml.more(&["Reserved"]);
            let inc = sml.sum(ModType::Inc, &[&format!("{name}Reserved"), "Reserved"]);
            let eff_names = [format!("{name}ReservationEfficiency"), "ReservationEfficiency".to_owned()];
            let eff_names: Vec<&str> = eff_names.iter().map(String::as_str).collect();
            let efficiency = max2(sml.sum(ModType::Inc, &eff_names), -100.0);
            let efficiency_more = sml.more(&eff_names);
            // :2014 `env.player[name.."Efficiency"]` -- the mod core reads `ManaEfficiency` (ModStore.lua:352); `LifeEfficiency` has no reader.
            if name == "Mana" {
                player_actor.borrow_mut().mana_efficiency = Some(efficiency);
            }
            // :2015-2027
            let mut reserved_flat = if let Some(f) = sk.skill_data.get(&format!("{name}ReservationFlatForced")).filter(|v| v.truthy()) {
                data_num(f, 2016, &format!("{name}ReservationFlatForced"))
            } else {
                let base_flat_val = modf(values.base_flat * mult).0;
                let mut r = 0.0;
                if base_flat_val != 0.0 {
                    // Reservation modifiers use separate integer stages before efficiency.
                    let mut modified = base_flat_val + modf(base_flat_val * inc / 100.0).0;
                    modified += modf(modified * (resource_more - 1.0)).0;
                    modified += modf(modified * (generic_more - 1.0)).0;
                    r = max2(round(modified / (1.0 + efficiency / 100.0) / efficiency_more, Some(0.0)), 0.0);
                }
                r
            };
            // :2028-2039
            let mut reserved_percent = if let Some(f) = sk.skill_data.get(&format!("{name}ReservationPercentForced")).filter(|v| v.truthy())
            {
                data_num(f, 2029, &format!("{name}ReservationPercentForced"))
            } else {
                let base_percent_val = modf(values.base_percent * 100.0 * mult).0;
                let mut r = 0.0;
                if base_percent_val != 0.0 {
                    let mut modified = base_percent_val + modf(base_percent_val * inc / 100.0).0;
                    modified += modf(modified * (resource_more - 1.0)).0;
                    modified += modf(modified * (generic_more - 1.0)).0;
                    r = max2(
                        round(modified / (1.0 + efficiency / 100.0) / efficiency_more, Some(0.0)) / 100.0,
                        0.0,
                    );
                }
                r
            };
            // :2040-2043
            if let Some(c) = sk.active_mine_count {
                reserved_flat *= c;
                reserved_percent *= c;
            }
            // :2044-2048 Blood Sacrament
            let skill_name = sk.skill_cfg.as_ref().map(|c| c.skill_name.as_str()).unwrap_or_default();
            if skill_name == "Blood Sacrament"
                && let Some(c) = sk.active_stage_count
            {
                reserved_flat *= c + 1.0;
                reserved_percent *= c + 1.0;
            }
            let (acc_base, acc_percent, acc_unc) = if name == "Mana" {
                (&mut acc.mana_base, &mut acc.mana_percent, &mut acc.uncancellable_mana)
            } else {
                (&mut acc.life_base, &mut acc.life_percent, &mut acc.uncancellable_life)
            };
            // :2049-2066
            let more = resource_more * generic_more;
            // :2051-2060 / :2068-2077 the `reservations` row (`x and ("x "..y)` is `false` when `x` fails)
            let row = |base: crate::breakdown::BVal, total: crate::breakdown::BVal| {
                use crate::breakdown::{BTable, BVal};
                let or_false = |c: bool, s: String| if c { BVal::Str(s) } else { BVal::Bool(false) };
                BTable::new()
                    .with("skillName", ge_name.as_str())
                    .with("base", base)
                    .with("mult", or_false(mult != 1.0, format!("x {}", tostring_num(mult))))
                    .with("more", or_false(more != 1.0, format!("x {}", tostring_num(more))))
                    .with("inc", or_false(inc != 0.0, format!("x {}", tostring_num(1.0 + inc / 100.0))))
                    .with(
                        "efficiency",
                        or_false(
                            efficiency != 0.0,
                            format!("x {}", tostring_num(round(100.0 / (100.0 + efficiency), Some(4.0)))),
                        ),
                    )
                    .with(
                        "efficiencyMore",
                        or_false(efficiency_more != 1.0, format!("x {}", tostring_num(efficiency_more))),
                    )
                    .with("total", total)
            };
            if reserved_flat != 0.0 {
                sk.skill_data.insert(format!("{name}ReservedBase"), Value::Num(reserved_flat));
                *acc_base += reserved_flat;
                if let Some(bd) = bd.as_deref_mut() {
                    let r = row(values.base_flat.into(), reserved_flat.into());
                    bd.table_mut(&format!("{name}Reserved")).field_mut("reservations").push(r);
                }
            }
            // :2067-2084
            if reserved_percent != 0.0 {
                sk.skill_data.insert(format!("{name}ReservedPercent"), Value::Num(reserved_percent));
                let pool_max = output.num(name);
                sk.skill_data.insert(
                    format!("{name}ReservedBase"),
                    Value::Num(reserved_flat + (pool_max * reserved_percent / 100.0).ceil()),
                );
                *acc_percent += reserved_percent;
                if let Some(bd) = bd.as_deref_mut() {
                    let r = row(
                        format!("{}%", tostring_num(values.base_percent)).into(),
                        format!("{}%", tostring_num(reserved_percent)).into(),
                    );
                    bd.table_mut(&format!("{name}Reserved")).field_mut("reservations").push(r);
                }
            }
            // :2085-2087
            if sml.flag(&["HasUncancellableReservation"]) {
                *acc_unc += reserved_percent;
            }
        }
        // (the old :2084-2087 "requires reserving Mana" check moved to :2005-2008 above in 3dbca4463)
        sync_core(env, i);
    }
}
